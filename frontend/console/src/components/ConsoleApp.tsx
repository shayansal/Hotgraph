"use client";

import Link from "next/link";
import { FormEvent, useEffect, useMemo, useState } from "react";
import { confidencePercent, formatObject, rgApi } from "@/lib/api";
import type {
  CandidateAssertionResponse,
  EntityResponse,
  EvidencePackResponse,
  MetricsResponse,
  PathResponse,
  QueryResponse,
  SourceResponse
} from "@/lib/types";

type Notice = { tone: "error" | "ok"; text: string } | null;

export function ConsoleApp() {
  const [notice, setNotice] = useState<Notice>(null);
  const [health, setHealth] = useState("checking");
  const [metrics, setMetrics] = useState<MetricsResponse | null>(null);
  const [entityId, setEntityId] = useState("person-a");
  const [validAt, setValidAt] = useState("2024-01-01");
  const [entity, setEntity] = useState<EntityResponse | null>(null);
  const [entityState, setEntityState] = useState<QueryResponse["results"]>([]);
  const [sourceId, setSourceId] = useState("source-employment");
  const [source, setSource] = useState<SourceResponse | null>(null);
  const [querySubject, setQuerySubject] = useState("person-a");
  const [queryPredicate, setQueryPredicate] = useState("WORKED_AT");
  const [minConfidence, setMinConfidence] = useState("0.8");
  const [queryResults, setQueryResults] = useState<QueryResponse["results"]>([]);
  const [pathStart, setPathStart] = useState("person-a");
  const [pathEnd, setPathEnd] = useState("city-c");
  const [pathPredicates, setPathPredicates] = useState("WORKED_AT, LOCATED_IN");
  const [paths, setPaths] = useState<PathResponse["paths"]>([]);
  const [evidencePack, setEvidencePack] = useState<EvidencePackResponse | null>(null);
  const [documentText, setDocumentText] = useState(
    "candidate: Person A | worked_at | Company B | valid=2021..2025 | confidence=0.92 | evidence=Person A worked at Company B."
  );
  const [candidates, setCandidates] = useState<CandidateAssertionResponse[]>([]);

  const timelineRows = useMemo(
    () => [...entityState].sort((left, right) => left.valid_from - right.valid_from),
    [entityState]
  );

  useEffect(() => {
    void refreshStatus();
  }, []);

  async function refreshStatus() {
    try {
      const [healthResponse, metricsResponse] = await Promise.all([rgApi.health(), rgApi.metrics()]);
      setHealth(healthResponse.status);
      setMetrics(metricsResponse);
    } catch {
      setHealth("offline");
      setMetrics(null);
    }
  }

  async function loadEntity(event?: FormEvent) {
    event?.preventDefault();
    try {
      const [entityResponse, stateResponse] = await Promise.all([
        rgApi.entity(entityId),
        rgApi.entityState(entityId, validAt)
      ]);
      setEntity(entityResponse);
      setEntityState(stateResponse.assertions);
      setNotice({ tone: "ok", text: `Loaded ${entityResponse.id}` });
    } catch (error) {
      setNotice({ tone: "error", text: readableError(error) });
    }
  }

  async function loadSource(event?: FormEvent) {
    event?.preventDefault();
    try {
      const sourceResponse = await rgApi.source(sourceId);
      setSource(sourceResponse);
      setNotice({ tone: "ok", text: `Loaded ${sourceResponse.id}` });
    } catch (error) {
      setNotice({ tone: "error", text: readableError(error) });
    }
  }

  async function runQuery(event?: FormEvent) {
    event?.preventDefault();
    try {
      const response = await rgApi.query({
        subject: querySubject ? { entity_id: querySubject } : undefined,
        predicate: queryPredicate || undefined,
        valid_at: validAt,
        min_confidence: Number(minConfidence),
        include_sources: true,
        limit: 50
      });
      setQueryResults(response.results);
      setNotice({ tone: "ok", text: `${response.results.length} query results` });
    } catch (error) {
      setNotice({ tone: "error", text: readableError(error) });
    }
  }

  async function runPath(event?: FormEvent) {
    event?.preventDefault();
    try {
      const response = await rgApi.path({
        start: pathStart,
        end: pathEnd || undefined,
        predicates: splitPredicates(pathPredicates),
        valid_at: validAt,
        max_depth: 4,
        min_confidence: Number(minConfidence)
      });
      setPaths(response.paths);
      setNotice({ tone: "ok", text: `${response.paths.length} paths` });
    } catch (error) {
      setNotice({ tone: "error", text: readableError(error) });
    }
  }

  async function buildEvidencePack() {
    try {
      const response = await rgApi.evidencePack({
        query: `Evidence for ${querySubject} ${queryPredicate}`,
        graph_query: {
          subject: querySubject ? { entity_id: querySubject } : undefined,
          predicate: queryPredicate || undefined,
          valid_at: validAt,
          min_confidence: Number(minConfidence)
        },
        path_query:
          pathStart && pathEnd
            ? {
                start: pathStart,
                end: pathEnd,
                predicates: splitPredicates(pathPredicates),
                valid_at: validAt,
                max_depth: 4,
                min_confidence: Number(minConfidence)
              }
            : undefined
      });
      setEvidencePack(response);
      setNotice({ tone: "ok", text: "Evidence pack generated" });
    } catch (error) {
      setNotice({ tone: "error", text: readableError(error) });
    }
  }

  async function ingestDocument(event?: FormEvent) {
    event?.preventDefault();
    try {
      const response = await rgApi.ingestDocument({
        id: `doc-${Date.now()}`,
        source_id: sourceId,
        uri: source?.uri ?? undefined,
        content: documentText
      });
      setCandidates(response.candidates);
      setNotice({ tone: "ok", text: `${response.candidates.length} candidate assertions` });
    } catch (error) {
      setNotice({ tone: "error", text: readableError(error) });
    }
  }

  return (
    <main className="consoleShell">
      <aside className="sidebar">
        <div>
          <div className="brandMark">RG</div>
          <h1>Reality Graph</h1>
        </div>
        <nav>
          <a href="#entities">Entities</a>
          <a href="#timeline">Timeline</a>
          <a href="#sources">Sources</a>
          <a href="#paths">Paths</a>
          <a href="#ingestion">Ingestion</a>
          <a href="#query">Query</a>
        </nav>
        <div className="serverPill" data-status={health}>
          <span>{health}</span>
        </div>
      </aside>

      <section className="workspace">
        <header className="workspaceHeader">
          <div>
            <p className="label">Admin Console</p>
            <h2>4D knowledge graph operations</h2>
          </div>
          <button onClick={refreshStatus}>Refresh</button>
        </header>

        {notice && <div className={`notice ${notice.tone}`}>{notice.text}</div>}

        <section className="metricsGrid" aria-label="Metrics">
          <Metric label="Entities" value={metrics?.entities ?? 0} />
          <Metric label="Assertions" value={metrics?.assertions ?? 0} />
          <Metric label="Sources" value={metrics?.sources ?? 0} />
          <Metric label="Events" value={metrics?.events ?? 0} />
        </section>

        <section className="splitGrid">
          <Panel id="entities" eyebrow="Entity Browser" title="Lookup">
            <form className="controlRow" onSubmit={loadEntity}>
              <input value={entityId} onChange={(event) => setEntityId(event.target.value)} />
              <input value={validAt} onChange={(event) => setValidAt(event.target.value)} />
              <button type="submit">Load</button>
            </form>
            {entity && (
              <div className="entitySummary">
                <div>
                  <strong>{entity.canonical_name ?? entity.id}</strong>
                  <p>{entity.id}</p>
                </div>
                <span>{entity.entity_type}</span>
                <Link href={`/entities/${encodeURIComponent(entity.id)}`}>Detail</Link>
              </div>
            )}
          </Panel>

          <Panel id="sources" eyebrow="Source Viewer" title="Evidence">
            <form className="controlRow" onSubmit={loadSource}>
              <input value={sourceId} onChange={(event) => setSourceId(event.target.value)} />
              <button type="submit">Open</button>
            </form>
            {source && (
              <dl className="keyValue">
                <dt>Type</dt>
                <dd>{source.source_type}</dd>
                <dt>URI</dt>
                <dd>{source.uri ?? "none"}</dd>
                <dt>Hash</dt>
                <dd>{source.content_hash}</dd>
                <dt>Trust</dt>
                <dd>{source.trust_score ?? "none"}</dd>
              </dl>
            )}
          </Panel>
        </section>

        <section className="panel" id="timeline">
          <div className="panelHeader">
            <div>
              <p className="label">Assertion Timeline</p>
              <h3>{entity?.id ?? entityId}</h3>
            </div>
            <span className="count">{timelineRows.length}</span>
          </div>
          <div className="timeline">
            {timelineRows.map((assertion) => (
              <article className="timelineRow" key={assertion.assertion_id}>
                <div className="timeRail">
                  <span>{assertion.valid_from}</span>
                  <span>{assertion.valid_to ?? "open"}</span>
                </div>
                <div>
                  <strong>{assertion.predicate}</strong>
                  <p>
                    {assertion.subject}
                    {" -> "}
                    {formatObject(assertion.object)}
                  </p>
                  <small>
                    {confidencePercent(assertion.confidence)} | {assertion.sources.join(", ")}
                  </small>
                </div>
              </article>
            ))}
            {timelineRows.length === 0 && <p className="emptyState">No assertions loaded.</p>}
          </div>
        </section>

        <section className="splitGrid">
          <Panel id="query" eyebrow="Query Workbench" title="Point-in-time">
            <form className="stackForm" onSubmit={runQuery}>
              <label>
                Subject
                <input value={querySubject} onChange={(event) => setQuerySubject(event.target.value)} />
              </label>
              <label>
                Predicate
                <input
                  value={queryPredicate}
                  onChange={(event) => setQueryPredicate(event.target.value)}
                />
              </label>
              <label>
                Min confidence
                <input
                  value={minConfidence}
                  onChange={(event) => setMinConfidence(event.target.value)}
                />
              </label>
              <button type="submit">Run Query</button>
            </form>
          </Panel>

          <Panel id="paths" eyebrow="Graph Path Explorer" title="Traversal">
            <form className="stackForm" onSubmit={runPath}>
              <label>
                Start
                <input value={pathStart} onChange={(event) => setPathStart(event.target.value)} />
              </label>
              <label>
                End
                <input value={pathEnd} onChange={(event) => setPathEnd(event.target.value)} />
              </label>
              <label>
                Predicates
                <input
                  value={pathPredicates}
                  onChange={(event) => setPathPredicates(event.target.value)}
                />
              </label>
              <button type="submit">Find Paths</button>
            </form>
          </Panel>
        </section>

        <section className="panel">
          <div className="panelHeader">
            <div>
              <p className="label">Results</p>
              <h3>Assertions and paths</h3>
            </div>
            <button onClick={buildEvidencePack}>Evidence Pack</button>
          </div>
          <div className="resultsGrid">
            <ResultTable results={queryResults} />
            <PathList paths={paths} />
          </div>
        </section>

        <section className="splitGrid">
          <Panel id="ingestion" eyebrow="Ingestion Review Queue" title="Candidates">
            <form className="stackForm" onSubmit={ingestDocument}>
              <textarea value={documentText} onChange={(event) => setDocumentText(event.target.value)} />
              <button type="submit">Extract</button>
            </form>
            <div className="candidateList">
              {candidates.map((candidate, index) => (
                <article className="candidateRow" key={`${candidate.subject_text}-${index}`}>
                  <strong>
                    {candidate.subject_text} | {candidate.predicate_text} | {candidate.object_text}
                  </strong>
                  <small>
                    {confidencePercent(candidate.confidence)} | {candidate.source_id}
                  </small>
                </article>
              ))}
            </div>
          </Panel>

          <Panel id="contradictions" eyebrow="Contradiction Dashboard" title="Evidence pack">
            {evidencePack ? (
              <div className="packSummary">
                <dl className="keyValue">
                  <dt>Assertions</dt>
                  <dd>{evidencePack.assertions.length}</dd>
                  <dt>Sources</dt>
                  <dd>{evidencePack.sources.length}</dd>
                  <dt>Paths</dt>
                  <dd>{evidencePack.paths.length}</dd>
                </dl>
                <div className="candidateList">
                  {evidencePack.contradictions.map((contradiction) => (
                    <article className="candidateRow warn" key={contradiction.id}>
                      <strong>{contradiction.contradiction_type}</strong>
                      <small>
                        {contradiction.assertion_a} | {contradiction.assertion_b} | {contradiction.severity}
                      </small>
                    </article>
                  ))}
                  {evidencePack.contradictions.length === 0 && (
                    <p className="emptyState">No contradictions returned.</p>
                  )}
                </div>
              </div>
            ) : (
              <p className="emptyState">No evidence pack loaded.</p>
            )}
          </Panel>
        </section>
      </section>
    </main>
  );
}

function Metric({ label, value }: { label: string; value: number }) {
  return (
    <article className="metric">
      <span>{label}</span>
      <strong>{value}</strong>
    </article>
  );
}

function Panel({
  id,
  eyebrow,
  title,
  children
}: {
  id: string;
  eyebrow: string;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section className="panel" id={id}>
      <div className="panelHeader">
        <div>
          <p className="label">{eyebrow}</p>
          <h3>{title}</h3>
        </div>
      </div>
      {children}
    </section>
  );
}

function ResultTable({ results }: { results: QueryResponse["results"] }) {
  return (
    <div className="tableWrap">
      <table>
        <thead>
          <tr>
            <th>Assertion</th>
            <th>Predicate</th>
            <th>Object</th>
            <th>Confidence</th>
          </tr>
        </thead>
        <tbody>
          {results.map((result) => (
            <tr key={result.assertion_id}>
              <td>{result.assertion_id}</td>
              <td>{result.predicate}</td>
              <td>{formatObject(result.object)}</td>
              <td>{confidencePercent(result.confidence)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {results.length === 0 && <p className="emptyState">No query results.</p>}
    </div>
  );
}

function PathList({ paths }: { paths: PathResponse["paths"] }) {
  return (
    <div className="pathList">
      {paths.map((path, index) => (
        <article className="pathRow" key={`${path.start}-${path.end}-${index}`}>
          <strong>
            {path.start}
            {" -> "}
            {path.end}
          </strong>
          <div>
            {path.hops.map((hop) => (
              <span key={hop.assertion_id}>{hop.assertion_id}</span>
            ))}
          </div>
        </article>
      ))}
      {paths.length === 0 && <p className="emptyState">No paths loaded.</p>}
    </div>
  );
}

function splitPredicates(value: string): string[] {
  return value
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

function readableError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
