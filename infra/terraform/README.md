# Terraform

Terraform is intentionally a placeholder in this phase. The first production deployment path is:

1. Build and push the API image from `infra/docker/Dockerfile`.
2. Provision a Kubernetes cluster, ingress controller, persistent volumes, DNS, TLS, and secret manager with your platform tooling.
3. Apply the manifests in `infra/k8s/`.

Future Terraform modules should own cloud-specific infrastructure only: Kubernetes cluster, node pools, object storage buckets, secret manager entries, DNS, TLS certificates, and monitoring persistence.
