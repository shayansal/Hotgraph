import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const dataPath = new URL("../src/lib/labData.json", import.meta.url);

test("lab console data covers every requested command-center view", async () => {
  const data = JSON.parse(await readFile(dataPath, "utf8"));
  const viewIds = data.views.map((view) => view.id);

  assert.deepEqual(viewIds, [
    "eval-leaderboard",
    "agent-memory-health",
    "evidence-trace-explorer",
    "contradiction-map",
    "source-trust-dashboard",
    "latency-cost-dashboard",
    "security-incidents",
    "graph-growth-compaction"
  ]);
});

test("lab console tracks quality, memory, security, trust, latency, cost, and graph growth signals", async () => {
  const data = JSON.parse(await readFile(dataPath, "utf8"));
  const metricIds = data.metrics.map((metric) => metric.id);

  for (const requiredMetric of [
    "retrieval-quality",
    "memory-health",
    "contradiction-clusters",
    "stale-knowledge",
    "agent-memory-writes",
    "source-trust",
    "p95-latency",
    "cost-per-context",
    "poisoning-attempts",
    "benchmark-score",
    "eval-regressions"
  ]) {
    assert.ok(metricIds.includes(requiredMetric), `missing metric ${requiredMetric}`);
  }

  assert.ok(data.leaderboard.some((row) => row.name === "Reality Graph full stack"));
  assert.ok(data.securityIncidents.some((incident) => incident.type.includes("poison")));
  assert.ok(data.sourceTrust.some((source) => source.independence < 0.8));
});
