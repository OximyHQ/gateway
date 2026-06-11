//! The hot-path seam. Request handlers (P1.4) call `TelemetrySink::log(row)`,
//! which is a non-blocking `try_send` into a bounded channel — it NEVER blocks
//! and NEVER errors upward. A full channel drops the row and bumps
//! `gateway_dropped_rows_total` (back-pressure must never reach a request). A
//! single background task drains the channel, enforces the capture mode, folds
//! the row into the live Prometheus aggregates, and appends it to the
//! `SpendStore`. This is the "telemetry writes off the hot path" invariant
//! (design §3/§9) made concrete.

use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::export::{Exporter, NoopExporter};
use crate::metrics::GatewayMetrics;
use crate::row::RequestLogRow;
use crate::store::SpendStore;

/// Default channel depth. Sized so a brief writer stall buffers rather than
/// drops, but bounded so a wedged writer can't grow memory unbounded.
pub const DEFAULT_CHANNEL_CAPACITY: usize = 16_384;

/// Cheap, `Clone`-able handle held by every request handler.
#[derive(Clone)]
pub struct TelemetrySink {
    tx: mpsc::Sender<RequestLogRow>,
    metrics: Arc<GatewayMetrics>,
}

impl TelemetrySink {
    /// Non-blocking. Enforces capture mode defensively, then tries to enqueue.
    /// On a full or closed channel the row is dropped and counted — the request
    /// is never affected.
    pub fn log(&self, row: RequestLogRow) {
        let row = row.enforce_capture();
        if self.tx.try_send(row).is_err() {
            self.metrics.record_dropped();
        }
    }

    /// Shared metrics handle (the `/metrics` handler renders from this).
    pub fn metrics(&self) -> Arc<GatewayMetrics> {
        Arc::clone(&self.metrics)
    }
}

/// Owns the background drain task; dropping it stops the writer once the channel
/// empties. Built once at boot and kept alive for the process lifetime.
pub struct TelemetryWriter {
    handle: JoinHandle<()>,
}

impl TelemetryWriter {
    pub fn abort(&self) {
        self.handle.abort();
    }
}

/// Wire up the sink + writer over a store + metrics with NO external export
/// (the shipped default — standalone posture).
pub fn spawn<S: SpendStore + 'static>(
    store: Arc<S>,
    metrics: Arc<GatewayMetrics>,
    capacity: usize,
) -> (TelemetrySink, TelemetryWriter) {
    spawn_with_exporter(store, metrics, Box::new(NoopExporter), capacity)
}

/// Wire up the sink + writer with an explicit exporter (e.g. OTLP or Oximy
/// substrate when an operator opts in via `ExportConfig`).
pub fn spawn_with_exporter<S: SpendStore + 'static>(
    store: Arc<S>,
    metrics: Arc<GatewayMetrics>,
    exporter: Box<dyn Exporter>,
    capacity: usize,
) -> (TelemetrySink, TelemetryWriter) {
    let (tx, mut rx) = mpsc::channel::<RequestLogRow>(capacity);
    let writer_metrics = Arc::clone(&metrics);
    let handle = tokio::spawn(async move {
        // Drain in small batches to amortize lock acquisition under load.
        let mut batch: Vec<RequestLogRow> = Vec::with_capacity(256);
        loop {
            let n = rx.recv_many(&mut batch, 256).await;
            if n == 0 {
                break; // all senders dropped
            }
            for row in batch.drain(..) {
                writer_metrics.record(&row);
                exporter.export(&row);
                store.append(row);
            }
        }
    });
    (TelemetrySink { tx, metrics }, TelemetryWriter { handle })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::Exporter;
    use crate::metrics::GatewayMetrics;
    use crate::row::{CacheStatus, CaptureMode, RequestKind};
    use crate::store::{GroupBy, MemorySpendStore, TimeRange};
    use gateway_spine::{TokenUsage, Usd};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn row(key: &str, cost: i64, mode: CaptureMode) -> RequestLogRow {
        RequestLogRow {
            ts_ms: 1,
            kind: RequestKind::Llm,
            key_id: key.into(),
            team_id: None,
            user_id: None,
            tags: vec![],
            model: "gpt-4o".into(),
            provider: "openai".into(),
            usage: TokenUsage {
                input_tokens: 100,
                output_tokens: 50,
                ..Default::default()
            },
            cost: Usd::from_micros(cost),
            latency_ms: 10,
            ttft_ms: None,
            status: 200,
            served_by: "openai".into(),
            fallback_fired: false,
            cache_status: CacheStatus::Miss,
            capture_mode: mode,
            request_text: Some("secret prompt".into()),
            response_text: Some("secret reply".into()),
        }
    }

    async fn drain(store: &Arc<MemorySpendStore>, expected: usize) {
        for _ in 0..1000 {
            if store.row_count() >= expected {
                return;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        panic!("writer did not drain {expected} rows in time");
    }

    #[tokio::test]
    async fn logged_rows_reach_the_store_and_metrics() {
        let store = Arc::new(MemorySpendStore::new());
        let metrics = Arc::new(GatewayMetrics::new());
        let (sink, _writer) = spawn(Arc::clone(&store), Arc::clone(&metrics), 1024);

        sink.log(row("k1", 100, CaptureMode::Metadata));
        sink.log(row("k1", 200, CaptureMode::Metadata));
        drain(&store, 2).await;

        let buckets = store.query(GroupBy::Key, TimeRange::default(), None);
        assert_eq!(buckets[0].cost, Usd::from_micros(300));
        assert!(metrics.render().contains("gateway_requests_total"));
    }

    #[tokio::test]
    async fn metadata_mode_strips_text_before_storage() {
        let store = Arc::new(MemorySpendStore::new());
        let metrics = Arc::new(GatewayMetrics::new());
        let (sink, _writer) = spawn(Arc::clone(&store), Arc::clone(&metrics), 1024);

        sink.log(row("k1", 100, CaptureMode::Metadata));
        drain(&store, 1).await;

        let recent = store.recent(TimeRange::default(), 1);
        assert!(recent[0].request_text.is_none());
        assert!(recent[0].response_text.is_none());
    }

    #[tokio::test]
    async fn full_mode_preserves_text() {
        let store = Arc::new(MemorySpendStore::new());
        let metrics = Arc::new(GatewayMetrics::new());
        let (sink, _writer) = spawn(Arc::clone(&store), Arc::clone(&metrics), 1024);

        sink.log(row("k1", 100, CaptureMode::Full));
        drain(&store, 1).await;

        let recent = store.recent(TimeRange::default(), 1);
        assert_eq!(recent[0].request_text.as_deref(), Some("secret prompt"));
    }

    #[tokio::test]
    async fn full_channel_drops_and_counts_never_errors() {
        // Capacity 1, no writer running: the channel fills and `log` must still
        // return without panicking, bumping the dropped counter.
        let metrics = Arc::new(GatewayMetrics::new());
        let (tx, _rx) = mpsc::channel::<RequestLogRow>(1);
        let sink = TelemetrySink {
            tx,
            metrics: Arc::clone(&metrics),
        };

        // First fills the buffer, the rest are dropped — none block or error.
        for _ in 0..100 {
            sink.log(row("k1", 1, CaptureMode::Metadata));
        }
        assert!(metrics.render().contains("gateway_dropped_rows_total"));
        // At least 99 dropped (buffer holds 1).
        let text = metrics.render();
        let dropped_line = text
            .lines()
            .find(|l| l.starts_with("gateway_dropped_rows_total "))
            .expect("dropped counter present");
        let n: i64 = dropped_line.rsplit(' ').next().unwrap().parse().unwrap();
        assert!(n >= 99, "expected >=99 drops, got {n}");
    }

    #[tokio::test]
    async fn writer_fans_out_to_exporter() {
        struct CountingExporter(Arc<AtomicUsize>);
        impl Exporter for CountingExporter {
            fn export(&self, _row: &RequestLogRow) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let store = Arc::new(MemorySpendStore::new());
        let metrics = Arc::new(GatewayMetrics::new());
        let count = Arc::new(AtomicUsize::new(0));
        let exporter: Box<dyn Exporter> = Box::new(CountingExporter(Arc::clone(&count)));
        let (sink, _writer) =
            spawn_with_exporter(Arc::clone(&store), Arc::clone(&metrics), exporter, 1024);

        sink.log(row("k1", 100, CaptureMode::Metadata));
        sink.log(row("k1", 200, CaptureMode::Metadata));
        drain(&store, 2).await;

        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
}
