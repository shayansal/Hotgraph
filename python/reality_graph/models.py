"""Typed response models for the Reality Graph HTTP SDK."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True)
class Entity:
    id: str
    entity_type: str
    canonical_name: str | None
    created_tx: int

    @classmethod
    def from_dict(cls, payload: dict[str, Any]) -> "Entity":
        return cls(
            id=payload["id"],
            entity_type=payload["entity_type"],
            canonical_name=payload.get("canonical_name"),
            created_tx=payload["created_tx"],
        )


@dataclass(frozen=True)
class Source:
    id: str
    source_type: str
    uri: str | None
    content_hash: str
    observed_at: int
    trust_score: float | None

    @classmethod
    def from_dict(cls, payload: dict[str, Any]) -> "Source":
        return cls(
            id=payload["id"],
            source_type=payload["source_type"],
            uri=payload.get("uri"),
            content_hash=payload["content_hash"],
            observed_at=payload["observed_at"],
            trust_score=payload.get("trust_score"),
        )


@dataclass(frozen=True)
class Assertion:
    assertion_id: str
    subject: str
    predicate: str
    object: dict[str, Any]
    valid_from: int
    valid_to: int | None
    tx_from: int
    tx_to: int | None
    confidence: float
    sources: list[str]
    context: str
    status: str

    @classmethod
    def from_dict(cls, payload: dict[str, Any]) -> "Assertion":
        return cls(
            assertion_id=payload["assertion_id"],
            subject=payload["subject"],
            predicate=payload["predicate"],
            object=payload["object"],
            valid_from=payload["valid_from"],
            valid_to=payload.get("valid_to"),
            tx_from=payload["tx_from"],
            tx_to=payload.get("tx_to"),
            confidence=payload["confidence"],
            sources=list(payload["sources"]),
            context=payload["context"],
            status=payload["status"],
        )
