"use client";

import Link from "next/link";
import { useEffect, useState } from "react";
import { confidencePercent, formatObject, rgApi } from "@/lib/api";
import type { EntityStateResponse } from "@/lib/types";

type LoadState =
  | { status: "idle" | "loading"; data?: never; error?: never }
  | { status: "ready"; data: EntityStateResponse; error?: never }
  | { status: "error"; data?: never; error: string };

export function EntityDetail({ entityId }: { entityId: string }) {
  const [validAt, setValidAt] = useState("2024-01-01");
  const [state, setState] = useState<LoadState>({ status: "idle" });

  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [entityId]);

  async function load() {
    setState({ status: "loading" });
    try {
      const data = await rgApi.entityState(entityId, validAt);
      setState({ status: "ready", data });
    } catch (error) {
      setState({ status: "error", error: error instanceof Error ? error.message : String(error) });
    }
  }

  return (
    <main className="page">
      <header className="topbar">
        <Link className="brand" href="/">
          Reality Graph
        </Link>
        <div className="topbarStatus">Entity detail</div>
      </header>

      <section className="detailHero">
        <div>
          <p className="label">Entity</p>
          <h1>{state.status === "ready" ? state.data.entity.canonical_name ?? entityId : entityId}</h1>
        </div>
        <div className="toolbar">
          <input value={validAt} onChange={(event) => setValidAt(event.target.value)} />
          <button onClick={load}>Load State</button>
        </div>
      </section>

      {state.status === "error" && <p className="errorText">{state.error}</p>}

      {state.status === "ready" && (
        <section className="panel">
          <div className="panelHeader">
            <div>
              <p className="label">Assertion Timeline</p>
              <h2>{state.data.entity.id}</h2>
            </div>
            <span className="count">{state.data.assertions.length}</span>
          </div>
          <div className="timeline">
            {state.data.assertions.map((assertion) => (
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
                    {confidencePercent(assertion.confidence)} confidence | {assertion.sources.join(", ")}
                  </small>
                </div>
              </article>
            ))}
          </div>
        </section>
      )}
    </main>
  );
}
