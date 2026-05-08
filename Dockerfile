# Build stage
FROM rust:1-slim-bookworm AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl-dev \
    pkg-config \
    ca-certificates \
    libzstd1 \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY ingest-router ./ingest-router
COPY locator ./locator
COPY proxy ./proxy
COPY synapse ./synapse
COPY shared ./shared

RUN cargo build --release

RUN mkdir /stage && cp --parents /usr/lib/$(gcc -print-multiarch)/libzstd.so.1 /stage

# Runtime stage
FROM gcr.io/distroless/cc-debian13:nonroot
WORKDIR /app

COPY --from=builder /app/target/release/synapse synapse
COPY --from=builder /stage/ /

ENTRYPOINT ["/app/synapse"]
CMD []
