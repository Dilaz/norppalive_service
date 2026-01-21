# Service Actor Architecture Refactoring Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Eliminate code duplication in service actors and improve reliability of actor restart handling.

**Architecture:** Refactor the four nearly-identical service actors (Twitter, Mastodon, Bluesky, Kafka) into a generic `ServiceActor<S>` pattern. Fix the fragile string-based actor name matching and silent downcast failures in OutputActor. Add actor name constants to prevent typos.

**Tech Stack:** Rust, Actix actors, generics, procedural macro consideration (optional)

---

## Priority Overview

| Task | Priority | Complexity | Files Changed |
|------|----------|------------|---------------|
| 1. Actor Name Constants | HIGH | Low | 3 files |
| 2. Fix Downcast Failure Handling | HIGH | Low | 1 file |
| 3. Generic ServiceActor | HIGH | Medium | 5+ files |
| 4. Core Actor Factories | MEDIUM | High | 4 files |
| 5. Recipient-based Dispatch | LOW | Low | 1 file |

---

## Task 1: Add Actor Name Constants

**Files:**
- Create: `src/actors/names.rs`
- Modify: `src/actors/mod.rs`
- Modify: `src/actors/output.rs:367-403`
- Modify: `src/main.rs:116,125,131,142`

**Step 1: Create the actor names module**

```rust
// src/actors/names.rs
//! Constants for actor names to prevent string duplication and typos.

pub const TWITTER_ACTOR: &str = "TwitterActor";
pub const BLUESKY_ACTOR: &str = "BlueskyActor";
pub const MASTODON_ACTOR: &str = "MastodonActor";
pub const KAFKA_ACTOR: &str = "KafkaActor";
pub const STREAM_ACTOR: &str = "StreamActor";
pub const DETECTION_ACTOR: &str = "DetectionActor";
pub const OUTPUT_ACTOR: &str = "OutputActor";
pub const SUPERVISOR_ACTOR: &str = "SupervisorActor";
```

**Step 2: Export from actors/mod.rs**

Add to `src/actors/mod.rs`:
```rust
pub mod names;
```

**Step 3: Update output.rs Handler<ActorRestarted>**

Replace string literals in `src/actors/output.rs` around lines 367-403:

```rust
use crate::actors::names::{TWITTER_ACTOR, BLUESKY_ACTOR, MASTODON_ACTOR, KAFKA_ACTOR};

// In Handler<ActorRestarted>::handle():
match msg.actor_name.as_str() {
    TWITTER_ACTOR => {
        // ... existing downcast logic
    }
    BLUESKY_ACTOR => {
        // ... existing downcast logic
    }
    MASTODON_ACTOR => {
        // ... existing downcast logic
    }
    KAFKA_ACTOR => {
        // ... existing downcast logic
    }
    _ => {
        debug!(
            "OutputActor ignoring restart notification for unknown actor: {}",
            msg.actor_name
        );
    }
}
```

**Step 4: Update main.rs registrations**

Replace string literals in `src/main.rs`:

```rust
use crate::actors::names::{
    TWITTER_ACTOR, BLUESKY_ACTOR, MASTODON_ACTOR, KAFKA_ACTOR,
    STREAM_ACTOR, DETECTION_ACTOR, OUTPUT_ACTOR,
};

// Line ~116:
supervisor.do_send(RegisterActor::with_factory(TWITTER_ACTOR, twitter_factory));

// Line ~125:
supervisor.do_send(RegisterActor::with_factory(BLUESKY_ACTOR, bluesky_factory));

// Line ~131:
supervisor.do_send(RegisterActor::with_factory(MASTODON_ACTOR, mastodon_factory));

// Line ~142:
supervisor.do_send(RegisterActor::with_factory(KAFKA_ACTOR, kafka_factory));

// Lines ~145-148:
supervisor.do_send(RegisterActor::new(STREAM_ACTOR));
supervisor.do_send(RegisterActor::new(DETECTION_ACTOR));
supervisor.do_send(RegisterActor::new(OUTPUT_ACTOR));
```

**Step 5: Run tests**

```bash
cargo test
```

Expected: All tests pass.

**Step 6: Commit**

```bash
git add src/actors/names.rs src/actors/mod.rs src/actors/output.rs src/main.rs
git commit -m "refactor: add actor name constants to prevent typos"
```

---

## Task 2: Fix Downcast Failure Handling

**Files:**
- Modify: `src/actors/output.rs:366-417`

**Step 1: Improve downcast error handling**

Replace the current silent failure pattern. In `src/actors/output.rs`, update the `Handler<ActorRestarted>` implementation:

```rust
impl Handler<ActorRestarted> for OutputActor {
    type Result = ();

    fn handle(&mut self, msg: ActorRestarted, _ctx: &mut Self::Context) -> Self::Result {
        info!(
            "OutputActor received restart notification for actor: {}",
            msg.actor_name
        );

        match msg.actor_name.as_str() {
            TWITTER_ACTOR => {
                match msg.new_address.downcast_ref::<Addr<TwitterActor>>().cloned() {
                    Some(addr) => {
                        info!("Updated TwitterActor address after restart");
                        self.twitter_actor = Some(addr);
                    }
                    None => {
                        error!(
                            "Type mismatch: Failed to downcast {} address. \
                             Service will be disabled until next restart.",
                            TWITTER_ACTOR
                        );
                        self.twitter_actor = None;
                    }
                }
            }
            BLUESKY_ACTOR => {
                match msg.new_address.downcast_ref::<Addr<BlueskyActor>>().cloned() {
                    Some(addr) => {
                        info!("Updated BlueskyActor address after restart");
                        self.bluesky_actor = Some(addr);
                    }
                    None => {
                        error!(
                            "Type mismatch: Failed to downcast {} address. \
                             Service will be disabled until next restart.",
                            BLUESKY_ACTOR
                        );
                        self.bluesky_actor = None;
                    }
                }
            }
            MASTODON_ACTOR => {
                match msg.new_address.downcast_ref::<Addr<MastodonActor>>().cloned() {
                    Some(addr) => {
                        info!("Updated MastodonActor address after restart");
                        self.mastodon_actor = Some(addr);
                    }
                    None => {
                        error!(
                            "Type mismatch: Failed to downcast {} address. \
                             Service will be disabled until next restart.",
                            MASTODON_ACTOR
                        );
                        self.mastodon_actor = None;
                    }
                }
            }
            KAFKA_ACTOR => {
                match msg.new_address.downcast_ref::<Addr<KafkaActor>>().cloned() {
                    Some(addr) => {
                        info!("Updated KafkaActor address after restart");
                        self.kafka_actor = Some(addr);
                    }
                    None => {
                        error!(
                            "Type mismatch: Failed to downcast {} address. \
                             Service will be disabled until next restart.",
                            KAFKA_ACTOR
                        );
                        self.kafka_actor = None;
                    }
                }
            }
            _ => {
                debug!(
                    "OutputActor ignoring restart notification for actor: {}",
                    msg.actor_name
                );
            }
        }
    }
}
```

**Step 2: Run tests**

```bash
cargo test output
```

Expected: All output tests pass.

**Step 3: Commit**

```bash
git add src/actors/output.rs
git commit -m "fix: set service to None on downcast failure instead of keeping stale reference"
```

---

## Task 3: Generic ServiceActor Implementation

**Files:**
- Create: `src/actors/services/generic_actor.rs`
- Modify: `src/actors/services/mod.rs`
- Modify: `src/main.rs`
- Eventually remove or simplify: `src/actors/services/twitter.rs`, `bluesky.rs`, `mastodon.rs`, `kafka.rs`

### Step 3.1: Create the generic service actor

```rust
// src/actors/services/generic_actor.rs
//! Generic service actor that eliminates duplication across service implementations.

use actix::prelude::*;
use log::{error, info};

use crate::error::NorppaliveError;
use crate::messages::output::{GetServiceStatus, ServicePost, ServicePostResult, ServiceStatus};
use crate::services::generic::SocialMediaService;

/// Generic actor for any service implementing SocialMediaService.
///
/// This eliminates the duplication between TwitterActor, BlueskyActor,
/// MastodonActor, and KafkaActor which all had ~90% identical code.
pub struct ServiceActor<S>
where
    S: SocialMediaService + Clone + Unpin + 'static,
{
    service: S,
    service_status: ServiceStatus,
}

impl<S> ServiceActor<S>
where
    S: SocialMediaService + Clone + Unpin + 'static,
{
    pub fn new(service: S) -> Self {
        let service_name = service.name().to_string();
        Self {
            service,
            service_status: ServiceStatus {
                name: service_name,
                healthy: true,
                last_post_time: None,
                error_count: 0,
                rate_limited: false,
            },
        }
    }
}

impl<S> Actor for ServiceActor<S>
where
    S: SocialMediaService + Clone + Unpin + 'static,
{
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("ServiceActor ({}) started", self.service.name());
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("ServiceActor ({}) stopped", self.service.name());
    }
}

impl<S> Handler<ServicePost> for ServiceActor<S>
where
    S: SocialMediaService + Clone + Unpin + 'static,
{
    type Result = ResponseActFuture<Self, ServicePostResult>;

    fn handle(&mut self, msg: ServicePost, _ctx: &mut Self::Context) -> Self::Result {
        let service_name = self.service.name().to_string();
        info!(
            "ServiceActor ({}) received post request: {}",
            service_name,
            msg.message.chars().take(50).collect::<String>()
        );

        let message = msg.message.clone();
        let image_path = msg.image_path.clone();
        let service_instance = self.service.clone();

        Box::pin(
            async move {
                match service_instance.post(&message, &image_path).await {
                    Ok(_) => {
                        info!("Successfully posted to {}", service_instance.name());
                        Ok(())
                    }
                    Err(err) => {
                        error!("Failed to post to {}: {}", service_instance.name(), err);
                        Err(err)
                    }
                }
            }
            .into_actor(self)
            .map(|result, actor, _ctx| {
                let service_name = actor.service.name().to_string();
                match &result {
                    Ok(_) => {
                        actor.service_status.last_post_time =
                            Some(chrono::Utc::now().to_rfc3339());
                        actor.service_status.error_count = 0;
                        actor.service_status.healthy = true;
                    }
                    Err(_) => {
                        actor.service_status.error_count += 1;
                        if actor.service_status.error_count >= 3 {
                            actor.service_status.healthy = false;
                        }
                    }
                }
                ServicePostResult {
                    service_name,
                    success: result.is_ok(),
                    error: result.err().map(|e| e.to_string()),
                }
            }),
        )
    }
}

impl<S> Handler<GetServiceStatus> for ServiceActor<S>
where
    S: SocialMediaService + Clone + Unpin + 'static,
{
    type Result = Result<ServiceStatus, NorppaliveError>;

    fn handle(&mut self, _msg: GetServiceStatus, _ctx: &mut Self::Context) -> Self::Result {
        Ok(self.service_status.clone())
    }
}
```

### Step 3.2: Export from services/mod.rs

Add to `src/actors/services/mod.rs`:

```rust
pub mod generic_actor;
pub use generic_actor::ServiceActor;
```

### Step 3.3: Create type aliases for existing actors

To maintain backwards compatibility during migration, add type aliases:

```rust
// In src/actors/services/mod.rs or generic_actor.rs
use crate::services::{BlueskyService, KafkaService, MastodonService, TwitterService};

// Type aliases for backwards compatibility
pub type TwitterServiceActor = ServiceActor<TwitterService>;
pub type BlueskyServiceActor = ServiceActor<BlueskyService>;
pub type MastodonServiceActor = ServiceActor<MastodonService>;
pub type KafkaServiceActor = ServiceActor<KafkaService>;
```

### Step 3.4: Update main.rs to use generic actors

Update factory registrations in `src/main.rs`:

```rust
use crate::actors::services::ServiceActor;
use crate::services::{TwitterService, BlueskyService, MastodonService, KafkaService};

// Twitter factory (replacing TwitterActor)
if twitter_is_some() {
    let twitter_factory: ActorFactoryFn = Arc::new(|| {
        Arc::new(ServiceActor::new(TwitterService::default()).start())
    });
    supervisor.do_send(RegisterActor::with_factory(TWITTER_ACTOR, twitter_factory));
}

// Bluesky factory
if bluesky_is_some() {
    let bluesky_factory: ActorFactoryFn = Arc::new(|| {
        Arc::new(ServiceActor::new(BlueskyService::default()).start())
    });
    supervisor.do_send(RegisterActor::with_factory(BLUESKY_ACTOR, bluesky_factory));
}

// Mastodon factory
if mastodon_is_some() {
    let mastodon_factory: ActorFactoryFn = Arc::new(|| {
        Arc::new(ServiceActor::new(MastodonService::default()).start())
    });
    supervisor.do_send(RegisterActor::with_factory(MASTODON_ACTOR, mastodon_factory));
}

// Kafka factory
if kafka_is_some() {
    let kafka_factory: ActorFactoryFn = Arc::new(|| {
        Arc::new(ServiceActor::new(KafkaService::default()).start())
    });
    supervisor.do_send(RegisterActor::with_factory(KAFKA_ACTOR, kafka_factory));
}
```

### Step 3.5: Update OutputActor to use generic actor types

In `src/actors/output.rs`, update the struct fields and downcast types:

```rust
use crate::actors::services::{ServiceActor, TwitterServiceActor, BlueskyServiceActor, MastodonServiceActor, KafkaServiceActor};
use crate::services::{TwitterService, BlueskyService, MastodonService, KafkaService};

pub struct OutputActor {
    // ... other fields ...
    pub twitter_actor: Option<Addr<ServiceActor<TwitterService>>>,
    pub bluesky_actor: Option<Addr<ServiceActor<BlueskyService>>>,
    pub mastodon_actor: Option<Addr<ServiceActor<MastodonService>>>,
    pub kafka_actor: Option<Addr<ServiceActor<KafkaService>>>,
}

// Update Handler<ActorRestarted> downcasts accordingly:
TWITTER_ACTOR => {
    match msg.new_address.downcast_ref::<Addr<ServiceActor<TwitterService>>>().cloned() {
        // ...
    }
}
```

### Step 3.6: Run tests

```bash
cargo test
```

Expected: All tests pass.

### Step 3.7: Commit

```bash
git add src/actors/services/generic_actor.rs src/actors/services/mod.rs src/main.rs src/actors/output.rs
git commit -m "refactor: implement generic ServiceActor to eliminate service actor duplication"
```

### Step 3.8: Remove old actor files (optional cleanup)

After verifying everything works, the old actor files can be removed:

```bash
rm src/actors/services/twitter.rs
rm src/actors/services/bluesky.rs
rm src/actors/services/mastodon.rs
rm src/actors/services/kafka.rs
```

Update `src/actors/services/mod.rs` to remove the old exports.

**Step 3.9: Final test and commit**

```bash
cargo test
git add -A
git commit -m "refactor: remove deprecated service actor implementations"
```

---

## Task 4: Add Factories for Core Actors (Optional - Higher Risk)

> **Note:** This task is more complex and carries higher risk. Consider whether the added complexity is worth the auto-recovery benefit.

**Files:**
- Modify: `src/actors/stream.rs`
- Modify: `src/actors/detection.rs`
- Modify: `src/actors/output.rs`
- Modify: `src/main.rs`

### Step 4.1: Add ActorRestarted handler to StreamActor

In `src/actors/stream.rs`:

```rust
use crate::messages::supervisor::{ActorRestarted, SubscribeToRestarts};
use crate::actors::names::DETECTION_ACTOR;

impl Handler<ActorRestarted> for StreamActor {
    type Result = ();

    fn handle(&mut self, msg: ActorRestarted, _ctx: &mut Self::Context) -> Self::Result {
        if msg.actor_name == DETECTION_ACTOR {
            match msg.new_address.downcast_ref::<Addr<DetectionActor>>().cloned() {
                Some(addr) => {
                    info!("StreamActor: Updated DetectionActor reference after restart");
                    self.detection_actor = Some(addr);
                }
                None => {
                    error!("StreamActor: Failed to downcast DetectionActor address");
                    self.detection_actor = None;
                }
            }
        }
    }
}

// In Actor::started(), subscribe to restarts:
fn started(&mut self, ctx: &mut Self::Context) {
    info!("StreamActor started");

    if let Some(ref supervisor) = self.supervisor_actor {
        supervisor.do_send(SubscribeToRestarts {
            subscriber: ctx.address().recipient(),
        });
        info!("StreamActor subscribed to restart notifications");
    }
}
```

### Step 4.2: Create factory-compatible constructors

Modify core actors to support factory-based creation with dependency injection:

```rust
// In main.rs, create factories that capture dependencies
let output_addr = output_actor.clone();
let detection_addr = detection_actor.clone();
let supervisor_addr = supervisor.clone();

let stream_factory: ActorFactoryFn = {
    let detection = detection_addr.clone();
    let supervisor = supervisor_addr.clone();
    Arc::new(move || {
        Arc::new(StreamActor::with_actors(detection.clone(), supervisor.clone()).start())
    })
};

supervisor.do_send(RegisterActor::with_factory(STREAM_ACTOR, stream_factory));
```

### Step 4.3: Run tests

```bash
cargo test stream
cargo test detection
cargo test supervisor
```

### Step 4.4: Commit

```bash
git add src/actors/stream.rs src/actors/detection.rs src/actors/output.rs src/main.rs
git commit -m "feat: add factory support for core actors enabling auto-recovery"
```

---

## Task 5: Recipient-Based Service Dispatch (Optional Enhancement)

**Files:**
- Modify: `src/actors/output.rs`

### Step 5.1: Replace individual actor fields with Recipient vector

```rust
use actix::Recipient;

pub struct OutputActor {
    // ... other fields ...

    /// Service recipients for posting (replaces individual actor fields)
    service_recipients: Vec<(String, Recipient<ServicePost>)>,
}

impl OutputActor {
    pub fn with_recipients(
        output_service: Box<dyn OutputServiceTrait>,
        recipients: Vec<(String, Recipient<ServicePost>)>,
        supervisor_actor: Option<Addr<SupervisorActor>>,
    ) -> Self {
        Self {
            output_service,
            service_recipients: recipients,
            supervisor_actor,
            // ... other fields ...
        }
    }
}
```

### Step 5.2: Update dispatch logic

```rust
// Replace individual if-let blocks with loop
for (name, recipient) in &self.service_recipients {
    if let Err(e) = recipient.try_send(service_post.clone()) {
        warn!("Failed to send post to {}: {}", name, e);
    }
}
```

### Step 5.3: Run tests

```bash
cargo test output
```

### Step 5.4: Commit

```bash
git add src/actors/output.rs
git commit -m "refactor: use Recipient vector for service dispatch"
```

---

## Verification Checklist

After completing all tasks:

- [ ] `cargo build` succeeds
- [ ] `cargo test` passes all tests
- [ ] `cargo clippy` shows no new warnings
- [ ] Service actors start correctly with generic implementation
- [ ] Actor restart notifications are handled correctly
- [ ] Downcast failures log errors and disable services
- [ ] No string literals for actor names remain in code

---

## Rollback Plan

If issues arise during migration:

1. Keep old actor implementations until generic is proven
2. Type aliases allow gradual migration
3. Git revert to last working commit if needed
4. Run full test suite after each commit to catch regressions early

---

*Plan created: 2026-01-21*
