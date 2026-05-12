"""AI-facing helpers for the thin Reality Graph HTTP SDK."""

from __future__ import annotations

from typing import Any

from .client import RealityGraphClient


def evidence_pack(client: RealityGraphClient, **request: Any) -> dict[str, Any]:
    return client.evidence_pack(**request)
