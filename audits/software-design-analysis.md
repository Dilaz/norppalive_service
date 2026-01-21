# Software Design Analysis Report

**Project:** Norppalive Service
**Date:** 2026-01-21
**Analyst:** Automated Architecture Review

---

## Executive Summary

| Metric | Score | Notes |
|--------|-------|-------|
| **Modularity** | 8/10 | Well-structured actor system with clear boundaries |
| **Separation of Concerns** | 8/10 | Actors have focused responsibilities |
| **Dependency Management** | 9/10 | No circular dependencies detected |
| **Error Handling** | 9/10 | Comprehensive error types with diagnostics |
| **Code Duplication** | 5/10 | Significant duplication in service actors |

---

## 1. Architecture Overview

### Pattern Identified: **Actor-Based Architecture (Actix)**

The project implements a supervisor-based actor system with clear hierarchical communication:

```
┌─────────────────────────────────────────────────────────────────┐
│                     SupervisorActor                              │
│              (Health monitoring, lifecycle)                      │
└───────────────────────────┬─────────────────────────────────────┘
                            │
            ┌───────────────┼───────────────┐
            ▼               ▼               ▼
     ┌──────────┐    ┌─────────────┐  ┌───────────┐
     │StreamActor│───▶│DetectionActor│─▶│OutputActor │
     │  (FFmpeg) │    │  (AI API)    │  │ (Coord.)  │
     └──────────┘    └─────────────┘  └─────┬─────┘
                                            │
              ┌─────────────┬───────────────┼───────────────┐
              ▼             ▼               ▼               ▼
        ┌──────────┐  ┌──────────┐   ┌───────────┐   ┌─────────┐
        │Twitter   │  │Bluesky   │   │Mastodon   │   │Kafka    │
        │Actor     │  │Actor     │   │Actor      │   │Actor    │
        └──────────┘  └──────────┘   └───────────┘   └─────────┘
```

### Data Flow

```
Video Stream → FFmpeg (blocking) → Frame → AI Detection → Filtering → Annotation → Social Media
```

---

## 2. Separation of Concerns Analysis

### Layer Responsibilities

| Layer | Directory | Purpose | Rating |
|-------|-----------|---------|--------|
| Actors | `src/actors/` | Message handling, lifecycle | Good |
| Services | `src/services/` | Business logic (API calls) | Good |
| Messages | `src/messages/` | Type definitions | Good |
| Utils | `src/utils/` | Shared utilities | Good |
| Config | `src/config.rs` | Configuration loading | Adequate |

**Assessment:** Clear separation between actors (coordination) and services (business logic). The trait pattern (`SocialMediaService`, `OutputServiceTrait`) enables testability.

---

## 3. Circular Dependencies

**Status: NONE DETECTED**

The dependency graph is acyclic:

```
main.rs
  └─▶ actors/* (one-way)
       └─▶ messages/* (shared types)
            └─▶ services/* (business logic)
                 └─▶ error.rs (shared error)
```

Key patterns that prevent cycles:
- Actors communicate via `Addr<T>` references, not direct imports
- `reply_to` pattern for bidirectional communication (`stream.rs:ProcessFrame`)
- Supervisor uses type-erased `Arc<dyn Any>` for generic actor storage

---

## 4. Modularity Rating: **8/10**

### Strengths

1. **Actor Isolation:** Each actor has single responsibility
2. **Trait Abstraction:** `SocialMediaService` enables service swapping
3. **Message Bus Abstraction:** `src/message_bus/` prepared for future HTTP-based comms
4. **Factory Pattern:** Service actors use factories for restart capability

### Weaknesses

1. **Global CONFIG Singleton:** Tight coupling to static config
2. **Service Actor Duplication:** Near-identical code across 4 actors
3. **Type Erasure Fragility:** Downcast failures silently ignored

---

## 5. Anti-Patterns Identified

### 5.1 Copy-Paste Programming

**Severity: 6/10**

**Location:** `src/actors/services/` - BlueskyActor, MastodonActor, KafkaActor are nearly identical

**Evidence:**
```rust
// bluesky.rs:43-97 vs mastodon.rs:43-97 vs kafka.rs:41-106
// ~95% identical Handler<ServicePost> implementation
```

| File | Lines | Unique Logic |
|------|-------|-------------|
| `twitter.rs` | 185 | Rate limiting, health checks |
| `bluesky.rs` | 107 | None (template copy) |
| `mastodon.rs` | 107 | None (template copy) |
| `kafka.rs` | 116 | Slightly different error handling |

**Remediation:**
```rust
// Option 1: Generic ServiceActor<T: SocialMediaService>
pub struct ServiceActor<S: SocialMediaService + Clone + 'static> {
    service: S,
    status: ServiceStatus,
    rate_limiter: Option<RateLimiter>,
}

// Option 2: Procedural macro
#[derive(ServiceActor)]
#[service_actor(rate_limit = "30m")] // optional
pub struct TwitterActor(TwitterService);
```

---

### 5.2 Global Singleton Configuration

**Severity: 5/10**

**Location:** `src/config.rs:6-16`

```rust
lazy_static! {
    pub static ref CONFIG: Config = {
        // Panics on failure, no reload capability
        let config_path = std::env::var("CONFIG_PATH")...;
        let config_content = std::fs::read_to_string(&config_path)
            .unwrap_or_else(|_| panic!(...));
        toml::from_str(&config_content)
            .unwrap_or_else(|_| panic!(...))
    };
}
```

**Issues:**
- Panics on config errors instead of graceful handling
- No hot-reload capability
- Makes testing harder (can't inject different configs)
- 35+ direct `CONFIG.*` references throughout codebase

**Remediation:**
```rust
// Pass config through actor constructors instead
impl StreamActor {
    pub fn new(config: StreamConfig, ...) -> Self { ... }
}

// Or use Arc<Config> passed at startup
pub struct AppState {
    pub config: Arc<Config>,
}
```

---

### 5.3 Type-Erased Address Downcast Without Validation

**Severity: 4/10**

**Location:** `src/actors/output.rs:366-410`

```rust
// Silent failure if type doesn't match
if let Some(addr) = msg.new_address
    .downcast_ref::<Addr<TwitterActor>>()
    .cloned()
{
    self.twitter_actor = Some(addr);
} else {
    warn!("Failed to downcast new TwitterActor address");  // Continues with stale address
}
```

**Issue:** On downcast failure, the old (potentially dead) address is retained, leading to silent message delivery failures.

**Remediation:**
```rust
// Set to None on downcast failure to fail fast
"TwitterActor" => {
    self.twitter_actor = msg.new_address
        .downcast_ref::<Addr<TwitterActor>>()
        .cloned();
    if self.twitter_actor.is_none() {
        error!("Type mismatch: TwitterActor address downcast failed");
    }
}
```

---

### 5.4 Core Actors Cannot Auto-Restart

**Severity: 6/10**

**Location:** `src/main.rs:145-148`

```rust
// Register core actors without factories (complex dependencies make restart harder)
supervisor.do_send(RegisterActor::new("StreamActor"));
supervisor.do_send(RegisterActor::new("DetectionActor"));
supervisor.do_send(RegisterActor::new("OutputActor"));
```

**Issue:** If StreamActor, DetectionActor, or OutputActor fail, the system cannot recover without manual restart. These are the most critical actors.

**Remediation:**
```rust
// Create core actor factories that capture dependencies
let stream_factory: ActorFactoryFn = {
    let detection = detection_actor.clone();
    let supervisor = supervisor.clone();
    Arc::new(move || {
        Arc::new(StreamActor::with_actors(detection.clone(), supervisor.clone()).start())
    })
};
supervisor.do_send(RegisterActor::with_factory("StreamActor", stream_factory));
```

---

### 5.5 Missing Abstraction: OutputActor Service Dispatch

**Severity: 4/10**

**Location:** `src/actors/output.rs:340-351`

```rust
// Manual dispatch to each service
if let Some(ref twitter) = self.twitter_actor {
    twitter.do_send(service_post.clone());
}
if let Some(ref bluesky) = self.bluesky_actor {
    bluesky.do_send(service_post.clone());
}
if let Some(ref mastodon) = self.mastodon_actor {
    mastodon.do_send(service_post.clone());
}
if let Some(ref kafka) = self.kafka_actor {
    kafka.do_send(service_post);
}
```

**Remediation:**
```rust
// Use a Vec of Recipients
struct OutputActor {
    service_recipients: Vec<Recipient<ServicePost>>,
}

// In handler:
for recipient in &self.service_recipients {
    let _ = recipient.try_send(service_post.clone());
}
```

---

### 5.6 Handler<ActorRestarted> String Matching

**Severity: 3/10**

**Location:** `src/actors/output.rs:366-417`

```rust
match msg.actor_name.as_str() {
    "TwitterActor" => { ... }
    "BlueskyActor" => { ... }
    // Fragile: typo in registration breaks restart handling
}
```

**Remediation:**
```rust
// Use constants or an enum
pub const TWITTER_ACTOR_NAME: &str = "TwitterActor";
// Or better: use TypeId for type-safe matching
```

---

## 6. Potential Bottlenecks

### 6.1 FFmpeg Blocking Thread Pool

**Location:** `src/actors/stream.rs`

FFmpeg processing runs in `spawn_blocking()`. If frame processing is slow, it can exhaust the blocking thread pool (default: 512 threads).

**Mitigation:** The code enforces `frame_processing_delay_ms` (config) between frames.

### 6.2 Detection Service Mutex Contention

**Location:** `src/actors/detection.rs:19`

```rust
detection_service: Arc<Mutex<dyn DetectionServiceTrait + Send>>
```

Single mutex for all detection requests. If AI API latency is high, this serializes all detections.

**Mitigation:** Detection is async; lock is held only during API call setup, not await.

### 6.3 Image Clone on Detection

**Location:** `src/actors/output.rs:284`

```rust
let image = (*msg.image_data).clone();  // Full image copy
```

Each detection triggers a full `DynamicImage` clone for annotation.

**Potential optimization:** Use `Arc<RwLock<DynamicImage>>` or copy-on-write.

---

## 7. External Service Integrations

| Service | Library | Config Location | Notes |
|---------|---------|-----------------|-------|
| Twitter | `twitter-v1`, `twitter-v2` | `config.twitter.*` | 30-min rate limit enforced |
| Bluesky | `atrium-api` | `config.bluesky.*` | Session-based auth |
| Mastodon | `megalodon` | `config.mastodon.*` | ActivityPub compatible |
| Kafka | `rdkafka` | `config.kafka.*` | Dual topic support |
| AI Detection | HTTP (reqwest) | `config.detection.api_url` | External ML endpoint |

---

## 8. Findings Summary

| # | Finding | Severity | Location | Status |
|---|---------|----------|----------|--------|
| 1 | Service actor code duplication | 6/10 | `src/actors/services/*.rs` | Needs refactor |
| 2 | Global CONFIG singleton | 5/10 | `src/config.rs:6-16` | Design debt |
| 3 | Core actors cannot auto-restart | 6/10 | `src/main.rs:145-148` | Risk |
| 4 | Type-erased downcast silent failure | 4/10 | `src/actors/output.rs:366-410` | Bug risk |
| 5 | Manual service dispatch | 4/10 | `src/actors/output.rs:340-351` | Maintainability |
| 6 | String-based actor name matching | 3/10 | `src/actors/output.rs:366-417` | Fragility |
| 7 | Full image clone on detection | 3/10 | `src/actors/output.rs:284` | Performance |

---

## 9. Positive Patterns

1. **Comprehensive Error Types:** `NorppaliveError` with 13 variants, boxed for size optimization
2. **Diagnostic Support:** All errors implement `miette::Diagnostic` for helpful messages
3. **Graceful Shutdown:** `GracefulStop` message with ACK pattern and 5s timeout
4. **Factory-Based Restart:** Service actors can auto-recover from failures
5. **Rate Limiting:** Twitter actor enforces 30-minute minimum between posts
6. **Health Monitoring:** Supervisor runs 30-second health checks
7. **Clean Module Organization:** Clear separation between actors, services, messages, and utils
8. **Test Coverage:** Comprehensive tests using `#[actix::test]` and mock services

---

## 10. Architecture Diagram

```
                              ┌─────────────────────┐
                              │   CONFIG (Lazy)     │
                              │   config.toml       │
                              └──────────┬──────────┘
                                         │ read once
                                         ▼
┌────────────────────────────────────────────────────────────────────────────┐
│                              ACTOR SYSTEM                                   │
│                                                                             │
│  ┌─────────────────┐                                                        │
│  │ SupervisorActor │ ◄─── RegisterActor, ActorFailed, SystemShutdown       │
│  │                 │ ───► ActorRestarted (broadcast)                        │
│  │ • Health checks │ ───► GracefulStop (to all)                            │
│  │ • Factory mgmt  │                                                        │
│  └────────┬────────┘                                                        │
│           │                                                                 │
│           │ monitors                                                        │
│           ▼                                                                 │
│  ┌─────────────────┐     ProcessFrame      ┌─────────────────┐              │
│  │   StreamActor   │ ─────────────────────►│ DetectionActor  │              │
│  │                 │ ◄─────────────────────│                 │              │
│  │ • FFmpeg decode │     DetectorReady     │ • AI API calls  │              │
│  │ • Frame extract │                       │ • Confidence    │              │
│  └─────────────────┘                       │   filtering     │              │
│         │                                  └────────┬────────┘              │
│         │                                           │                       │
│         │                                           │ DetectionCompleted    │
│         │                                           ▼                       │
│         │                                  ┌─────────────────┐              │
│         │                                  │   OutputActor   │              │
│         │                                  │                 │              │
│         │                                  │ • Annotate imgs │              │
│         │                                  │ • Rate checks   │              │
│         │                                  │ • Dispatch      │              │
│         │                                  └────────┬────────┘              │
│         │                                           │                       │
│         │                    ┌──────────────────────┼──────────────────┐    │
│         │                    │         ServicePost  │                  │    │
│         │                    ▼                      ▼                  ▼    │
│         │           ┌──────────────┐       ┌──────────────┐    ┌──────────┐│
│         │           │ TwitterActor │       │ BlueskyActor │    │KafkaActor││
│         │           │ (rate limit) │       │              │    │          ││
│         │           └──────────────┘       └──────────────┘    └──────────┘│
│         │                                                                   │
└─────────┼───────────────────────────────────────────────────────────────────┘
          │
          │ spawn_blocking()
          ▼
    ┌───────────────┐         ┌───────────────┐
    │    FFmpeg     │         │  AI Detection │
    │   (decode)    │         │     API       │
    └───────────────┘         └───────────────┘
          │                          │
          ▼                          │
    ┌───────────────┐                │
    │   yt-dlp      │ ◄──────────────┘
    │ (URL resolve) │    HTTP (reqwest)
    └───────────────┘
```

---

## 11. Recommendations Priority

1. **HIGH:** Refactor service actors to eliminate duplication (generic or macro)
2. **HIGH:** Add factories for core actors to enable auto-recovery
3. **MEDIUM:** Replace CONFIG singleton with injected config
4. **MEDIUM:** Fix downcast failure handling in OutputActor
5. **LOW:** Use constants/enums for actor names
6. **LOW:** Optimize image handling to reduce clones

---

*Report generated by automated architecture analysis*
