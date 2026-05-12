export type Trend = "up" | "down" | "flat";

export type HealthStatus = "healthy" | "watch" | "critical";

export type Metric = {
  id: string;
  label: string;
  value: string;
  detail: string;
  trend: Trend;
  status: HealthStatus;
};

export type ConsoleView = {
  id: string;
  label: string;
  summary: string;
  status: HealthStatus;
};

export type LeaderboardRow = {
  name: string;
  accuracy: number;
  evidenceRecall: number;
  temporalCorrectness: number;
  latencyMs: number;
  costUsd: number;
};

export type MemoryHealth = {
  agent: string;
  activeMemories: number;
  writes24h: number;
  stalePercent: number;
  supersededPercent: number;
  permissionDenials: number;
  status: HealthStatus;
};

export type EvidenceTraceStep = {
  operator: string;
  reason: string;
  inputs: number;
  outputs: number;
  latencyMs: number;
};

export type ContradictionCluster = {
  id: string;
  topic: string;
  claims: number;
  severity: HealthStatus;
  openAgeHours: number;
  preferredClaim: string;
};

export type SourceTrust = {
  source: string;
  issuer: string;
  authority: number;
  reputation: number;
  independence: number;
  conflictRate: number;
  signature: "verified" | "missing" | "failed";
};

export type HeatmapCell = {
  route: string;
  p95Ms: number;
  costUsd: number;
  status: HealthStatus;
};

export type SecurityIncident = {
  id: string;
  type: string;
  source: string;
  blockedAt: string;
  status: HealthStatus;
};

export type GrowthSeries = {
  label: string;
  events: string;
  assertions: string;
  snapshotSize: string;
  compactionLag: string;
  status: HealthStatus;
};

export type LabConsoleData = {
  metrics: Metric[];
  views: ConsoleView[];
  leaderboard: LeaderboardRow[];
  memoryHealth: MemoryHealth[];
  evidenceTrace: EvidenceTraceStep[];
  contradictionClusters: ContradictionCluster[];
  sourceTrust: SourceTrust[];
  latencyCost: HeatmapCell[];
  securityIncidents: SecurityIncident[];
  graphGrowth: GrowthSeries[];
};
