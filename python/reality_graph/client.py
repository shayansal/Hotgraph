"""HTTP client wrapper for Reality Graph."""

from __future__ import annotations

import json
from typing import Any
from urllib.error import HTTPError
from urllib.parse import urlencode
from urllib.request import Request, urlopen

from .models import Assertion, Entity, Source


class RealityGraphError(RuntimeError):
    """Raised when the Reality Graph API returns an error response."""


class RealityGraphClient:
    def __init__(self, base_url: str, *, timeout: float = 30.0) -> None:
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout

    def create_entity(
        self, *, type: str, name: str | None = None, id: str | None = None
    ) -> Entity:
        payload: dict[str, Any] = {"type": type}
        if id is not None:
            payload["id"] = id
        if name is not None:
            payload["name"] = name
        return Entity.from_dict(self._request("POST", "/v1/entities", payload))

    def add_source(
        self,
        *,
        content_hash: str,
        id: str | None = None,
        source_type: str = "Document",
        uri: str | None = None,
        trust_score: float | None = None,
    ) -> Source:
        payload: dict[str, Any] = {
            "source_type": source_type,
            "content_hash": content_hash,
        }
        if id is not None:
            payload["id"] = id
        if uri is not None:
            payload["uri"] = uri
        if trust_score is not None:
            payload["trust_score"] = trust_score
        return Source.from_dict(self._request("POST", "/v1/sources", payload))

    def add_assertion(
        self,
        *,
        subject: str,
        predicate: str,
        object: str | dict[str, Any],
        valid_from: str,
        valid_to: str | None = None,
        confidence: float,
        sources: list[str],
        id: str | None = None,
        context: str | None = None,
    ) -> Assertion:
        payload: dict[str, Any] = {
            "subject": subject,
            "predicate": predicate,
            "object": self._object_payload(object),
            "valid_from": valid_from,
            "confidence": confidence,
            "sources": sources,
        }
        if id is not None:
            payload["id"] = id
        if valid_to is not None:
            payload["valid_to"] = valid_to
        if context is not None:
            payload["context"] = context
        return Assertion.from_dict(self._request("POST", "/v1/assertions", payload))

    def entity(self, entity_id: str) -> Entity:
        return Entity.from_dict(self._request("GET", f"/v1/entities/{entity_id}"))

    def entity_state(self, *, entity_id: str, valid_at: str | None = None) -> dict[str, Any]:
        query = f"?{urlencode({'valid_at': valid_at})}" if valid_at is not None else ""
        return self._request("GET", f"/v1/entities/{entity_id}/state{query}")

    def assertion(self, assertion_id: str) -> Assertion:
        return Assertion.from_dict(self._request("GET", f"/v1/assertions/{assertion_id}"))

    def source(self, source_id: str) -> Source:
        return Source.from_dict(self._request("GET", f"/v1/sources/{source_id}"))

    def query(self, **payload: Any) -> dict[str, Any]:
        return self._request("POST", "/v1/query", payload)

    def path(self, **payload: Any) -> dict[str, Any]:
        return self._request("POST", "/v1/path", payload)

    def evidence_pack(self, **payload: Any) -> dict[str, Any]:
        return self._request("POST", "/v1/evidence-pack", payload)

    def ingest_document(
        self,
        *,
        id: str,
        source_id: str,
        content: str,
        uri: str | None = None,
    ) -> dict[str, Any]:
        payload = {
            "id": id,
            "source_id": source_id,
            "content": content,
        }
        if uri is not None:
            payload["uri"] = uri
        return self._request("POST", "/v1/ingest/document", payload)

    def health(self) -> dict[str, Any]:
        return self._request("GET", "/v1/health")

    def metrics(self) -> dict[str, Any]:
        return self._request("GET", "/v1/metrics")

    def _request(
        self, method: str, path: str, payload: dict[str, Any] | None = None
    ) -> dict[str, Any]:
        body = None if payload is None else json.dumps(payload).encode("utf-8")
        request = Request(
            f"{self.base_url}{path}",
            data=body,
            method=method,
            headers={"content-type": "application/json"},
        )
        try:
            with urlopen(request, timeout=self.timeout) as response:
                data = response.read()
        except HTTPError as error:
            message = error.read().decode("utf-8")
            raise RealityGraphError(message) from error
        if not data:
            return {}
        return json.loads(data.decode("utf-8"))

    @staticmethod
    def _object_payload(value: str | dict[str, Any]) -> dict[str, Any]:
        if isinstance(value, dict):
            return value
        return {"entity_id": value}
