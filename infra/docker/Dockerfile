# syntax=docker/dockerfile:1.7

FROM rust:1-bookworm AS chef
WORKDIR /workspace

FROM chef AS planner
COPY . .
RUN cargo fetch --locked

FROM chef AS builder
COPY --from=planner /usr/local/cargo /usr/local/cargo
COPY . .
RUN cargo build --locked --release -p rg-api

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd --system --gid 10001 realitygraph \
    && useradd --system --uid 10001 --gid realitygraph --home-dir /nonexistent --shell /usr/sbin/nologin realitygraph \
    && mkdir -p /var/lib/reality-graph \
    && chown -R realitygraph:realitygraph /var/lib/reality-graph

COPY --from=builder /workspace/target/release/rg-api /usr/local/bin/rg-api

ENV RG_API_ADDR=0.0.0.0:8080
ENV HOTGRAPH_NODE_ROLE=writer
ENV RG_REDB_PATH=/var/lib/reality-graph/hotgraph.redb
ENV RG_EVENT_LOG_PATH=/var/lib/reality-graph/events.log
ENV RG_IDEMPOTENCY_LOG_PATH=/var/lib/reality-graph/idempotency.log
ENV RUST_LOG=info

USER realitygraph
EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD curl --fail --silent http://127.0.0.1:8080/v1/health >/dev/null || exit 1

ENTRYPOINT ["/usr/local/bin/rg-api"]
