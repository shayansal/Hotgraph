"""Minimal dependency-free RMP HTTP reference client."""

from __future__ import annotations

import json
import uuid
from dataclasses import dataclass
from typing import Any
from urllib import error, request


RMP_VERSION = "1.0.0"


@dataclass(frozen=True)
class RmpClient:
    base_url: str
    token: str
    principal_id: str
    tenant_id: str
    agent_id: str | None = None
    client_name: str = "rmp-python-reference"

    def remember(
        self,
        memory: dict[str, Any],
        *,
        idempotency_key: str | None = None,
    ) -> dict[str, Any]:
        return self._call(
            "REMEMBER",
            {"memory": memory},
            path="/rmp/v1/remember",
            idempotency_key=idempotency_key or f"remember-{memory.get('id', uuid.uuid4())}",
        )

    def recall(self, task: str, **options: Any) -> dict[str, Any]:
        return self._call("RECALL", {"task": task, "options": options}, path="/rmp/v1/recall")

    def verify(self, claim: dict[str, Any]) -> dict[str, Any]:
        return self._call("VERIFY", {"claim": claim}, path="/rmp/v1/verify")

    def revise(
        self,
        memory: dict[str, Any],
        *,
        idempotency_key: str | None = None,
    ) -> dict[str, Any]:
        return self._call(
            "REVISE",
            {"memory": memory},
            path="/rmp/v1/revise",
            idempotency_key=idempotency_key or f"revise-{memory.get('id', uuid.uuid4())}",
        )

    def forget(
        self,
        target_ids: list[str],
        *,
        idempotency_key: str | None = None,
    ) -> dict[str, Any]:
        return self._call(
            "FORGET",
            {"target_ids": target_ids},
            path="/rmp/v1/forget",
            idempotency_key=idempotency_key or f"forget-{uuid.uuid4()}",
        )

    def explain(self, question: str, target_ids: list[str] | None = None) -> dict[str, Any]:
        return self._call(
            "EXPLAIN",
            {"question": question, "target_ids": target_ids or []},
            path="/rmp/v1/explain",
        )

    def simulate(self, simulation: dict[str, Any]) -> dict[str, Any]:
        return self._call("SIMULATE", {"simulation": simulation}, path="/rmp/v1/simulate")

    def ground(self, query: str, evidence: list[dict[str, Any]]) -> dict[str, Any]:
        return self._call("GROUND", {"query": query, "evidence": evidence}, path="/rmp/v1/ground")

    def compress(self, context_pack: dict[str, Any], target_tokens: int) -> dict[str, Any]:
        return self._call(
            "COMPRESS",
            {
                "context_pack": context_pack,
                "compression": {
                    "target_tokens": target_tokens,
                    "preserve_contradictions": True,
                    "preserve_citations": True,
                },
            },
            path="/rmp/v1/compress",
        )

    def share(
        self,
        target_scope: str,
        memory_ids: list[str] | None = None,
        claim_ids: list[str] | None = None,
        *,
        idempotency_key: str | None = None,
    ) -> dict[str, Any]:
        return self._call(
            "SHARE",
            {
                "share": {
                    "target_scope": target_scope,
                    "memory_ids": memory_ids or [],
                    "claim_ids": claim_ids or [],
                }
            },
            path="/rmp/v1/share",
            idempotency_key=idempotency_key or f"share-{uuid.uuid4()}",
        )

    def audit(self, resource_ids: list[str]) -> dict[str, Any]:
        return self._call("AUDIT", {"audit": {"resource_ids": resource_ids}}, path="/rmp/v1/audit")

    def _call(
        self,
        operation: str,
        payload: dict[str, Any],
        *,
        path: str = "/rmp/v1/exchange",
        idempotency_key: str | None = None,
    ) -> dict[str, Any]:
        envelope = {
            "protocol": "rmp",
            "version": RMP_VERSION,
            "operation": operation,
            "request_id": f"req_{uuid.uuid4().hex}",
            "actor": {
                "principal_id": self.principal_id,
                "tenant_id": self.tenant_id,
                "agent_id": self.agent_id,
                "client_name": self.client_name,
            },
            "request": payload,
        }
        if idempotency_key is not None:
            envelope["idempotency_key"] = idempotency_key

        body = json.dumps(envelope, sort_keys=True).encode("utf-8")
        http_request = request.Request(
            f"{self.base_url.rstrip('/')}{path}",
            data=body,
            method="POST",
            headers={
                "Authorization": f"Bearer {self.token}",
                "Content-Type": "application/rmp+json",
                "Accept": "application/rmp+json",
                "RMP-Version": RMP_VERSION,
                **({"Idempotency-Key": idempotency_key} if idempotency_key else {}),
            },
        )
        try:
            with request.urlopen(http_request, timeout=30) as response:
                return json.loads(response.read().decode("utf-8"))
        except error.HTTPError as exc:
            detail = exc.read().decode("utf-8")
            raise RuntimeError(f"RMP request failed: HTTP {exc.code}: {detail}") from exc
