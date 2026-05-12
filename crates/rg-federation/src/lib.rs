//! Federated Reality Graph query planning and evidence merging.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rg_core::{AssertionId, EntityId, SourceId};

macro_rules! string_newtype {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

string_newtype!(GraphNodeId);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FederatedGraphNodeKind {
    LocalGraph,
    TeamGraph,
    EnterpriseGraph,
    LabGraph,
    ExternalPublicGraph,
    PartnerGraph,
    PersonalGraph,
}

impl FederatedGraphNodeKind {
    pub fn all() -> Vec<Self> {
        vec![
            Self::LocalGraph,
            Self::TeamGraph,
            Self::EnterpriseGraph,
            Self::LabGraph,
            Self::ExternalPublicGraph,
            Self::PartnerGraph,
            Self::PersonalGraph,
        ]
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum TrustBoundary {
    Local,
    Team,
    Enterprise,
    Lab,
    ExternalPublicGraph,
    Partner,
    Personal,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BoundarySensitivity {
    Public,
    Internal,
    Restricted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceBoundaryLabel {
    pub graph_id: GraphNodeId,
    pub node_kind: FederatedGraphNodeKind,
    pub boundary: TrustBoundary,
}

impl SourceBoundaryLabel {
    pub fn unknown(graph_id: GraphNodeId) -> Self {
        Self {
            graph_id,
            node_kind: FederatedGraphNodeKind::ExternalPublicGraph,
            boundary: TrustBoundary::ExternalPublicGraph,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FederatedEvidenceItem {
    pub id: String,
    pub entity_id: EntityId,
    pub assertion_id: AssertionId,
    pub source_id: SourceId,
    pub text: String,
    pub confidence: f32,
    pub weighted_score: f32,
    pub boundary: SourceBoundaryLabel,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FederatedGraphNode {
    pub id: GraphNodeId,
    pub kind: FederatedGraphNodeKind,
    pub boundary: TrustBoundary,
    pub trust_score: f32,
    allowed_principals: BTreeSet<String>,
    public_read: bool,
    results: Vec<FederatedEvidenceItem>,
    attestation: Option<RemoteAttestation>,
}

impl FederatedGraphNode {
    pub fn new(
        id: impl Into<String>,
        kind: FederatedGraphNodeKind,
        boundary: TrustBoundary,
        trust_score: f32,
    ) -> Self {
        Self {
            id: GraphNodeId::new(id),
            kind,
            boundary,
            trust_score: trust_score.clamp(0.0, 1.0),
            allowed_principals: BTreeSet::new(),
            public_read: false,
            results: Vec::new(),
            attestation: None,
        }
    }

    pub fn allow_principal(mut self, principal: impl Into<String>) -> Self {
        self.allowed_principals.insert(principal.into());
        self
    }

    pub fn allow_public_read(mut self) -> Self {
        self.public_read = true;
        self
    }

    pub fn with_result(mut self, result: FederatedEvidenceItem) -> Self {
        self.results.push(result);
        self
    }

    pub fn with_attestation(mut self, attestation: RemoteAttestation) -> Self {
        self.attestation = Some(attestation);
        self
    }

    fn can_read(&self, principal: &str) -> bool {
        self.public_read || self.allowed_principals.contains(principal)
    }

    fn boundary_label(&self) -> SourceBoundaryLabel {
        SourceBoundaryLabel {
            graph_id: self.id.clone(),
            node_kind: self.kind,
            boundary: self.boundary,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FederationQuery {
    pub principal_id: String,
    pub query: String,
    pub entity_filters: Vec<EntityId>,
}

impl FederationQuery {
    pub fn new(
        principal_id: impl Into<String>,
        query: impl Into<String>,
        entity_filters: Vec<EntityId>,
    ) -> Self {
        Self {
            principal_id: principal_id.into(),
            query: query.into(),
            entity_filters,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkippedGraph {
    pub graph_id: GraphNodeId,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteQueryPlan {
    pub query: FederationQuery,
    pub target_graph_ids: Vec<GraphNodeId>,
    pub skipped_graphs: Vec<SkippedGraph>,
    pub fanout_parallel: bool,
    pub steps: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PartialGraphResult {
    pub node_id: GraphNodeId,
    pub boundary: SourceBoundaryLabel,
    pub trust_score: f32,
    pub evidence: Vec<FederatedEvidenceItem>,
    pub complete: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FederatedEvidencePack {
    pub query: String,
    pub partial_results: Vec<PartialGraphResult>,
    pub merged_evidence: Vec<FederatedEvidenceItem>,
    pub source_boundaries: Vec<SourceBoundaryLabel>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FederatedGraph {
    nodes: BTreeMap<GraphNodeId, FederatedGraphNode>,
}

impl FederatedGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, node: FederatedGraphNode) {
        self.nodes.insert(node.id.clone(), node);
    }

    pub fn node(&self, node_id: &GraphNodeId) -> Option<&FederatedGraphNode> {
        self.nodes.get(node_id)
    }

    pub fn plan_query(&self, query: FederationQuery) -> RemoteQueryPlan {
        let mut target_graph_ids = Vec::new();
        let mut skipped_graphs = Vec::new();
        for node in self.nodes.values() {
            if node.can_read(&query.principal_id) {
                target_graph_ids.push(node.id.clone());
            } else {
                skipped_graphs.push(SkippedGraph {
                    graph_id: node.id.clone(),
                    reason: format!(
                        "permission denied for principal {} at {:?} boundary",
                        query.principal_id, node.boundary
                    ),
                });
            }
        }
        target_graph_ids.sort_by_key(|graph_id| {
            self.nodes
                .get(graph_id)
                .map(|node| node_kind_rank(node.kind))
                .unwrap_or(usize::MAX)
        });
        skipped_graphs.sort_by(|left, right| left.graph_id.cmp(&right.graph_id));
        RemoteQueryPlan {
            query,
            target_graph_ids,
            skipped_graphs,
            fanout_parallel: true,
            steps: vec![
                "fan out query to permitted graph nodes".to_owned(),
                "attach source boundary labels to every remote result".to_owned(),
                "merge partial results by graph trust and item confidence".to_owned(),
                "preserve warnings for skipped or incomplete graphs".to_owned(),
            ],
        }
    }

    pub fn execute_plan(&self, plan: &RemoteQueryPlan) -> FederatedEvidencePack {
        let mut partial_results = Vec::new();
        let mut merged_evidence = Vec::new();
        let mut source_boundaries = Vec::new();
        let mut warnings = plan
            .skipped_graphs
            .iter()
            .map(|skipped| format!("graph {} skipped: {}", skipped.graph_id, skipped.reason))
            .collect::<Vec<_>>();

        for graph_id in &plan.target_graph_ids {
            let Some(node) = self.nodes.get(graph_id) else {
                warnings.push(format!("graph {graph_id} missing during execution"));
                continue;
            };
            let boundary = node.boundary_label();
            source_boundaries.push(boundary.clone());
            let mut evidence = node
                .results
                .iter()
                .cloned()
                .map(|mut item| {
                    item.boundary = boundary.clone();
                    item.weighted_score = (item.confidence * node.trust_score).clamp(0.0, 1.0);
                    item
                })
                .collect::<Vec<_>>();
            evidence.sort_by(|left, right| {
                right
                    .weighted_score
                    .total_cmp(&left.weighted_score)
                    .then_with(|| left.id.cmp(&right.id))
            });
            merged_evidence.extend(evidence.clone());
            partial_results.push(PartialGraphResult {
                node_id: node.id.clone(),
                boundary,
                trust_score: node.trust_score,
                complete: true,
                evidence,
            });
        }

        partial_results.sort_by(|left, right| left.node_id.cmp(&right.node_id));
        source_boundaries.sort_by(|left, right| left.graph_id.cmp(&right.graph_id));
        source_boundaries.dedup_by(|left, right| left.graph_id == right.graph_id);
        merged_evidence.sort_by(|left, right| {
            right
                .weighted_score
                .total_cmp(&left.weighted_score)
                .then_with(|| left.boundary.graph_id.cmp(&right.boundary.graph_id))
                .then_with(|| left.id.cmp(&right.id))
        });

        FederatedEvidencePack {
            query: plan.query.query.clone(),
            partial_results,
            merged_evidence,
            source_boundaries,
            warnings,
        }
    }

    pub fn evaluate_join(
        &self,
        join: &PermissionedGraphJoin,
        principal_id: &str,
    ) -> Result<GraphJoinDecision, FederationError> {
        let left = self
            .nodes
            .get(&join.left_graph_id)
            .ok_or_else(|| FederationError::UnknownGraph(join.left_graph_id.clone()))?;
        let right = self
            .nodes
            .get(&join.right_graph_id)
            .ok_or_else(|| FederationError::UnknownGraph(join.right_graph_id.clone()))?;

        if (!left.can_read(principal_id) || !right.can_read(principal_id))
            && join.explicit_policy_id.is_none()
        {
            return Ok(GraphJoinDecision {
                allowed: false,
                reason: format!(
                    "join denied because principal {principal_id} cannot read both graph boundaries"
                ),
                left_boundary: left.boundary_label(),
                right_boundary: right.boundary_label(),
            });
        }

        let restricted_cross_boundary = join.required_sensitivity
            >= BoundarySensitivity::Restricted
            && left.boundary != right.boundary;
        if restricted_cross_boundary && join.explicit_policy_id.is_none() {
            return Ok(GraphJoinDecision {
                allowed: false,
                reason:
                    "join denied because restricted cross-boundary joins require explicit policy"
                        .to_owned(),
                left_boundary: left.boundary_label(),
                right_boundary: right.boundary_label(),
            });
        }

        Ok(GraphJoinDecision {
            allowed: true,
            reason: join.explicit_policy_id.as_ref().map_or_else(
                || "join allowed by read permissions".to_owned(),
                |policy| format!("join allowed by explicit policy {policy}"),
            ),
            left_boundary: left.boundary_label(),
            right_boundary: right.boundary_label(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FederationError {
    UnknownGraph(GraphNodeId),
}

impl fmt::Display for FederationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownGraph(graph_id) => write!(formatter, "unknown graph {graph_id}"),
        }
    }
}

impl std::error::Error for FederationError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntityResolutionMember {
    pub graph_id: GraphNodeId,
    pub local_entity_id: EntityId,
    pub evidence: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntityResolutionCluster {
    pub canonical_entity_id: EntityId,
    pub members: Vec<EntityResolutionMember>,
    pub explanation: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CrossGraphEntityResolution {
    aliases: BTreeMap<EntityId, EntityId>,
    members_by_canonical: BTreeMap<EntityId, Vec<EntityResolutionMember>>,
}

impl CrossGraphEntityResolution {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn link(
        &mut self,
        local_entity_id: EntityId,
        graph_id: GraphNodeId,
        canonical_entity_id: EntityId,
        evidence: impl Into<String>,
    ) {
        self.aliases
            .insert(local_entity_id.clone(), canonical_entity_id.clone());
        let members = self
            .members_by_canonical
            .entry(canonical_entity_id)
            .or_default();
        members.push(EntityResolutionMember {
            graph_id,
            local_entity_id,
            evidence: evidence.into(),
        });
        members.sort_by(|left, right| {
            left.graph_id
                .cmp(&right.graph_id)
                .then_with(|| left.local_entity_id.cmp(&right.local_entity_id))
        });
        members.dedup_by(|left, right| {
            left.graph_id == right.graph_id && left.local_entity_id == right.local_entity_id
        });
    }

    pub fn resolve(&self, entity_id: &EntityId) -> Option<EntityResolutionCluster> {
        let canonical = self.aliases.get(entity_id).unwrap_or(entity_id);
        let members = self.members_by_canonical.get(canonical)?.clone();
        Some(EntityResolutionCluster {
            canonical_entity_id: canonical.clone(),
            members,
            explanation:
                "cross-graph resolution preserves graph-local entity IDs under a canonical identity"
                    .to_owned(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PermissionedGraphJoin {
    pub left_graph_id: GraphNodeId,
    pub right_graph_id: GraphNodeId,
    pub entity_id: EntityId,
    pub required_sensitivity: BoundarySensitivity,
    pub explicit_policy_id: Option<String>,
}

impl PermissionedGraphJoin {
    pub fn new(
        left_graph_id: GraphNodeId,
        right_graph_id: GraphNodeId,
        entity_id: EntityId,
    ) -> Self {
        Self {
            left_graph_id,
            right_graph_id,
            entity_id,
            required_sensitivity: BoundarySensitivity::Internal,
            explicit_policy_id: None,
        }
    }

    pub fn with_required_sensitivity(mut self, sensitivity: BoundarySensitivity) -> Self {
        self.required_sensitivity = sensitivity;
        self
    }

    pub fn with_explicit_policy(mut self, policy_id: impl Into<String>) -> Self {
        self.explicit_policy_id = Some(policy_id.into());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphJoinDecision {
    pub allowed: bool,
    pub reason: String,
    pub left_boundary: SourceBoundaryLabel,
    pub right_boundary: SourceBoundaryLabel,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAttestation {
    pub graph_id: GraphNodeId,
    pub evidence_hash: String,
    pub verified: bool,
    pub statement: String,
}

impl RemoteAttestation {
    pub fn placeholder(graph_id: GraphNodeId, evidence_hash: impl Into<String>) -> Self {
        Self {
            graph_id,
            evidence_hash: evidence_hash.into(),
            verified: false,
            statement: "placeholder attestation; verification not implemented".to_owned(),
        }
    }
}

fn node_kind_rank(kind: FederatedGraphNodeKind) -> usize {
    match kind {
        FederatedGraphNodeKind::LocalGraph => 0,
        FederatedGraphNodeKind::TeamGraph => 1,
        FederatedGraphNodeKind::EnterpriseGraph => 2,
        FederatedGraphNodeKind::LabGraph => 3,
        FederatedGraphNodeKind::ExternalPublicGraph => 4,
        FederatedGraphNodeKind::PartnerGraph => 5,
        FederatedGraphNodeKind::PersonalGraph => 6,
    }
}
