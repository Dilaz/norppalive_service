FROM rust:1.91-bookworm AS builder
WORKDIR /norppalive_service
COPY . .
RUN apt-get update && apt-get install -y --no-install-recommends \
    ffmpeg \
    libavcodec-dev \
    libavdevice-dev \
    libavfilter-dev \
    libavformat-dev \
    libavutil-dev \
    libpostproc-dev \
    libswresample-dev \
    libswscale-dev \
    libclang-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    ffmpeg \
    libavcodec59 \
    libavdevice59 \
    libavfilter8 \
    libavformat59 \
    libavutil57 \
    libpostproc56 \
    libswresample4 \
    libswscale6 \
    && rm -rf /var/lib/apt/lists/*
# yt-dlp PyInstaller standalone — pinned via build arg so rebuilds are reproducible
ARG YTDLP_VERSION=2025.10.22
RUN curl -fL -o /usr/local/bin/yt-dlp \
        "https://github.com/yt-dlp/yt-dlp/releases/download/${YTDLP_VERSION}/yt-dlp_linux" \
    && chmod +x /usr/local/bin/yt-dlp \
    && /usr/local/bin/yt-dlp --version
RUN useradd --system --create-home --uid 1000 norppa
COPY --from=builder /norppalive_service/target/release/norppalive_service /usr/local/bin/norppalive_service
USER 1000
WORKDIR /home/norppa
ENTRYPOINT ["/usr/local/bin/norppalive_service"]
