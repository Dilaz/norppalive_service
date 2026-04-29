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
    unzip \
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
# Bun — yt-dlp 2026.03.17+ requires a JS runtime for YouTube; the service
# invokes yt-dlp with `--js-runtime bun`, so install bun specifically.
ARG BUN_VERSION=bun-v1.1.38
RUN ARCH="$(dpkg --print-architecture)" \
    && case "$ARCH" in \
         amd64) BUN_ARCH="bun-linux-x64" ;; \
         arm64) BUN_ARCH="bun-linux-aarch64" ;; \
         *) echo "unsupported arch: $ARCH" && exit 1 ;; \
       esac \
    && curl -fL -o /tmp/bun.zip \
        "https://github.com/oven-sh/bun/releases/download/${BUN_VERSION}/${BUN_ARCH}.zip" \
    && unzip /tmp/bun.zip -d /tmp/ \
    && mv "/tmp/${BUN_ARCH}/bun" /usr/local/bin/bun \
    && chmod +x /usr/local/bin/bun \
    && rm -rf /tmp/bun.zip "/tmp/${BUN_ARCH}" \
    && /usr/local/bin/bun --version
# yt-dlp PyInstaller standalone — pinned via build arg so rebuilds are reproducible
ARG YTDLP_VERSION=2026.03.17
RUN curl -fL -o /usr/local/bin/yt-dlp \
        "https://github.com/yt-dlp/yt-dlp/releases/download/${YTDLP_VERSION}/yt-dlp_linux" \
    && chmod +x /usr/local/bin/yt-dlp \
    && /usr/local/bin/yt-dlp --version
RUN useradd --system --create-home --uid 1000 norppa
COPY --from=builder /norppalive_service/target/release/norppalive_service /usr/local/bin/norppalive_service
USER 1000
WORKDIR /home/norppa
ENTRYPOINT ["/usr/local/bin/norppalive_service"]
