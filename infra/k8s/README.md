# Kubernetes Deployment

These manifests define a production-shaped starter deployment:

- `reality-graph-api` Deployment
- `reality-graph-worker` Deployment
- `qdrant` StatefulSet
- `object-storage` StatefulSet backed by MinIO
- Prometheus Deployment
- Grafana Deployment
- API backup PVC and scheduled backup CronJob
- default-deny ingress NetworkPolicy with explicit internal allows
- PodDisruptionBudgets for API and Qdrant
- ClusterIP Services and an optional NGINX Ingress

The API uses a single-writer redb file plus local append/audit logs on one PVC in
this starter manifest, so the API Deployment is pinned to one replica and sets
`HOTGRAPH_NODE_ROLE=writer`. Do not scale writers horizontally until follower
tailing, write proxying, lease failover, and split-brain prevention have passed
their production gates. The worker currently runs the same `rg-api` image on port
`8081` and exposes the same health/metrics endpoints. Replace its command/image
when a dedicated ingestion worker binary is added.

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

## Backups And Restore

`backup-cronjob.yaml` copies the API redb database, WAL, and idempotency log from the API PVC
to `reality-graph-api-backups` every 15 minutes and writes SHA-256 sums beside
each backup. This is a starter cluster-local backup only; production deployments
must mirror these artifacts to object storage or a managed backup service.

Restore is intentionally manual:

1. Scale `deployment/reality-graph-api` to zero.
2. Choose a backup directory from `reality-graph-api-backups`.
3. Restore `hotgraph.redb`, `events.log`, and `idempotency.log` into `reality-graph-api-data`.
4. Start a one-shot restore verification job or run the API locally against the
   restored PVC.
5. Compare state hash, event checksum, and post-restore query parity.
6. Scale the API back to one replica.

Do not run a restore job against a live writer. File-backed mode is single-node
only until a shared durable log or leader/follower design is implemented.
