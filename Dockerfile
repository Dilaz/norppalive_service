FROM rust:1.83-bullseye AS builder
WORKDIR /norppalive_service
COPY . .
RUN apt-get update
RUN apt-get install -y \
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
    pkg-config
RUN cargo build --release

FROM gcr.io/distroless/cc-debian12
COPY --from=builder /norppalive_service/target/release/norppalive_service /norppalive_service
ENTRYPOINT [ "./norppalive_service" ]
EXPOSE 3000
