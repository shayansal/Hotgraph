# Kubernetes Deployment

These manifests define a production-shaped starter deployment:

- `reality-graph-api` Deployment
- `reality-graph-worker` Deployment
- `qdrant` StatefulSet
- `object-storage` StatefulSet backed by MinIO
- Prometheus Deployment
- Grafana Deployment
- API backup PVC and scheduled backup CronJob
- manual restore and migration-check Job templates
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

Environment overlays are available for explicit deployment lanes:

```bash
kubectl apply -k infra/k8s-overlays/dev
kubectl apply -k infra/k8s-overlays/staging
kubectl apply -k infra/k8s-overlays/prod
```

The prod overlay still sets `HOTGRAPH_PRODUCTION_CLAIM=false`. Flip that only
after the release record links passing crash, restore, security, benchmark, and
pilot evidence for the exact commit and image digest.

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

`backup-cronjob.yaml` runs `hotgraph backup create` against `/data/hotgraph.redb`
every 15 minutes, verifies the artifact with `hotgraph backup verify`, and writes
SHA-256 sums beside the backup. This is a starter cluster-local backup only;
production deployments must mirror these artifacts to object storage or a
managed backup service.

Restore is intentionally manual:

1. Scale `deployment/reality-graph-api` to zero.
2. Choose a backup directory from `reality-graph-api-backups`.
3. Create or bind a clean PVC named `reality-graph-api-restore-target`.
4. Set `HOTGRAPH_RESTORE_BACKUP` in `restore-job.yaml` to the chosen backup
   directory and set `HOTGRAPH_RESTORE_CONFIRM=restore-to-clean-pvc`.
5. Apply `restore-pvc.yaml` and `restore-job.yaml`.
6. Compare `RESTORE_VERIFY.txt`, state hash, event checksum, and query parity.
7. Promote the restored PVC only after verification, then scale the API back to
   one replica.

Do not run a restore job against a live writer. File-backed mode is single-node
only until a shared durable log or leader/follower design is implemented.

Before a migration or upgrade, run `migration-job.yaml` against the writer PVC.
It creates and verifies a pre-migration backup artifact from the existing redb
store. It is a guardrail, not a replacement for schema migration tests.
