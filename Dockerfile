FROM rust:1-bullseye AS builder

# Set to 1 to build with Sentry (e.g. docker build --build-arg SENTRY_FEATURE=1)
ARG SENTRY_FEATURE=0

WORKDIR /app

# Cache dependency build
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release || true

# Build real binary (use --no-cache when switching branches or after code fixes)
COPY . .
RUN if [ "$SENTRY_FEATURE" = "1" ]; then cargo build --release --features sentry; else cargo build --release; fi

FROM debian:bullseye-slim

WORKDIR /app

# ca-certificates for HTTPS; docker.io (CLI) for watchdog container to run "docker ps"
RUN apt-get update && apt-get install -y ca-certificates docker.io && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/sublime /app/sublime
COPY --from=builder /app/config.toml /app/config.toml
COPY --from=builder /app/migrations /app/migrations

ENV RUST_LOG=info

CMD ["/app/sublime"]

