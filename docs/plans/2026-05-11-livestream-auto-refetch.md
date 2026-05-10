# Livestream Auto-Refetch Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** When the configured stream is a livestream, `StreamActor` re-resolves a fresh URL via yt-dlp and reconnects when the current FFmpeg session ends. For non-livestreams, today's "shut down on EOF" behavior is preserved.

**Architecture:** Extend the existing `get_stream_url` call to also report `is_live`. Cache that on `StreamActor`. On `InternalProcessingComplete`, branch: if live → schedule a refetch with exponential backoff via `ctx.run_later`; if not → fall through to today's `SystemShutdown` path. Cap consecutive refetch failures at 10 to avoid hammering yt-dlp forever.

**Tech Stack:** Rust, Actix actors, yt-dlp subprocess (already a dependency), FFmpeg via `ffmpeg-next` (already integrated).

**Design doc:** `docs/plans/2026-05-11-livestream-auto-refetch-design.md`

**Files touched:**
- `src/actors/stream.rs` — primary changes
- `src/messages/stream.rs` — new `RefetchAndRestart` message
- `tests/actors/stream_tests.rs` — new tests

---

## Priority Overview

| Task | Priority | Complexity | Files |
|---|---|---|---|
| 1. Backoff schedule (pure fn) | HIGH | Low | 1 |
| 2. yt-dlp output parser (pure fn) | HIGH | Low | 1 |
| 3. End-of-session decision (pure fn) | HIGH | Low | 1 |
| 4. Plumb `ResolvedStream` through resolver | HIGH | Medium | 1 |
| 5. Capture `is_livestream` + counter on StreamActor | HIGH | Low | 1 |
| 6. Wire refetch state machine | HIGH | Medium | 2 |
| 7. Final verification (cargo test + clippy) | HIGH | Low | 0 |

---

## Task 1: Backoff schedule (pure function)

**Files:**
- Modify: `src/actors/stream.rs` (add new module at end of file, before tests would be)

**Step 1: Write failing test**

Add to `src/actors/stream.rs` inside a new `#[cfg(test)] mod tests { ... }` block at the bottom of the file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_schedule_matches_design() {
        assert_eq!(backoff_for(1), Duration::from_secs(5));
        assert_eq!(backoff_for(2), Duration::from_secs(15));
        assert_eq!(backoff_for(3), Duration::from_secs(30));
        assert_eq!(backoff_for(4), Duration::from_secs(60));
        assert_eq!(backoff_for(5), Duration::from_secs(120));
        assert_eq!(backoff_for(6), Duration::from_secs(300));
        assert_eq!(backoff_for(7), Duration::from_secs(300));
        assert_eq!(backoff_for(50), Duration::from_secs(300));
    }

    #[test]
    fn backoff_attempt_zero_is_five_seconds() {
        // Safety net: even if caller passes 0, we don't return a zero-duration.
        assert_eq!(backoff_for(0), Duration::from_secs(5));
    }
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test --lib backoff_schedule 2>&1 | tail -10
```

Expected: compile error `cannot find function backoff_for in this scope`.

**Step 3: Add minimal implementation**

Near the top of `src/actors/stream.rs`, just below the existing `const MAX_STREAM_RUNTIME` block:

```rust
const MAX_CONSECUTIVE_REFETCH_FAILURES: u32 = 10;

/// Backoff delay for a given consecutive-failure attempt number (1-indexed).
/// Capped at 5 minutes for attempt 6 and above.
fn backoff_for(attempt: u32) -> Duration {
    match attempt {
        0 | 1 => Duration::from_secs(5),
        2 => Duration::from_secs(15),
        3 => Duration::from_secs(30),
        4 => Duration::from_secs(60),
        5 => Duration::from_secs(120),
        _ => Duration::from_secs(300),
    }
}
```

**Step 4: Run test, expect pass**

```bash
cargo test --lib backoff_schedule
```

Expected: 2 passed.

**Step 5: Commit**

```bash
git add src/actors/stream.rs
git commit -m "feat(stream): add backoff schedule for livestream refetch"
```

---

## Task 2: yt-dlp output parser (pure function)

**Files:**
- Modify: `src/actors/stream.rs`

**Step 1: Write failing tests**

Add to the `mod tests` block from Task 1:

```rust
#[test]
fn parse_yt_dlp_two_lines_true() {
    let out = "https://cdn.example.com/segment\nTrue\n";
    let r = parse_yt_dlp_resolve_output(out).expect("parse ok");
    assert_eq!(r.url, "https://cdn.example.com/segment");
    assert!(r.is_live);
}

#[test]
fn parse_yt_dlp_two_lines_false() {
    let out = "https://cdn.example.com/segment\nFalse\n";
    let r = parse_yt_dlp_resolve_output(out).expect("parse ok");
    assert_eq!(r.url, "https://cdn.example.com/segment");
    assert!(!r.is_live);
}

#[test]
fn parse_yt_dlp_na_is_not_live() {
    let out = "https://cdn.example.com/file.mp4\nNA\n";
    let r = parse_yt_dlp_resolve_output(out).expect("parse ok");
    assert!(!r.is_live);
}

#[test]
fn parse_yt_dlp_empty_is_error() {
    assert!(parse_yt_dlp_resolve_output("").is_err());
    assert!(parse_yt_dlp_resolve_output("\n").is_err());
}

#[test]
fn parse_yt_dlp_single_line_treated_as_url_not_live() {
    // Defensive: if yt-dlp omits the is_live print for some reason,
    // we still get a URL but assume not-live (safer default).
    let r = parse_yt_dlp_resolve_output("https://x/y\n").expect("parse ok");
    assert_eq!(r.url, "https://x/y");
    assert!(!r.is_live);
}

#[test]
fn parse_yt_dlp_strips_whitespace() {
    let r = parse_yt_dlp_resolve_output("  https://x/y  \n  True  \n").expect("parse ok");
    assert_eq!(r.url, "https://x/y");
    assert!(r.is_live);
}
```

**Step 2: Run, verify fail**

```bash
cargo test --lib parse_yt_dlp 2>&1 | tail -10
```

Expected: compile error — `ResolvedStream` and `parse_yt_dlp_resolve_output` not found.

**Step 3: Add types + parser**

Near the top of `src/actors/stream.rs` (just under the constants from Task 1):

```rust
/// Result of resolving an input URL via yt-dlp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedStream {
    pub url: String,
    pub is_live: bool,
}

/// Parse the stdout of `yt-dlp --print url --print is_live <input>`.
///
/// Expected layout: two lines, URL on the first, `True`/`False`/`NA` on the second.
/// A single line is accepted as URL with `is_live = false` (defensive default).
/// Empty input is an error.
fn parse_yt_dlp_resolve_output(stdout: &str) -> Result<ResolvedStream, NorppaliveError> {
    let mut lines = stdout.lines().map(str::trim).filter(|s| !s.is_empty());
    let url = lines
        .next()
        .ok_or_else(|| {
            NorppaliveError::StreamUrlError("yt-dlp returned no URL line".to_string())
        })?
        .to_string();
    let is_live = matches!(lines.next(), Some("True"));
    Ok(ResolvedStream { url, is_live })
}
```

**Step 4: Run, expect pass**

```bash
cargo test --lib parse_yt_dlp
```

Expected: 6 passed.

**Step 5: Commit**

```bash
git add src/actors/stream.rs
git commit -m "feat(stream): parse url+is_live from yt-dlp output"
```

---

## Task 3: End-of-session decision (pure function)

**Files:**
- Modify: `src/actors/stream.rs`

**Step 1: Write failing tests**

Add to `mod tests`:

```rust
#[test]
fn decide_live_ok_means_refetch() {
    assert_eq!(decide_after_session_end(true, true), EndAction::Refetch);
}

#[test]
fn decide_live_err_means_refetch() {
    assert_eq!(decide_after_session_end(true, false), EndAction::Refetch);
}

#[test]
fn decide_non_live_means_shutdown() {
    assert_eq!(decide_after_session_end(false, true), EndAction::Shutdown);
    assert_eq!(decide_after_session_end(false, false), EndAction::Shutdown);
}
```

**Step 2: Run, verify fail**

```bash
cargo test --lib decide_ 2>&1 | tail -10
```

Expected: compile error — `EndAction` not found.

**Step 3: Implement**

Just under the parser in `src/actors/stream.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EndAction {
    Refetch,
    Shutdown,
}

/// What to do when a stream session ends.
/// `is_live` is the current liveness of the source.
/// `was_clean` is true if the session ended without an FFmpeg/decoder error.
fn decide_after_session_end(is_live: bool, _was_clean: bool) -> EndAction {
    if is_live {
        EndAction::Refetch
    } else {
        EndAction::Shutdown
    }
}
```

Rationale for keeping `_was_clean`: it is part of the documented contract from the design doc and we may want to log differently in the handler. The decision currently only depends on `is_live`.

**Step 4: Run, expect pass**

```bash
cargo test --lib decide_
```

Expected: 3 passed.

**Step 5: Commit**

```bash
git add src/actors/stream.rs
git commit -m "feat(stream): add end-of-session decision helper"
```

---

## Task 4: Plumb `ResolvedStream` through the resolver

**Files:**
- Modify: `src/actors/stream.rs` — `get_stream_url`, `init_ffmpeg_stream`

This task changes the **signature** of `get_stream_url` but keeps behavior the same: callers don't yet use `is_live`. After this commit, the actor still always shuts down on EOF.

**Step 1: Update `get_stream_url`**

Replace the existing function body in `src/actors/stream.rs` (currently around line 152–224). The new version asks yt-dlp for both fields with `--print` and parses the result:

```rust
fn get_stream_url(stream_url: &str) -> Result<ResolvedStream, NorppaliveError> {
    if !stream_url.starts_with("http") {
        return Ok(ResolvedStream {
            url: stream_url.to_string(),
            is_live: false,
        });
    }

    const RUNTIME_COOKIES: &str = "/tmp/yt-cookies-runtime.txt";
    let mut args: Vec<&str> = vec!["-f", "best[height<=720]", "--js-runtimes", "deno"];
    if let Some(src) = CONFIG.stream.cookies_path.as_deref() {
        if std::path::Path::new(src).exists() {
            match std::fs::copy(src, RUNTIME_COOKIES).and_then(|_| {
                std::fs::set_permissions(
                    RUNTIME_COOKIES,
                    <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o600),
                )
            }) {
                Ok(_) => args.extend(["--cookies", RUNTIME_COOKIES]),
                Err(e) => error!(
                    target: "stream",
                    "failed to stage cookies from {} to {}: {}; running yt-dlp without --cookies",
                    src, RUNTIME_COOKIES, e
                ),
            }
        } else {
            info!(
                target: "stream",
                "stream.cookies_path is set ({}) but the file does not exist; running yt-dlp without --cookies",
                src
            );
        }
    }
    // --print replaces -g; two prints gives us two stdout lines.
    args.extend(["--print", "url", "--print", "is_live", stream_url]);

    let output_result = Command::new("yt-dlp").args(&args).output();

    match output_result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            match parse_yt_dlp_resolve_output(&stdout) {
                Ok(resolved) => {
                    info!(
                        target: "stream",
                        "yt-dlp resolved {} (is_live={})",
                        stream_url, resolved.is_live
                    );
                    Ok(resolved)
                }
                Err(_) => {
                    error!(
                        target: "stream",
                        "yt-dlp returned no URL for {}. stderr: {}",
                        stream_url,
                        stderr
                    );
                    Err(NorppaliveError::StreamUrlError(format!(
                        "yt-dlp failed for {}: {}",
                        stream_url,
                        stderr.trim()
                    )))
                }
            }
        }
        Err(e) => {
            error!(
                target: "stream",
                "Failed to execute yt-dlp command for {}: {}",
                stream_url,
                e
            );
            Err(NorppaliveError::StreamUrlError(format!(
                "Failed to execute yt-dlp for {}: {}",
                stream_url, e
            )))
        }
    }
}
```

**Step 2: Update `init_ffmpeg_stream` to consume the new shape**

In the same file, find the block around line 263–275 that calls `Self::get_stream_url`:

Old:
```rust
let actual_url = if stream_url.starts_with("https://") {
    match Self::get_stream_url(&stream_url) {
        Ok(url) => url,
        Err(e) => { ... }
    }
} else {
    stream_url
};
```

Replace with:
```rust
let resolved = if stream_url.starts_with("https://") {
    match Self::get_stream_url(&stream_url) {
        Ok(r) => r,
        Err(e) => {
            error!(target: "stream", "Failed to get stream URL: {}", e);
            Self::send_init_status(init_sender.take(), &Err(e.clone_for_error_reporting()));
            return Err(e);
        }
    }
} else {
    ResolvedStream { url: stream_url, is_live: false }
};
let actual_url = resolved.url;
// `resolved.is_live` is not used yet; Task 5 wires it to the actor state.
```

Note: drop the now-unreachable inner `match` for `Err` that was in the old block — it's folded into the new match above.

**Step 3: Build + run existing tests**

```bash
cargo build 2>&1 | tail -20
cargo test
```

Expected: builds clean, all existing tests still pass (no behavior change; only signature). 59 tests should pass as before.

**Step 4: Commit**

```bash
git add src/actors/stream.rs
git commit -m "refactor(stream): yt-dlp resolver returns ResolvedStream{url,is_live}"
```

---

## Task 5: Capture `is_livestream` + failure counter on StreamActor

**Files:**
- Modify: `src/actors/stream.rs`

This task adds the new fields and starts populating `is_livestream` from `init_ffmpeg_stream`, but still does not change end-of-session behavior. It's a no-op functionally; the next task wires the state machine.

**Step 1: Add fields to `StreamActor` struct**

In `src/actors/stream.rs` at the struct definition (around line 36–47):

```rust
#[derive(Default)]
pub struct StreamActor {
    stream_url: Option<String>,
    running: bool,
    detection_actor: Option<Addr<crate::actors::DetectionActor>>,
    supervisor_actor: Option<Addr<crate::actors::SupervisorActor>>,
    latest_frame_buffer: Option<LatestFrameAvailable>,
    detector_ready: bool,
    shutdown_signal: Option<Arc<AtomicBool>>,
    is_processing_task_running: bool,
    processing_task_handle: Option<JoinHandle<()>>,
    is_livestream: bool,                 // NEW: set from yt-dlp on each resolve
    consecutive_refetch_failures: u32,   // NEW: increments per failed refetch; reset on success
}
```

**Step 2: Plumb `is_live` from blocking thread into the actor**

The challenge: `init_ffmpeg_stream` runs inside `spawn_blocking` (no `&mut StreamActor` access). We need to surface `is_live` back to the actor.

Approach: change the oneshot init signal to carry `is_live` on success. Currently:

```rust
init_signal: oneshot::Sender<Result<(), NorppaliveError>>
```

Change to:

```rust
init_signal: oneshot::Sender<Result<bool, NorppaliveError>>
//                                        ^ true if is_live
```

Touch points (search the file for `oneshot::Sender<Result<`):
- `start_stream_processing` signature
- `process_stream_blocking` signature
- `init_ffmpeg_stream` signature + the two `send_init_status` call sites (success case sends `Ok(resolved.is_live)`; failure unchanged)
- `send_init_status` helper signature
- `StartStream` handler: receive `bool` on success and store via a self-message

**send_init_status** (around line 226–240) becomes:

```rust
fn send_init_status(
    sender: Option<oneshot::Sender<Result<bool, NorppaliveError>>>,
    result: &Result<bool, NorppaliveError>,
) {
    if let Some(s) = sender {
        let status = match result {
            Ok(b) => Ok(*b),
            Err(err) => Err(err.clone_for_error_reporting()),
        };
        if let Err(e) = s.send(status) {
            error!(target: "stream", "Failed to send init status: {:?}. Receiver likely dropped.", e);
        }
    }
}
```

**init_ffmpeg_stream** success path now passes `Ok(resolved.is_live)`:

In the `match input(&actual_url)` block, change:
```rust
Self::send_init_status(init_sender.take(), &Ok(()));
```
to:
```rust
Self::send_init_status(init_sender.take(), &Ok(resolved.is_live));
```

And replace the surrounding `let resolved = ...; let actual_url = resolved.url;` with `let resolved = ...;` and then use `resolved.url` and `resolved.is_live` in place. (Move the `let actual_url = ...;` shadow away — we need `resolved` alive at the success-send.)

Concretely the new init_ffmpeg_stream tail looks like:

```rust
info!(target: "stream", "Attempting to open FFmpeg input for URL: {}", &resolved.url);
let ictx = match input(&resolved.url) {
    Ok(ctx) => {
        info!(target: "stream", "Successfully opened FFmpeg input for: {}", &resolved.url);
        Self::send_init_status(init_sender.take(), &Ok(resolved.is_live));
        ctx
    }
    Err(e) => {
        error!(target: "stream", "Failed to open FFmpeg input for {}: {}", &resolved.url, e);
        let err = NorppaliveError::from(e);
        Self::send_init_status(init_sender.take(), &Err(err.clone_for_error_reporting()));
        return Err(err);
    }
};
// ... continue using resolved.url below for stream lookup error message
```

**StartStream handler** receives the bool, sends a new internal message to record state:

Add a new internal message just below `StreamInitializationFailed`:

```rust
#[derive(Message)]
#[rtype(result = "()")]
struct SessionStarted { pub is_live: bool }

impl Handler<SessionStarted> for StreamActor {
    type Result = ();
    fn handle(&mut self, msg: SessionStarted, _ctx: &mut Context<Self>) {
        self.is_livestream = msg.is_live;
        self.consecutive_refetch_failures = 0;
        info!(target: "stream", "Session started: is_livestream={}", msg.is_live);
    }
}
```

In `StartStream` handler's async block, on `Ok(Ok(b))` send `SessionStarted { is_live: b }` to `actor_address`:

```rust
Box::pin(async move {
    match rx.await {
        Ok(Ok(is_live)) => {
            info!("Stream initialization reported success (is_live={}).", is_live);
            actor_address.do_send(SessionStarted { is_live });
            Ok(())
        }
        Ok(Err(e)) => { ... unchanged ... }
        Err(_channel_error) => { ... unchanged ... }
    }
})
```

**Step 3: Build + run tests**

```bash
cargo build 2>&1 | tail -20
cargo test
```

Expected: builds clean, all 59 existing tests still pass. Behavior unchanged; we just track is_livestream.

**Step 4: Commit**

```bash
git add src/actors/stream.rs
git commit -m "feat(stream): record is_livestream and refetch failure counter"
```

---

## Task 6: Wire the refetch state machine

**Files:**
- Modify: `src/messages/stream.rs` — add `RefetchAndRestart`
- Modify: `src/actors/stream.rs` — handler + scheduling

**Step 1: Add the message**

Append to `src/messages/stream.rs`:

```rust
/// Internal: scheduled by StreamActor after a livestream session ends; triggers a yt-dlp
/// re-resolve and a new FFmpeg session against the same configured URL.
#[derive(Message)]
#[rtype(result = "()")]
pub struct RefetchAndRestart;
```

**Step 2: Re-export at the messages root**

Check `src/messages/mod.rs` for the pattern used by the other stream messages, and add `RefetchAndRestart` to the same re-export list (look for `pub use stream::{...}` or `pub use self::stream::*;`). Mirror exactly what's there.

**Step 3: Update `InternalProcessingComplete` handler**

In `src/actors/stream.rs`, replace the handler body (around line 802–826):

```rust
impl Handler<InternalProcessingComplete> for StreamActor {
    type Result = ();

    fn handle(&mut self, msg: InternalProcessingComplete, ctx: &mut Context<Self>) {
        info!(target: "stream", "Internal FFmpeg processing task reported completion.");
        self.is_processing_task_running = false;
        self.running = false;
        self.processing_task_handle = None;

        let was_clean = msg.result.is_ok();
        if let Err(ref e) = msg.result {
            error!(target: "stream", "Stream processing task failed: {}", e);
        }

        match decide_after_session_end(self.is_livestream, was_clean) {
            EndAction::Refetch => {
                self.consecutive_refetch_failures =
                    self.consecutive_refetch_failures.saturating_add(1);
                if self.consecutive_refetch_failures > MAX_CONSECUTIVE_REFETCH_FAILURES {
                    error!(
                        target: "stream",
                        "Reached {} consecutive refetch failures; giving up and shutting down.",
                        MAX_CONSECUTIVE_REFETCH_FAILURES
                    );
                    if let Some(sup) = &self.supervisor_actor {
                        sup.do_send(SystemShutdown);
                    }
                    return;
                }
                let delay = backoff_for(self.consecutive_refetch_failures);
                info!(
                    target: "stream",
                    "Livestream session ended (clean={}); refetching in {:?} (attempt {}/{})",
                    was_clean,
                    delay,
                    self.consecutive_refetch_failures,
                    MAX_CONSECUTIVE_REFETCH_FAILURES
                );
                ctx.run_later(delay, |_a, c| {
                    c.address().do_send(RefetchAndRestart);
                });
            }
            EndAction::Shutdown => {
                if was_clean {
                    info!(
                        target: "stream",
                        "Stream source exhausted (not a livestream). Checking if all frames are processed before shutdown."
                    );
                    self.check_and_initiate_shutdown_if_all_done(ctx);
                } else {
                    info!(target: "stream", "Requesting system shutdown after non-livestream error.");
                    if let Some(sup) = &self.supervisor_actor {
                        sup.do_send(SystemShutdown);
                    }
                }
            }
        }
    }
}
```

**Step 4: Add the `RefetchAndRestart` handler**

Just after the `InternalProcessingComplete` handler:

```rust
impl Handler<RefetchAndRestart> for StreamActor {
    type Result = ();

    fn handle(&mut self, _msg: RefetchAndRestart, ctx: &mut Context<Self>) {
        let Some(url) = self.stream_url.clone() else {
            error!(target: "stream", "RefetchAndRestart fired but no stream_url cached; shutting down.");
            if let Some(sup) = &self.supervisor_actor {
                sup.do_send(SystemShutdown);
            }
            return;
        };

        info!(target: "stream", "Refetching stream URL: {}", url);

        // Reset session-local state, then re-enter start_stream_processing.
        // The init oneshot will report is_live on success; SessionStarted updates
        // the counter (resets to 0 on success).
        self.latest_frame_buffer = None;
        self.detector_ready = true;
        self.is_processing_task_running = false;

        let (tx, rx) = oneshot::channel::<Result<bool, NorppaliveError>>();
        self.start_stream_processing(ctx, url, tx);

        let actor_address = ctx.address();
        actix::spawn(async move {
            match rx.await {
                Ok(Ok(is_live)) => {
                    actor_address.do_send(SessionStarted { is_live });
                }
                Ok(Err(e)) => {
                    error!(target: "stream", "Refetch init failed: {}", e);
                    // Treat as a session-end with error so the existing branching
                    // handles backoff + cap uniformly.
                    actor_address.do_send(InternalProcessingComplete { result: Err(e) });
                }
                Err(_) => {
                    error!(target: "stream", "Refetch init oneshot dropped");
                    actor_address.do_send(InternalProcessingComplete {
                        result: Err(NorppaliveError::Other(
                            "Refetch init oneshot dropped".to_string(),
                        )),
                    });
                }
            }
        });
    }
}
```

Note: `SessionStarted` already resets `consecutive_refetch_failures = 0`, so a successful refetch clears the counter on the next init-success.

**Step 5: Update existing imports**

At the top of `src/actors/stream.rs`, the `use crate::messages::{...}` block needs `RefetchAndRestart` added:

```rust
use crate::messages::{
    DetectorReady, FrameExtracted, GracefulStop, InternalProcessingComplete, LatestFrameAvailable,
    ProcessFrame, RefetchAndRestart, StartStream, StopStream,
};
```

**Step 6: Add behavior tests**

Append to the `#[cfg(test)] mod tests` block in `src/actors/stream.rs`:

```rust
#[test]
fn refetch_failure_cap_constant_matches_design() {
    assert_eq!(MAX_CONSECUTIVE_REFETCH_FAILURES, 10);
}
```

Behavior tests for the handler that exercise the full message flow would require stubbing out FFmpeg, which the existing shadow-actor pattern in `tests/actors/stream_tests.rs` does not yet do for refetch. We leave a manual test in Task 7 to cover the live-pipeline scenario; the pure functions covered in Tasks 1–3 cover the branching logic.

**Step 7: Build + run tests**

```bash
cargo build 2>&1 | tail -30
cargo test 2>&1 | grep -E "^test result:" | head -20
```

Expected: builds clean, all existing 59 tests pass, plus 12 new unit tests from Tasks 1–3 and 6 in `stream.rs`.

**Step 8: Commit**

```bash
git add src/actors/stream.rs src/messages/stream.rs src/messages/mod.rs
git commit -m "feat(stream): refetch yt-dlp URL on livestream session end"
```

---

## Task 7: Final verification

**Step 1: Run full test suite**

```bash
cargo test 2>&1 | tail -40
```

Expected: all suites green; new unit tests visible in the lib summary line.

**Step 2: Run clippy**

```bash
cargo clippy --all-targets -- -D warnings 2>&1 | tail -30
```

Expected: zero warnings/errors. Fix any new clippy notices on the touched code (typically unused-import or needless-borrow nits).

**Step 3: Verify the new yt-dlp invocation works against a real URL** (manual; user-driven)

```bash
yt-dlp -f 'best[height<=720]' --js-runtimes deno \
       --print url --print is_live \
       https://www.youtube.com/watch?v=Iyrokrk2hM4
```

Expected: two lines on stdout — a CDN URL on line 1, `True` on line 2.

If `True` is not produced, the design's liveness detection is broken and the implementer should escalate to the user before continuing; no amount of code-side fixup will make this work.

**Step 4: Commit any clippy fixes**

```bash
git add -u
git commit -m "chore(stream): clippy fixes for refetch path" # only if anything to commit
```

**Step 5: Report ready for merge**

Print the final commit log:

```bash
git log --oneline main..HEAD
```

Hand the branch back to the user for merge via `superpowers:finishing-a-development-branch`.

---

## Verification summary

After Task 7 you should see:

- 12 new pure-function unit tests under `cargo test --lib`
- 0 changes to the existing 59 tests' behavior
- `cargo clippy --all-targets -- -D warnings` clean
- `git log --oneline main..HEAD` shows 6–7 small commits, one per task
- Manual yt-dlp probe confirms `is_live=True` for the real Norppalive URL

## Out of scope (do not touch)

- The 1hr `MAX_STREAM_RUNTIME` cap — leave as is. With refetch on, it becomes a built-in URL rotator for livestreams and a normal cap for finite sources, both of which are correct.
- Supervisor-driven restart of `StreamActor` — pre-existing gap; supervisor restart factory does not re-issue `StartStream`. Independent of this feature.
- Config schema changes — backoff schedule and cap live in code constants.
- `tests/actors/stream_tests.rs` shadow `TestStreamActor` — do not mirror the new fields there. The new logic is exercised via pure-function tests; the shadow actor's purpose is to keep FFmpeg out of integration tests, not to mirror every internal field.
