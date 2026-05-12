# Docker Deployment

This directory contains the single-node local deployment for Reality Graph.

## Build The API Image

```bash
docker build -f infra/docker/Dockerfile -t reality-graph-api:local .
```

The image runs `rg-api` with `RG_API_ADDR=0.0.0.0:8080` and exposes `/v1/health` plus `/v1/metrics`.

## Run Local Compose

```bash
docker compose -f infra/docker/docker-compose.yml up --build
```

Useful endpoints:

- API: `http://localhost:8080/v1/health`
- Prometheus: `http://localhost:9090`
- Grafana: `http://localhost:3000` with `admin/admin`
- Qdrant HTTP: `http://localhost:6333`

Postgres is optional for the current MVP and is behind a Compose profile:

```bash
docker compose -f infra/docker/docker-compose.yml --profile postgres up --build
```

The Rust core remains the source of truth. Qdrant is a sidecar retrieval index, and Postgres is reserved for later operational metadata or analytical paths.
