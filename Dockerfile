FROM rust:1.83-bullseye AS builder
WORKDIR /norppalive_service
COPY . .
RUN apt-get update
RUN apt-get -y install ffmpeg libavutil-dev pkg-config libavformat-dev libavfilter-dev libavdevice-dev clang libclang-dev llvm-dev cmake
RUN cargo build --release

FROM gcr.io/distroless/cc-debian12
COPY --from=builder /norppalive_service/target/release/norppalive_service /norppalive_service
ENTRYPOINT [ "./norppalive_service" ]
EXPOSE 3000
