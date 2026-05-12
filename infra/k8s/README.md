# Kubernetes Deployment

These manifests define a production-shaped starter deployment:

- `reality-graph-api` Deployment
- `reality-graph-worker` Deployment
- `qdrant` StatefulSet
- `object-storage` StatefulSet backed by MinIO
- Prometheus Deployment
- Grafana Deployment
- ClusterIP Services and an optional NGINX Ingress

The API uses a single file-backed event log PVC in this starter manifest, so the API Deployment is pinned to one replica. Move `RG_EVENT_LOG_PATH` to a shared production log service before scaling API replicas horizontally. The worker currently runs the same `rg-api` image on port `8081` and exposes the same health/metrics endpoints. Replace its command/image when a dedicated ingestion worker binary is added.

## Build And Push Image

```bash
docker build -f infra/docker/Dockerfile -t registry.example.com/reality-graph-api:0.1.0 .
docker push registry.example.com/reality-graph-api:0.1.0
```

Update `image:` in `api-deployment.yaml` and `worker-deployment.yaml` to your immutable registry tag before applying to a cluster.

## Create Secrets

Do not apply `examples/secret.example.yaml` with real credentials committed. Create secrets from your secret manager or use this local-only command:

```bash
kubectl create namespace reality-graph
kubectl -n reality-graph create secret generic reality-graph-secrets \
  --from-literal=RG_API_KEYS='replace-with-long-random-key:api-writer:tenant-default:reader|writer' \
  --from-literal=GRAFANA_ADMIN_PASSWORD='replace-me' \
  --from-literal=MINIO_ROOT_USER='realitygraph' \
  --from-literal=MINIO_ROOT_PASSWORD='replace-me'
```

`RG_API_KEYS` accepts comma-separated entries in the form `key:service-account:tenant:roles`, where roles are `reader`, `writer`, or `admin` joined with `|`.

## Apply Manifests

```bash
kubectl apply -k infra/k8s/
```

Check rollout and health:

```bash
kubectl -n reality-graph rollout status deployment/reality-graph-api
kubectl -n reality-graph rollout status deployment/reality-graph-worker
kubectl -n reality-graph rollout status statefulset/qdrant
kubectl -n reality-graph get pods
```

Port-forward for local inspection:

```bash
kubectl -n reality-graph port-forward svc/reality-graph-api 8080:8080
kubectl -n reality-graph port-forward svc/prometheus 9090:9090
kubectl -n reality-graph port-forward svc/grafana 3000:3000
```

Health endpoints:

- API: `/v1/health`
- API metrics: `/v1/metrics`
- Worker health: `/v1/health` on port `8081`
- Prometheus: `/-/healthy`
- Grafana: `/api/health`
