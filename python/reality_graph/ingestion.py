"""Ingestion helpers for the thin Reality Graph HTTP SDK."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class DocumentInput:
    id: str
    source_id: str
    content: str
    uri: str | None = None
