# Key Rotation Runbook

Use for scheduled or emergency KMS key rotation.

1. Confirm production uses `AwsKmsProvider`, not `LocalDevKmsProvider`.
2. Verify KMS health and IAM permissions.
3. Rotate the KMS key reference and create a new data key.
4. Re-encrypt affected event logs, snapshots, source stores, and backups.
5. Verify old records remain readable and tampered records fail authentication.
6. Record key ID, rotation time, evidence artifact IDs, and audit event IDs.
