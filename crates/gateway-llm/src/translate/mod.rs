//! The translation core: ingress dialect ⇄ unified ⇄ provider. Pure, I/O-free
//! functions over the P1.2 types. `warn` owns the no-silent-degradation taxonomy;
//! `aggregate` stitches streamed tool-call fragments; `structured` compiles
//! structured-output plans; `ingress` holds one parser/serializer per client
//! dialect; `dialect` is the uniform switchboard P1.4 routes to. The conformance
//! harness (tests/) gates every dialect round-trip.

pub mod aggregate;
pub mod dialect;
pub mod ingress;
pub mod structured;
pub mod warn;

pub use aggregate::ToolCallAggregator;
pub use dialect::Dialect;
pub use structured::{ProviderFamily, StructuredOutputPlan};
pub use warn::{IngressError, Translated, Warning};
