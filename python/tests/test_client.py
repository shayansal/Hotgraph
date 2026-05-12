import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from reality_graph import RealityGraphClient


def test_python_client_wraps_rest_api_without_engine_logic():
    server = RecordingServer()
    server.start()
    try:
        rg = RealityGraphClient(server.url)

        entity = rg.create_entity(type="Company", name="Oracle")
        assert entity.id == "entity-oracle"
        assert server.requests[-1] == (
            "POST",
            "/v1/entities",
            {"type": "Company", "name": "Oracle"},
        )

        assertion = rg.add_assertion(
            subject="person-a",
            predicate="WORKED_AT",
            object="entity-oracle",
            valid_from="2021-01-01",
            valid_to="2024-12-31",
            confidence=0.92,
            sources=["source-employment"],
        )
        assert assertion.assertion_id == "assertion-worked-at"
        assert server.requests[-1] == (
            "POST",
            "/v1/assertions",
            {
                "subject": "person-a",
                "predicate": "WORKED_AT",
                "object": {"entity_id": "entity-oracle"},
                "valid_from": "2021-01-01",
                "valid_to": "2024-12-31",
                "confidence": 0.92,
                "sources": ["source-employment"],
            },
        )

        state = rg.entity_state(entity_id="person-a", valid_at="2023-01-01")
        assert state["entity"]["id"] == "person-a"
        assert server.requests[-1] == (
            "GET",
            "/v1/entities/person-a/state?valid_at=2023-01-01",
            None,
        )

        pack = rg.evidence_pack(
            query="Where did Person A work?",
            graph_query={
                "subject": {"entity_id": "person-a"},
                "predicate": "WORKED_AT",
                "valid_at": "2023-01-01",
            },
        )
        assert pack["query"] == "Where did Person A work?"
        assert server.requests[-1][0:2] == ("POST", "/v1/evidence-pack")

        candidates = rg.ingest_document(
            id="doc-1",
            source_id="source-employment",
            content="candidate: Person A | worked_at | Company B",
            uri="file://doc.txt",
        )
        assert candidates["candidates"][0]["subject_text"] == "Person A"
        assert server.requests[-1][0:2] == ("POST", "/v1/ingest/document")
    finally:
        server.stop()


class RecordingServer:
    def __init__(self):
        self.requests = []
        handler = self._handler()
        self.httpd = HTTPServer(("127.0.0.1", 0), handler)
        self.url = f"http://127.0.0.1:{self.httpd.server_port}"
        self.thread = threading.Thread(target=self.httpd.serve_forever, daemon=True)

    def start(self):
        self.thread.start()

    def stop(self):
        self.httpd.shutdown()
        self.thread.join(timeout=5)
        self.httpd.server_close()

    def _handler(self):
        outer = self

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                outer.requests.append(("GET", self.path, None))
                if self.path.startswith("/v1/entities/person-a/state"):
                    self._send(
                        {
                            "entity": {
                                "id": "person-a",
                                "entity_type": "Person",
                                "canonical_name": "Person A",
                                "created_tx": 1,
                            },
                            "assertions": [],
                        }
                    )
                else:
                    self._send({"status": "ok"})

            def do_POST(self):
                length = int(self.headers.get("content-length", "0"))
                body = self.rfile.read(length).decode("utf-8")
                payload = json.loads(body) if body else None
                outer.requests.append(("POST", self.path, payload))
                if self.path == "/v1/entities":
                    self._send(
                        {
                            "id": "entity-oracle",
                            "entity_type": payload["type"],
                            "canonical_name": payload["name"],
                            "created_tx": 1,
                        }
                    )
                elif self.path == "/v1/assertions":
                    self._send(
                        {
                            "assertion_id": "assertion-worked-at",
                            "subject": payload["subject"],
                            "predicate": payload["predicate"],
                            "object": payload["object"],
                            "valid_from": 20210101,
                            "valid_to": 20241231,
                            "tx_from": 2,
                            "tx_to": None,
                            "confidence": payload["confidence"],
                            "sources": payload["sources"],
                            "context": "global",
                            "status": "active",
                        }
                    )
                elif self.path == "/v1/evidence-pack":
                    self._send({"query": payload["query"], "assertions": []})
                elif self.path == "/v1/ingest/document":
                    self._send(
                        {
                            "document_id": payload["id"],
                            "candidates": [
                                {
                                    "subject_text": "Person A",
                                    "predicate_text": "worked_at",
                                    "object_text": "Company B",
                                }
                            ],
                        }
                    )
                else:
                    self._send({})

            def log_message(self, format, *args):
                return

            def _send(self, payload):
                body = json.dumps(payload).encode("utf-8")
                self.send_response(200)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

        return Handler
