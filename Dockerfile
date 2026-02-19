FROM rust:1-bullseye AS builder

WORKDIR /app

# Cache dependency build
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release || true

# Build real binary (use --no-cache when switching branches or after code fixes)
COPY . .
RUN cargo build --release

FROM debian:bullseye-slim

WORKDIR /app

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/sublime /app/sublime
COPY --from=builder /app/config.toml /app/config.toml
COPY --from=builder /app/migrations /app/migrations

ENV RUST_LOG=info

CMD ["/app/sublime"]

