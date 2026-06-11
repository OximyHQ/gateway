# Kubernetes-Native Inference Routing — Competitive Intelligence Report

**Scope:** llm-d, AIBrix, NVIDIA Dynamo, Gateway API Inference Extension (GIE), vLLM Semantic Router, RouteLLM
**Why this matters to us:** This is the self-hosted-fleet gateway layer that most "AI gateway" feature matrices miss entirely. If our gateway treats self-hosted vLLM/SGLang endpoints as dumb OpenAI-compatible URLs, we are leaving 2-7x performance on the table versus what these systems achieve with KV-cache-aware, load-aware, and prefill/decode-disaggregated routing. A comprehensive gateway needs self-hosted endpoints as first-class peers of SaaS providers.
**Date:** 2026-06-10

---

## 1. The category in one paragraph

SaaS-oriented gateways (LiteLLM, Portkey, OpenRouter-style) route on price, availability, and static weights because the provider is a black box. When you own the fleet, the routing decision space explodes: which replica has the request's prefix already in KV cache, which replica's queue is shortest, which pod has the right LoRA adapter loaded, whether to split prefill and decode across different GPU pools, and whether the query even needs the big model at all. Two architectural camps have emerged: **stack-centric Kubernetes-native** (llm-d, AIBrix — CRDs + Envoy ext-proc + scheduler) and **pipeline-centric SDK/runtime** (NVIDIA Dynamo — Rust runtime + planner + NIXL transfer library). Both converge on the **Gateway API Inference Extension** as the standard north-south interface, and a separate **semantic/quality routing** layer (vLLM Semantic Router, RouteLLM) sits in front deciding *which model*, not *which replica*.

---

## 2. Gateway API Inference Extension (GIE) — the standard

**What:** Official Kubernetes SIG project (kubernetes-sigs/gateway-api-inference-extension). Extends the Gateway API so a Gateway can route to an **InferencePool** (a set of model-serving pods) instead of a plain Service. GA / v1 API for InferencePool; actively developed (flow control, predicted latency landing in recent releases).

**API types:**
- `InferencePool` (v1, stable) — pool of pods sharing base model, accelerator type, and model-server config; referenced as a Gateway API backendRef.
- `InferenceObjective` — per-model serving properties, notably **Priority** (criticality classes; low-priority traffic is shed/queued under load).
- `InferencePoolImport`, `InferenceModelRewrite` — multi-cluster import and model-name rewrite resources (newer).

**Architecture:** the gateway (any ext-proc-capable proxy) calls an **Endpoint Picker (EPP)** over the Envoy External Processing protocol. The EPP scrapes per-pod model-server metrics — KV-cache utilization, pending-queue depth, loaded LoRA adapters, model-load readiness — and tells the proxy exactly which pod gets the request. EPP is explicitly **pluggable**: scorers/filters/pickers are plugins (prefix-cache scorer, load scorer, LoRA-affinity scorer, latency predictor).

**Key features:** model-aware routing (routes on `model` field in the request body, i.e. body-based routing); serving priority/criticality; LoRA-adapter-aware scheduling (route to pod that already has the adapter loaded, treat adapters as virtual models); safe model rollouts via traffic splitting between pool versions; flow control / request queueing; metrics standardization for model servers.

**Implementations:** Envoy Gateway (via Envoy AI Gateway), **kgateway**, **GKE Inference Gateway** (Google's managed productization), Istio, NGINX Gateway Fabric, Alibaba Higress, KAITO integration. Dynamo and llm-d both plug in as EPP implementations.

**Strategic read:** GIE is becoming the POSIX of self-hosted inference routing. Any new gateway that wants credibility with platform teams should either (a) implement the EPP protocol so it can *be* an inference gateway on k8s, or (b) at minimum speak to InferencePool-backed gateways as upstream targets and consume the same model-server metrics (vLLM exposes them on a standardized endpoint).

---

## 3. llm-d (Red Hat, Google, IBM, CoreWeave, NVIDIA)

**What:** "Kubernetes-native high-performance distributed LLM inference framework." Built on vLLM + Kubernetes + Inference Gateway (GIE). Donated to **CNCF Sandbox March 24, 2026**. Current version **v0.7.0 (May 2026)**. Apache 2.0. Repo is mostly Helm/Kustomize + Python (the heavy lifting is in vLLM and the Go EPP from GIE).

**Core concepts — "well-lit paths"** (opinionated, benchmarked recipes rather than a monolith):
1. **Intelligent inference scheduling** — GIE EPP with prefix-cache-aware + load-aware scoring; routes to pods that already hold the prompt prefix in KV cache; understands model readiness, real inference queue depth, and hardware topology. Experimental **predicted-latency-based scheduling** (claimed 40% reduction in TTFT and ITL).
2. **Prefill/decode disaggregation** — separate prefill and decode pools, KV transferred over fast interconnect (claimed up to 70% higher tokens/sec on GPT-OSS).
3. **Wide expert-parallelism** — for MoE models like DeepSeek-R1 across nodes (~3.1k tok/s per B200 decode GPU; 50k tok/s on a 16×16 B200 cluster).

**Other features:** hierarchical/tiered KV-cache offloading to CPU/disk with global cache indexing (claimed **13.9x throughput** vs GPU-only in cache-heavy workloads); cache-aware LoRA routing; active-active HA; UCCL transport resilience; scale-to-zero and SLO-aware autoscaling on inference signals; multi-tenant serving with flow control; reproducible benchmark workflows (inference-perf); OpenAI-compatible batch API for offline inference; recipes for DeepSeek-R1/GPT-OSS/Llama; hardware coverage NVIDIA H100/H200/B200, AMD MI300X, Intel XPU, Google TPU.

**Deployment model:** Helm charts → Kustomize-first guides (v0.7+); explicitly targets "large inference deployers" (maintainer on HN: ~5+ full H100 hosts) with existing Kubernetes maturity.

**Published performance:** 3x output throughput + 2x faster TTFT (Llama 3.1 70B on 4× MI300X) vs baseline; numbers above.

**Weaknesses/complaints:** high barrier to entry — explicitly not for small fleets; stack-centric design questioned on HN vs Dynamo's SDK approach ("why not vLLM on Ray?"); young project with rapidly shifting deploy tooling (Helm→Kustomize churn); no real dashboard of its own (relies on Grafana); vLLM-only in practice despite SGLang mentions.

---

## 4. AIBrix (ByteDance → vllm-project)

**What:** "Battery-included" cost-efficient control plane for vLLM on Kubernetes, born inside ByteDance (production there since 2024), now under the vllm-project org. Apache 2.0, Go (controllers/gateway plugins) + Python (runtime). **v0.6.0 (March 5, 2026)**. Has an arXiv whitepaper.

**Feature surface:**
- **LLM gateway & routing:** extends **Envoy Gateway** with LLM-aware routing strategies — random, least-request, throughput-weighted, **prefix-cache-aware**, and **fairness routing** (v0.3); v0.6 added **Envoy sidecar mode, mixed-LLM-workload routing, routing profiles, and new APIs**.
- **High-density LoRA management:** dynamic LoRA registration/scheduling, many adapters per base-model pod, LoRA-aware routing + cost amortization; v0.6 "LoRA delivery."
- **LLM-app-tailored autoscaler:** scales on LLM-specific signals (KV utilization, queue), not CPU; supports proactive/optimizer-driven scaling.
- **Distributed KV cache:** L1 DRAM tier + optional L2 remote tier for **cross-engine KV reuse** (multi-node sharing); KVCache-offloading framework (v0.3) with vLLM connector.
- **Mixed-grain orchestration:** Ray (fine-grained) inside Kubernetes (coarse-grained) for multi-node inference; P/D disaggregation since v0.4.
- **Cost-efficient heterogeneous serving:** GPU-optimizer that picks mixed GPU types under SLO constraints.
- **Unified AI runtime sidecar:** metrics standardization, model downloading (HF/S3/TOS), engine management.
- **GPU hardware failure detection** and simulator tooling; benchmarking suite.

**Deployment:** two manifests (dependencies + core) or Helm; runs on any k8s incl. cloud guides. Has a Grafana-based monitoring/dashboard setup, not a bespoke UI.

**Weaknesses:** vLLM-centric (other engines second-class); newer than its production pedigree suggests outside ByteDance-scale shops; setup is multi-CRD and non-trivial; documentation lags features; no semantic/quality routing layer (replica-level only); no native non-k8s story.

---

## 5. NVIDIA Dynamo

**What:** Datacenter-scale distributed inference serving framework ("operating system of the AI factory"), announced GTC March 2025, **1.0 GA March 2026, v1.2.0 June 2, 2026**. Apache 2.0. **Rust core (53%) + Python (33%) + Go (12%)**. Backend-agnostic by design.

**Components:**
- **Frontend:** OpenAI-compatible HTTP API (OpenAPI 3 spec served at `/openapi.json`); runs standalone or as gateway sidecar in k8s.
- **Smart Router:** KV-aware routing on worker load + **KV-cache overlap** (radix-tree of cached blocks across the fleet) — claimed **2x faster TTFT**; v1.2 adds **multimodal KV routing** (image-content-aware cache overlap; +30% TTFT, +25% throughput on Qwen3-VL on GB200).
- **Planner:** SLA-driven autoscaler — profiles the workload, right-sizes prefill vs decode pools to hit TTFT/ITL targets at minimum cost; runtime-reconfigurable **xPyD** (x prefill / y decode workers, add/remove at runtime).
- **KVBM (KV Block Manager):** KV offload across GPU → CPU → SSD → remote storage.
- **NIXL:** async GPU-to-GPU KV transfer library (VRAM-to-VRAM prefill→decode handoff over NVLink/IB/Ethernet).
- **ModelExpress:** weight streaming for **7x faster replica cold-start**.

**Backends:** vLLM, SGLang, TensorRT-LLM — all with full disaggregation/KV-routing support; tool calling and agentic inference supported (SGLang path); multimodal + video generation.

**Deployment:** Kubernetes operator + CRDs, or standalone CLI/SDK ("pipeline-centric" — easier single-team onboarding than llm-d); also integrates *behind* GIE as an endpoint-picker plugin (NVIDIA, Google, Red Hat collaborate here — Dynamo and llm-d share GIE rather than competing on the API).

**Published performance:** up to **7x throughput per GPU** (DeepSeek-R1 on GB200 NVL72); marketing claims up to 750x aggregate on GB300 NVL72; 2x TTFT from KV routing; 7x cold-start.

**Weaknesses:** complexity is its Achilles heel for multi-node reasoning deployments (etcd + NATS + operator + per-backend images); NVIDIA-hardware gravity (works elsewhere but tuned for NVIDIA interconnects); Kubernetes story younger than its standalone story; observability assembled from Prometheus/Grafana parts, no product dashboard.

---

## 6. vLLM Semantic Router (v0.2 "Athena", now v0.3.0 June 2026)

**What:** "System-level intelligent router for Mixture-of-Models" — an Envoy **ext-proc** service that classifies each request and picks the right *model* (and settings), complementing the replica-level routers above. vllm-project org, Apache 2.0, **Go (46%) + Rust candle/ONNX/OpenVINO bindings + TypeScript dashboard**. 4.3k stars; AMD is the primary infra sponsor. Production-grade ambitions ("the system brain").

**Signal layer (Athena):** eight neural classifiers on a new **mmBERT-32K** multilingual backbone (307M, 1800+ languages, 32K context) + a 120M multimodal embedder — intent, PII, jailbreak, fact-check/hallucination-risk, feedback, language, modality, preference. Signals extracted **in parallel**, combined with symbolic logic: boolean expression trees over keyword (BM25/fuzzy/regex) + embedding-similarity matches.

**Decision layer:** multi-dimensional routing on intent probability, semantic-cache hit, safety tags, user role, text length, latency-awareness; **reasoning-mode routing** (turn on/off thinking modes per query — accepted paper); auto tool selection; cache-aware decisions.

**State layer:** category-aware **semantic caching** (published paper); **agentic memory** on Milvus with hybrid (vector+BM25+RRF) retrieval, memory scoring, MINJA injection defenses, response-level jailbreak gating before storage.

**Dashboard ("System Brain"):** topology visualization with live test queries, **Router Replay** (step-through of why a routing decision was made), evaluation surfaces, reasoning-aware playground, read-only demo mode. Public playground at play.vllm-semantic-router.com.

**Agent angle — most interesting in the whole space:** **ClawOS** — experimental natural-language OS for spawning and coordinating teams of OpenClaw agents (leader/worker hierarchies, shared rooms, per-team isolation) with the router providing cost/quality routing, guardrails, and hierarchical memory underneath. Roadmap: a coding agent that **writes and revises the routing DSL from natural-language requirements**, and self-learning loops that tune routing from outcome signals.

**Performance:** ONNX+GPU on MI300X: 22ms classification at ~500 tokens (vs 853ms CPU); 128ms at 8K tokens; prompt compression for the signal path cut jailbreak extraction 127ms→10ms and end-to-end 143ms→103ms; 20 concurrent 32K requests with zero OOM.

**Weaknesses:** young and fast-moving (v0.x, big API churn per release); Envoy-coupled deployment; classifier quality is workload-dependent and needs tuning/fine-tuning; heavyweight footprint (GPU recommended for the router itself at scale).

---

## 7. RouteLLM (LMSYS)

**What:** The academic ancestor of quality/cost routing — a framework for serving and evaluating routers between a strong (expensive) and weak (cheap) model pair. Python, ~5k stars. Research-grade; activity has slowed since 2024-2025 (effectively in maintenance mode; superseded in production usage by semantic-router-class systems and commercial routers it benchmarked against).

**Features:** four trained routers — **matrix factorization (recommended), BERT classifier, causal-LLM classifier, similarity-weighted Elo ranking** — trained on Chatbot Arena preference data; routers **generalize to new model pairs without retraining**; per-request cost threshold knob + a **calibration tool** ("route 50% of traffic to the strong model on this workload" → threshold); lightweight **OpenAI-compatible server** as a drop-in client replacement; LiteLLM for provider breadth; local models via Ollama/any OpenAI-compatible base URL; evaluation harness over MMLU/GSM8K/MT-Bench with caching and plots.

**Published performance:** 95% of GPT-4 quality with 26% GPT-4 calls (≈48% cheaper than random); up to 85% cost reduction at 95% quality on some benchmarks; claimed >40% cheaper than commercial routers at equal quality.

**Weaknesses:** two-model-pair abstraction only (no N-way routing); trained on dated preference data; no streaming-era production hardening, no governance, no k8s story; dormant compared to the rest of this report.

---

## 8. What a comprehensive gateway needs to treat self-hosted fleets as first-class (synthesis)

1. **Speak GIE both ways.** Consume InferencePool-backed gateways as upstreams, and offer an EPP-protocol mode so our gateway can *be* the inference gateway on k8s. This is the standardized hook — not proprietary.
2. **Metrics-aware endpoint scoring.** Scrape vLLM/SGLang per-pod metrics (KV utilization, queue depth, loaded LoRAs, readiness) and score replicas; even approximate prefix-affinity (consistent hashing on prompt prefix) captures much of the KV-cache win without a cache index.
3. **Model-level + replica-level routing as separate, composable layers.** Semantic/quality routing (which model) → fleet routing (which replica). Nobody in this space does both well in one product; that is the open seam.
4. **Priority/criticality + flow control.** GIE's InferenceObjective priority + queue-shedding is table stakes for shared fleets.
5. **LoRA-as-virtual-model.** Adapter-aware routing and adapter registries are a recurring first-class concept (GIE, AIBrix, llm-d).
6. **Don't rebuild the heavy machinery.** P/D disaggregation, KV offload tiering, NIXL-style transfer belong to llm-d/Dynamo/AIBrix; the gateway's job is to route *into* them intelligently and expose their health/cost upward.
7. **Cost parity for self-hosted.** None of these systems produce per-request $ cost for self-hosted tokens (GPU-hour amortization). A gateway that unifies SaaS token pricing with self-hosted GPU economics in one ledger is differentiated.
8. **Agent-facing control plane is wide open.** Everything here is configured via CRDs/Helm/YAML for human platform engineers; only Semantic Router gestures at NL-driven config (routing-DSL-writing agent, ClawOS). No MCP control surface exists anywhere in the category.

---

## 9. AX (agent-experience) observations

- Configuration surface across llm-d/AIBrix/GIE is **CRDs + Helm/Kustomize** — fully declarative and machine-writable, but designed for GitOps pipelines, not interactive agents; no MCP servers, no NL interfaces.
- Dynamo is the most API-first runtime: OpenAI-compatible frontend with a served **OpenAPI 3 spec at /openapi.json**, Rust/Python SDK, runtime-reconfigurable worker topology — good bones for agent control.
- vLLM Semantic Router is the only project explicitly designing *for* agents: ClawOS multi-agent orchestration, Router Replay for decision explainability (great for agent debugging), and a roadmap item where an agent writes the routing DSL from natural language.
- GIE's EPP plugin protocol (Envoy ext-proc) is the extension point a new gateway should target; it is gRPC, well-specified, and implementation-agnostic.
- RouteLLM is a Python library + simple OpenAI proxy; trivially scriptable but no operational API.

## 10. Sources (primary)

- https://github.com/llm-d/llm-d ; https://llm-d.ai/blog/kvcache-wins-you-can-see ; Red Hat Developer KV-cache-aware routing article
- https://github.com/vllm-project/aibrix ; aibrix.github.io release blogs (v0.2/v0.3/v0.6) ; arXiv 2504.03648
- https://github.com/ai-dynamo/dynamo ; docs.dynamo.nvidia.com (disaggregated serving) ; NVIDIA Dynamo 1.0 GA blog
- https://gateway-api-inference-extension.sigs.k8s.io/ ; kubernetes.io blog "Introducing Gateway API Inference Extension" ; kgateway.dev deep-dive ; aigateway.envoyproxy.io endpoint-picker blog
- https://github.com/vllm-project/semantic-router ; vllm.ai blog "v0.2 Athena" ; Red Hat Developer Athena getting-started
- https://github.com/lm-sys/RouteLLM ; lmsys.org RouteLLM blog
- HN threads 44040883 / 44043135 (llm-d launch discussion); thenewstack.io "Six Frameworks for Efficient LLM Inferencing"
