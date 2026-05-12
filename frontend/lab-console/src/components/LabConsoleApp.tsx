"use client";

import labConsoleData from "@/lib/labData.json";
import type {
  ConsoleView,
  ContradictionCluster,
  EvidenceTraceStep,
  GrowthSeries,
  HeatmapCell,
  HealthStatus,
  LabConsoleData,
  LeaderboardRow,
  MemoryHealth,
  Metric,
  SecurityIncident,
  SourceTrust
} from "@/lib/types";
import { useMemo, useState } from "react";

const data = labConsoleData as LabConsoleData;

export function LabConsoleApp() {
  const [activeView, setActiveView] = useState(data.views[0]?.id ?? "eval-leaderboard");
  const activeViewMeta = useMemo(
    () => data.views.find((view) => view.id === activeView) ?? data.views[0],
    [activeView]
  );

  return (
    <main className="labShell">
      <aside className="labSidebar" aria-label="Lab console sections">
        <a className="brandLockup" href="#overview" aria-label="Reality Graph lab overview">
          <span>RG</span>
          <strong>Lab Console</strong>
        </a>
        <nav className="viewNav">
          {data.views.map((view) => (
            <button
              className="navButton"
              data-active={view.id === activeView}
              key={view.id}
              onClick={() => setActiveView(view.id)}
              type="button"
            >
              <StatusDot status={view.status} />
              <span>{view.label}</span>
            </button>
          ))}
        </nav>
        <div className="policyPlate">
          <span>Policy epoch</span>
          <strong>lab-2026.05.12</strong>
          <small>Signed writes enforced</small>
        </div>
      </aside>

      <section className="labWorkspace">
        <header className="commandHeader" id="overview">
          <div>
            <p className="sectionLabel">Executive Command Console</p>
            <h1>AI memory quality and trust operations</h1>
          </div>
          <div className="controlStrip" aria-label="Console context">
            <span>prod-lab-west</span>
            <span>24h window</span>
            <span>strict trust</span>
          </div>
        </header>

        <section className="metricRail" aria-label="Lab health metrics">
          {data.metrics.map((metric) => (
            <MetricTile key={metric.id} metric={metric} />
          ))}
        </section>

        <section className="activeBrief" aria-live="polite">
          <div>
            <p className="sectionLabel">Active View</p>
            <h2>{activeViewMeta.label}</h2>
            <p>{activeViewMeta.summary}</p>
          </div>
          <StatusBadge status={activeViewMeta.status} />
        </section>

        <section className="dashboardGrid">
          <Leaderboard rows={data.leaderboard} />
          <MemoryHealthPanel rows={data.memoryHealth} />
          <EvidenceTrace rows={data.evidenceTrace} />
          <ContradictionMap rows={data.contradictionClusters} />
          <SourceTrustDashboard rows={data.sourceTrust} />
          <LatencyCostDashboard rows={data.latencyCost} />
          <SecurityIncidents rows={data.securityIncidents} />
          <GraphGrowth rows={data.graphGrowth} />
        </section>
      </section>
    </main>
  );
}

function MetricTile({ metric }: { metric: Metric }) {
  return (
    <article className="metricTile" data-status={metric.status}>
      <div>
        <span>{metric.label}</span>
        <strong>{metric.value}</strong>
      </div>
      <p>{metric.detail}</p>
      <small data-trend={metric.trend}>{trendLabel(metric.trend)}</small>
    </article>
  );
}

function Leaderboard({ rows }: { rows: LeaderboardRow[] }) {
  return (
    <Panel id="eval-leaderboard" label="Eval leaderboard" status="healthy">
      <div className="tableFrame">
        <table>
          <thead>
            <tr>
              <th>System</th>
              <th>Accuracy</th>
              <th>Evidence</th>
              <th>Temporal</th>
              <th>Latency</th>
              <th>Cost</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.name}>
                <td>{row.name}</td>
                <td>
                  <Score value={row.accuracy} />
                </td>
                <td>{formatPercent(row.evidenceRecall)}</td>
                <td>{formatPercent(row.temporalCorrectness)}</td>
                <td>{row.latencyMs} ms</td>
                <td>${row.costUsd.toFixed(3)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Panel>
  );
}

function MemoryHealthPanel({ rows }: { rows: MemoryHealth[] }) {
  return (
    <Panel id="agent-memory-health" label="Agent memory health" status="healthy">
      <div className="rowStack">
        {rows.map((row) => (
          <article className="memoryRow" data-status={row.status} key={row.agent}>
            <div>
              <strong>{row.agent}</strong>
              <span>{row.activeMemories.toLocaleString()} active memories</span>
            </div>
            <dl>
              <div>
                <dt>Writes</dt>
                <dd>{row.writes24h.toLocaleString()}</dd>
              </div>
              <div>
                <dt>Stale</dt>
                <dd>{row.stalePercent}%</dd>
              </div>
              <div>
                <dt>Denied</dt>
                <dd>{row.permissionDenials}</dd>
              </div>
            </dl>
          </article>
        ))}
      </div>
    </Panel>
  );
}

function EvidenceTrace({ rows }: { rows: EvidenceTraceStep[] }) {
  return (
    <Panel id="evidence-trace-explorer" label="Evidence trace explorer" status="healthy">
      <ol className="traceList">
        {rows.map((row) => (
          <li key={row.operator}>
            <div>
              <strong>{row.operator}</strong>
              <span>{row.reason}</span>
            </div>
            <small>
              {row.inputs.toLocaleString()}
              {" -> "}
              {row.outputs.toLocaleString()} | {row.latencyMs} ms
            </small>
          </li>
        ))}
      </ol>
    </Panel>
  );
}

function ContradictionMap({ rows }: { rows: ContradictionCluster[] }) {
  return (
    <Panel id="contradiction-map" label="Contradiction map" status="watch">
      <div className="clusterList">
        {rows.map((row) => (
          <article className="clusterRow" data-status={row.severity} key={row.id}>
            <div>
              <strong>{row.topic}</strong>
              <span>{row.preferredClaim}</span>
            </div>
            <dl>
              <div>
                <dt>Claims</dt>
                <dd>{row.claims}</dd>
              </div>
              <div>
                <dt>Age</dt>
                <dd>{row.openAgeHours}h</dd>
              </div>
            </dl>
          </article>
        ))}
      </div>
    </Panel>
  );
}

function SourceTrustDashboard({ rows }: { rows: SourceTrust[] }) {
  return (
    <Panel id="source-trust-dashboard" label="Source trust dashboard" status="watch">
      <div className="trustMap">
        {rows.map((row) => (
          <article className="trustRow" key={row.source}>
            <header>
              <div>
                <strong>{row.source}</strong>
                <span>{row.issuer}</span>
              </div>
              <small data-signature={row.signature}>{row.signature}</small>
            </header>
            <Meter label="Authority" value={row.authority} />
            <Meter label="Reputation" value={row.reputation} />
            <Meter label="Independence" value={row.independence} />
            <small>Conflict rate {formatPercent(row.conflictRate)}</small>
          </article>
        ))}
      </div>
    </Panel>
  );
}

function LatencyCostDashboard({ rows }: { rows: HeatmapCell[] }) {
  return (
    <Panel id="latency-cost-dashboard" label="Latency/cost dashboard" status="healthy">
      <div className="heatmapGrid">
        {rows.map((row) => (
          <article className="heatCell" data-status={row.status} key={row.route}>
            <strong>{row.route}</strong>
            <span>{row.p95Ms} ms</span>
            <small>${row.costUsd.toFixed(3)} per pack</small>
          </article>
        ))}
      </div>
    </Panel>
  );
}

function SecurityIncidents({ rows }: { rows: SecurityIncident[] }) {
  return (
    <Panel id="security-incidents" label="Security incidents" status="critical">
      <div className="incidentList">
        {rows.map((row) => (
          <article className="incidentRow" data-status={row.status} key={row.id}>
            <strong>{row.type}</strong>
            <span>{row.source}</span>
            <small>
              {row.id} | {row.blockedAt}
            </small>
          </article>
        ))}
      </div>
    </Panel>
  );
}

function GraphGrowth({ rows }: { rows: GrowthSeries[] }) {
  return (
    <Panel id="graph-growth-compaction" label="Graph growth and compaction" status="healthy">
      <div className="growthGrid">
        {rows.map((row) => (
          <article className="growthRow" data-status={row.status} key={row.label}>
            <strong>{row.label}</strong>
            <dl>
              <div>
                <dt>Events</dt>
                <dd>{row.events}</dd>
              </div>
              <div>
                <dt>Assertions</dt>
                <dd>{row.assertions}</dd>
              </div>
              <div>
                <dt>Snapshot</dt>
                <dd>{row.snapshotSize}</dd>
              </div>
              <div>
                <dt>Lag</dt>
                <dd>{row.compactionLag}</dd>
              </div>
            </dl>
          </article>
        ))}
      </div>
    </Panel>
  );
}

function Panel({
  children,
  id,
  label,
  status
}: {
  children: React.ReactNode;
  id: ConsoleView["id"];
  label: string;
  status: HealthStatus;
}) {
  return (
    <section className="consolePanel" id={id}>
      <header className="panelHeader">
        <div>
          <p className="sectionLabel">{label}</p>
          <h2>{panelTitle(id)}</h2>
        </div>
        <StatusBadge status={status} />
      </header>
      {children}
    </section>
  );
}

function StatusBadge({ status }: { status: HealthStatus }) {
  return <span className="statusBadge" data-status={status}>{status}</span>;
}

function StatusDot({ status }: { status: HealthStatus }) {
  return <span className="statusDot" data-status={status} aria-hidden="true" />;
}

function Score({ value }: { value: number }) {
  return (
    <span className="scoreMeter">
      <span style={{ inlineSize: `${Math.round(value * 100)}%` }} />
      <strong>{formatPercent(value)}</strong>
    </span>
  );
}

function Meter({ label, value }: { label: string; value: number }) {
  return (
    <div className="meterLine">
      <span>{label}</span>
      <div aria-hidden="true">
        <span style={{ inlineSize: `${Math.round(value * 100)}%` }} />
      </div>
      <strong>{value.toFixed(2)}</strong>
    </div>
  );
}

function formatPercent(value: number) {
  return `${Math.round(value * 1000) / 10}%`;
}

function trendLabel(trend: Metric["trend"]) {
  if (trend === "up") {
    return "rising";
  }
  if (trend === "down") {
    return "falling";
  }
  return "steady";
}

function panelTitle(id: string) {
  const titles: Record<string, string> = {
    "eval-leaderboard": "Frontier benchmark posture",
    "agent-memory-health": "Memory lifecycle by agent",
    "evidence-trace-explorer": "Context-pack operator trace",
    "contradiction-map": "Unresolved belief conflicts",
    "source-trust-dashboard": "Authority and independence map",
    "latency-cost-dashboard": "Route heatmap",
    "security-incidents": "Blocked adversarial activity",
    "graph-growth-compaction": "Storage and replay posture"
  };
  return titles[id] ?? "Console view";
}
