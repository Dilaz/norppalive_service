# Livestream Auto-Refetch Design

**Goal:** When the configured stream is a livestream, automatically refetch a fresh URL and reconnect when the current session ends. For non-livestreams (finite VODs, local files), keep today's behavior — let the service shut down on EOF.

**Why:** YouTube CDN URLs resolved by yt-dlp expire, and the existing `MAX_STREAM_RUNTIME = 1 hour` cap forces a clean break every hour. Today both paths trigger `SystemShutdown`, so the bot has to be restarted externally to keep watching a 24/7 livestream.

---

## Architecture

The refetch loop lives **inside `StreamActor`**. It already owns the configured (un-resolved) URL, already gets `InternalProcessingComplete` when a session ends, and already has a re-entrant `StartStream` path that aborts the prior FFmpeg task before starting the next one. Putting the loop here is the smallest change that does the job.

**New StreamActor state:**

```rust
is_livestream: bool,
consecutive_refetch_failures: u32,
```

**End-of-session decision tree:**

```
InternalProcessingComplete arrives
  ├── Ok  && is_livestream  → schedule_refetch (delay from backoff schedule)
  ├── Err && is_livestream  → schedule_refetch (delay from backoff schedule)
  ├── Ok  && !is_livestream → SystemShutdown   (today's behavior)
  └── Err && !is_livestream → SystemShutdown   (today's behavior)
```

A successful re-init (oneshot from `start_stream_processing` reports `Ok(())`) resets `consecutive_refetch_failures = 0`. Any failure on the refetch path (yt-dlp non-zero, empty URL, `is_live` flipped to false, ffmpeg init error) increments the counter; at 10 consecutive failures the actor sends `SystemShutdown`.

---

## Liveness detection

We extend the existing yt-dlp call in `get_stream_url` to print both the resolved URL and `is_live` in one subprocess:

```
yt-dlp [existing flags] --print url --print is_live <stream_url>
```

Output is two lines (URL, then `True`/`False`/`NA`). Parse into:

```rust
struct ResolvedStream {
    url: String,
    is_live: bool,
}

fn get_stream_url(stream_url: &str) -> Result<ResolvedStream, NorppaliveError>
```

`NA` and parse failures map to `is_live = false` (safer default — we'd rather treat ambiguity as "let it die" than loop forever). Local files and any non-`http` input short-circuit to `is_live = false`.

We re-read `is_live` on **every** refetch, not just startup. YouTube transitions live broadcasts to VODs after they end, and a stale `is_live = true` belief would cause us to refetch forever and pull segments from the recorded VOD until we hit its end, then refetch again. Re-checking makes "stream genuinely ended" a terminating condition.

---

## Refetch flow & backoff schedule

**New self-message:**

```rust
#[derive(Message)]
#[rtype(result = "()")]
struct RefetchAndRestart;
```

**Schedule (hardcoded; not exposed in config until we have a reason to tune live):**

| attempt | delay |
|---|---|
| 1 | 5s |
| 2 | 15s |
| 3 | 30s |
| 4 | 60s |
| 5 | 120s |
| 6+ | 300s (capped) |

`max_consecutive_failures = 10` → `SystemShutdown`.

`schedule_refetch` calls `ctx.run_later(delay, |a, c| a.do_refetch(c))`. `do_refetch` re-resolves the URL via `get_stream_url`, updates `is_livestream` from the new response, and either feeds the new URL into `start_stream_processing` (preserving the existing reset of `latest_frame_buffer` and `detector_ready`) or — if `is_live` flipped to false — sends `SystemShutdown` directly.

We reset `consecutive_refetch_failures` on **init success** (oneshot reports `Ok(())`), not on first frame. Live streams sometimes have long initial buffering and we don't want slow starts to stack as failures.

---

## Edge cases

1. **Stream ended → became VOD.** Refetch's `is_live` returns false → `SystemShutdown`. No infinite loop.
2. **yt-dlp / network blip.** Failure counter ticks up, backoff applies, recovers on the next success.
3. **YouTube hard rate-limit.** Capped at 10 consecutive failures → `SystemShutdown`. Pod restart via Kubernetes.
4. **Local file or other finite source.** `is_live = false` → existing "let it die" path on EOF. No regression.
5. **Initial `StartStream` failure** (today's `StreamInitializationFailed` path). Unchanged. Refetch is for runtime resilience, not bootstrap. A startup failure still goes through the existing shutdown path.
6. **Ctrl+C / `GracefulStop` arrives during the backoff window.** `ctx.run_later` is bound to actor lifetime; `ctx.stop()` drops the scheduled future. Verify on a manual test.
7. **Concurrent end + refetch races.** `start_stream_processing` aborts the previous task and clears `shutdown_signal` before starting (existing logic). Back-to-back ends cannot pile up overlapping FFmpeg threads.

---

## Out of scope

- The 1hr `MAX_STREAM_RUNTIME` cap is left untouched. With auto-refetch on, it becomes a built-in URL rotator on livestreams and a normal cap on finite sources — exactly what we want, no change needed.
- Supervisor-driven actor restart is unchanged. Today the supervisor's restart factory does not re-send `StartStream` — that's a pre-existing gap independent of this feature.
- No config schema changes. Backoff schedule and max-failure cap live in code.
- yt-dlp invocation flags only change to add `--print url --print is_live`.

---

## Testing plan

**Unit (no network, no FFmpeg):**

- Factor `fn backoff_for(attempt: u32) -> Duration` and assert the schedule shape, including the cap at attempt 6+.
- Factor `fn parse_yt_dlp_resolve_output(stdout: &str) -> Result<ResolvedStream, NorppaliveError>` and test against:
  - `url\nTrue\n` → `is_live = true`
  - `url\nFalse\n` → `is_live = false`
  - `url\nNA\n` → `is_live = false`
  - empty stdout → error
  - single line / malformed → error or `is_live = false` (decide once during impl)

**Actor test (`#[actix::test]`):**

Inject a fake URL resolver via a trait stored on `StreamActor` (`Box<dyn UrlResolver + Send>`), so tests can drive `get_stream_url` outcomes without spawning yt-dlp:

```rust
trait UrlResolver: Send {
    fn resolve(&self, url: &str) -> Result<ResolvedStream, NorppaliveError>;
}
```

Production constructor wires the real yt-dlp resolver; tests wire a scripted fake.

Drive `InternalProcessingComplete` directly and assert:

- live + `Ok(())` → refetch is scheduled, then `start_stream_processing` invoked with fresh URL.
- live + `Err(_)` → refetch is scheduled.
- non-live + either result → `SystemShutdown` sent to supervisor mock.
- 10 consecutive refetch failures → `SystemShutdown`.
- Successful re-init resets the counter (failure 9 → success → failure 1, not failure 10).

**Manual:**

Point at the real Norppalive YouTube URL, run for >1hr, watch logs roll over the 1hr cap and reconnect cleanly with no detection-pipeline gap visible to OutputActor.
