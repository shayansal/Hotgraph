export type EntityResponse = {
  id: string;
  entity_type: string;
  canonical_name: string | null;
  created_tx: number;
};

export type SourceResponse = {
  id: string;
  source_type: string;
  uri: string | null;
  content_hash: string;
  observed_at: number;
  trust_score: number | null;
};

export type GraphValueResponse = {
  entity_id: string | null;
  text: string | null;
  integer: number | null;
  decimal: number | null;
  boolean: boolean | null;
  time: number | null;
  null: boolean;
};

export type AssertionResponse = {
  assertion_id: string;
  subject: string;
  predicate: string;
  object: GraphValueResponse;
  valid_from: number;
  valid_to: number | null;
  tx_from: number;
  tx_to: number | null;
  confidence: number;
  sources: string[];
  context: string;
  status: string;
};

export type QueryResultResponse = Omit<AssertionResponse, "status">;

export type QueryResponse = {
  results: QueryResultResponse[];
};

export type PathResultResponse = {
  start: string;
  end: string;
  hops: QueryResultResponse[];
};

export type PathResponse = {
  paths: PathResultResponse[];
};

export type EntityStateResponse = {
  entity: EntityResponse;
  assertions: AssertionResponse[];
};

export type EvidencePackResponse = {
  query: string;
  entities: EntityResponse[];
  assertions: AssertionResponse[];
  sources: SourceExcerptResponse[];
  paths: PathResultResponse[];
  contradictions: ContradictionResponse[];
  generated_at: number;
};

export type SourceExcerptResponse = {
  source_id: string;
  source_type: string;
  uri: string | null;
  content_hash: string;
  snippet: string;
  trust_score: number | null;
};

export type ContradictionResponse = {
  id: string;
  assertion_a: string;
  assertion_b: string;
  contradiction_type: string;
  severity: string;
  explanation: string;
};

export type CandidateAssertionResponse = {
  subject_text: string;
  predicate_text: string;
  object_text: string;
  valid_from: number | null;
  valid_to: number | null;
  confidence: number;
  source_id: string;
  source_excerpt: string;
  extraction_model: string;
};

export type IngestDocumentResponse = {
  document_id: string;
  candidates: CandidateAssertionResponse[];
};

export type MetricsResponse = {
  entities: number;
  assertions: number;
  sources: number;
  events: number;
};

export type HealthResponse = {
  status: string;
};

export type GraphValueRequest =
  | { entity_id: string }
  | { text: string }
  | { integer: number }
  | { decimal: number }
  | { boolean: boolean };

export type GraphQueryRequest = {
  subject?: { entity_id: string };
  predicate?: string;
  object?: GraphValueRequest;
  valid_at?: string;
  known_at?: string;
  context?: string;
  min_confidence?: number;
  limit?: number;
  include_sources?: boolean;
};

export type PathQueryRequest = {
  start: string;
  end?: string;
  predicates: string[];
  valid_at?: string;
  max_depth: number;
  min_confidence?: number;
};
