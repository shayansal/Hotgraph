import type {
  AssertionResponse,
  EntityResponse,
  EntityStateResponse,
  EvidencePackResponse,
  GraphQueryRequest,
  HealthResponse,
  IngestDocumentResponse,
  MetricsResponse,
  PathQueryRequest,
  PathResponse,
  QueryResponse,
  SourceResponse
} from "./types";

type JsonBody = Record<string, unknown>;

export class ConsoleApiError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ConsoleApiError";
  }
}

export async function apiGet<T>(path: string): Promise<T> {
  return apiRequest<T>(path);
}

export async function apiPost<T>(path: string, body: JsonBody): Promise<T> {
  return apiRequest<T>(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body)
  });
}

export const rgApi = {
  health: () => apiGet<HealthResponse>("/v1/health"),
  metrics: () => apiGet<MetricsResponse>("/v1/metrics"),
  entity: (id: string) => apiGet<EntityResponse>(`/v1/entities/${encodeURIComponent(id)}`),
  entityState: (id: string, validAt?: string) => {
    const query = validAt ? `?valid_at=${encodeURIComponent(validAt)}` : "";
    return apiGet<EntityStateResponse>(`/v1/entities/${encodeURIComponent(id)}/state${query}`);
  },
  source: (id: string) => apiGet<SourceResponse>(`/v1/sources/${encodeURIComponent(id)}`),
  assertion: (id: string) =>
    apiGet<AssertionResponse>(`/v1/assertions/${encodeURIComponent(id)}`),
  query: (request: GraphQueryRequest) => apiPost<QueryResponse>("/v1/query", request),
  path: (request: PathQueryRequest) => apiPost<PathResponse>("/v1/path", request),
  evidencePack: (request: {
    query: string;
    graph_query: GraphQueryRequest;
    path_query?: PathQueryRequest;
  }) => apiPost<EvidencePackResponse>("/v1/evidence-pack", request),
  ingestDocument: (request: {
    id: string;
    source_id: string;
    uri?: string;
    content: string;
  }) => apiPost<IngestDocumentResponse>("/v1/ingest/document", request)
};

async function apiRequest<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`/api/rg${path}`, {
    ...init,
    cache: "no-store"
  });
  const payload = (await response.json().catch(() => null)) as { error?: string } | null;
  if (!response.ok) {
    throw new ConsoleApiError(payload?.error ?? `Reality Graph API returned ${response.status}`);
  }
  return payload as T;
}

export function formatObject(value: {
  entity_id: string | null;
  text: string | null;
  integer: number | null;
  decimal: number | null;
  boolean: boolean | null;
  time: number | null;
  null: boolean;
}): string {
  if (value.entity_id) return value.entity_id;
  if (value.text) return value.text;
  if (value.integer !== null) return String(value.integer);
  if (value.decimal !== null) return String(value.decimal);
  if (value.boolean !== null) return String(value.boolean);
  if (value.time !== null) return String(value.time);
  if (value.null) return "null";
  return "";
}

export function confidencePercent(value: number): string {
  return `${Math.round(value * 100)}%`;
}
