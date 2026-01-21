# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Norppalive Service detects Saimaa ringed seals from the Finnish WWF Norppalive livestream using an actor-based architecture (Actix). It processes video frames through an AI detection API and posts findings to social media platforms (Twitter, Mastodon, Bluesky) and Kafka.

## Build & Run Commands

```bash
cargo build                           # Debug build
cargo build --release                 # Release build (optimized for size with LTO)
cargo run                             # Run with default config.toml
cargo run -- --config <path>          # Run with custom config file
```

## Testing

```bash
cargo test                            # Run all tests
cargo test -- --nocapture             # Show println! output
cargo test <name>                     # Run specific test by name

# Test categories
cargo test actors                     # Actor tests
cargo test integration_tests          # Integration tests
cargo test supervisor                 # SupervisorActor tests
cargo test detection                  # DetectionActor tests
cargo test stream                     # StreamActor tests
cargo test twitter                    # TwitterActor tests
```

Tests use `#[actix::test]` and mock services - no external dependencies required.

## Architecture

**Actor System (Actix)**: All components are independent actors communicating via messages.

```
StreamActor (FFmpeg) → ProcessFrame → DetectionActor (AI API)
                                           ↓
                                    DetectionCompleted
                                           ↓
                                      OutputActor
                                    ↓    ↓    ↓    ↓
                              Twitter Mastodon Bluesky Kafka
                                     (Service Actors)

SupervisorActor monitors all actors, performs health checks every 30s,
and restarts failed actors using registered factory functions.
```

**Key Patterns**:
- Actors with factories (service actors) get automatic restart on failure
- Core actors (Stream, Detection, Output) are registered without factories due to complex dependencies
- Message bus abstraction (`src/message_bus/`) enables future HTTP-based service communication

## Key Files

- `src/main.rs` - Actor system initialization and startup sequence
- `src/actors/supervisor.rs` - Health monitoring, factory-based restarts (647 lines)
- `src/actors/stream.rs` - FFmpeg video processing, frame extraction (826 lines)
- `src/actors/detection.rs` - AI API calls, confidence filtering, heatmap data (446 lines)
- `src/actors/output.rs` - Social media coordination, rate limiting (610 lines)
- `src/config.rs` - Global CONFIG singleton, TOML parsing
- `src/messages/` - All actor message type definitions

## Configuration

Config file location set by `CONFIG_PATH` env var (default: `config.toml`). Copy `config.toml.example` to get started.

Key config sections:
- `[stream]` - Video source URL, keyframe-only mode, frame delay
- `[detection]` - AI API URL, confidence thresholds, heatmap settings
- `[output]` - Post intervals, services to enable, visualization colors
- `[twitter]`, `[mastodon]`, `[bluesky]`, `[kafka]` - Service credentials

## Adding a New Service

1. Create actor in `src/actors/services/your_service.rs`
2. Implement handlers for `ServicePost` and `GetServiceStatus` messages
3. Export in `src/actors/services/mod.rs`
4. Add config struct in `src/config.rs` and update `Service` enum
5. Initialize in `main.rs` following the pattern of existing services
6. Register with supervisor using `RegisterActor::with_factory()` for auto-restart

## System Dependencies

Requires FFmpeg development libraries:
```bash
# Ubuntu/Debian
apt install ffmpeg libavcodec-dev libavdevice-dev libavfilter-dev \
    libavformat-dev libavutil-dev libpostproc-dev libswresample-dev libswscale-dev

# Also needed: yt-dlp for YouTube stream URL extraction
pip install yt-dlp
```

## Environment Variables

- `CONFIG_PATH` - Config file path (default: `config.toml`)
- `RUST_LOG` - Log level (default: `norppalive_service=debug`)
