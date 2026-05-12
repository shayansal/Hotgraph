use rg_core::{AssertionId, EntityId, SourceId};
use rg_federation::{
    BoundarySensitivity, CrossGraphEntityResolution, FederatedEvidenceItem, FederatedGraph,
    FederatedGraphNode, FederatedGraphNodeKind, FederationQuery, GraphNodeId,
    PermissionedGraphJoin, RemoteAttestation, SourceBoundaryLabel, TrustBoundary,
};

#[test]
fn remote_query_plan_fans_out_only_to_permitted_graphs() {
    let federation = fixture_federation();

    let plan = federation.plan_query(FederationQuery::new(
        "principal-lab",
        "supplier risk around entity company-a",
        vec![EntityId::new("company-a")],
    ));

    assert_eq!(
        plan.target_graph_ids,
        vec![
            GraphNodeId::new("local"),
            GraphNodeId::new("team"),
            GraphNodeId::new("lab"),
            GraphNodeId::new("public"),
        ]
    );
    assert!(plan
        .skipped_graphs
        .iter()
        .any(|skipped| skipped.graph_id == GraphNodeId::new("partner")
            && skipped.reason.contains("permission")));
    assert!(plan.fanout_parallel);
    assert!(plan
        .steps
        .iter()
        .any(|step| step.contains("source boundary labels")));
}

#[test]
fn federated_evidence_pack_merges_partial_results_with_boundary_labels_and_trust_scores() {
    let federation = fixture_federation();
    let plan = federation.plan_query(FederationQuery::new(
        "principal-lab",
        "supplier risk around entity company-a",
        vec![EntityId::new("company-a")],
    ));

    let pack = federation.execute_plan(&plan);

    assert_eq!(pack.query, "supplier risk around entity company-a");
    assert_eq!(pack.partial_results.len(), 4);
    assert_eq!(pack.merged_evidence.len(), 4);
    assert!(pack
        .partial_results
        .iter()
        .all(|result| result.boundary.graph_id == result.node_id));
    assert!(pack
        .merged_evidence
        .windows(2)
        .all(|window| window[0].weighted_score >= window[1].weighted_score));
    assert_eq!(
        pack.merged_evidence[0].boundary.boundary,
        TrustBoundary::Lab
    );
    assert!(pack
        .source_boundaries
        .iter()
        .any(|label| label.graph_id == GraphNodeId::new("public")
            && label.boundary == TrustBoundary::ExternalPublicGraph));
    assert!(pack
        .warnings
        .iter()
        .any(|warning| warning.contains("partner")));
}

#[test]
fn cross_graph_entity_resolution_links_aliases_without_erasing_local_ids() {
    let mut resolver = CrossGraphEntityResolution::new();
    resolver.link(
        EntityId::new("local:company-a"),
        GraphNodeId::new("local"),
        EntityId::new("company-a"),
        "canonical local company identity",
    );
    resolver.link(
        EntityId::new("lab:co-a"),
        GraphNodeId::new("lab"),
        EntityId::new("company-a"),
        "lab graph alias from experiment registry",
    );

    let cluster = resolver.resolve(&EntityId::new("lab:co-a")).unwrap();

    assert_eq!(cluster.canonical_entity_id, EntityId::new("company-a"));
    assert_eq!(cluster.members.len(), 2);
    assert!(cluster
        .members
        .iter()
        .any(|member| member.graph_id == GraphNodeId::new("lab")
            && member.local_entity_id == EntityId::new("lab:co-a")));
    assert!(cluster
        .explanation
        .contains("preserves graph-local entity IDs"));
}

#[test]
fn permissioned_graph_join_blocks_sensitive_cross_boundary_join_without_policy() {
    let federation = fixture_federation();
    let join = PermissionedGraphJoin::new(
        GraphNodeId::new("lab"),
        GraphNodeId::new("partner"),
        EntityId::new("company-a"),
    )
    .with_required_sensitivity(BoundarySensitivity::Restricted);

    let denied = federation.evaluate_join(&join, "principal-lab").unwrap();
    assert!(!denied.allowed);
    assert!(denied.reason.contains("join denied"));

    let allowed = federation
        .evaluate_join(
            &join.with_explicit_policy("partner-risk-mou"),
            "principal-lab",
        )
        .unwrap();
    assert!(allowed.allowed);
    assert!(allowed.reason.contains("partner-risk-mou"));
}

#[test]
fn remote_attestation_placeholder_tracks_declared_and_unverified_nodes() {
    let attested =
        RemoteAttestation::placeholder(GraphNodeId::new("enterprise"), "sha256:enterprise-runtime");

    assert_eq!(attested.graph_id, GraphNodeId::new("enterprise"));
    assert!(!attested.verified);
    assert_eq!(
        attested.statement,
        "placeholder attestation; verification not implemented"
    );
    assert!(attested.evidence_hash.contains("sha256"));
}

fn fixture_federation() -> FederatedGraph {
    let mut federation = FederatedGraph::new();
    federation.add_node(
        FederatedGraphNode::new(
            "local",
            FederatedGraphNodeKind::LocalGraph,
            TrustBoundary::Local,
            1.0,
        )
        .allow_principal("principal-lab")
        .with_result(evidence(
            "local-a",
            "company-a",
            "assertion-local",
            "source-local",
            0.92,
        )),
    );
    federation.add_node(
        FederatedGraphNode::new(
            "team",
            FederatedGraphNodeKind::TeamGraph,
            TrustBoundary::Team,
            0.88,
        )
        .allow_principal("principal-lab")
        .with_result(evidence(
            "team-a",
            "company-a",
            "assertion-team",
            "source-team",
            0.81,
        )),
    );
    federation.add_node(
        FederatedGraphNode::new(
            "enterprise",
            FederatedGraphNodeKind::EnterpriseGraph,
            TrustBoundary::Enterprise,
            0.91,
        )
        .allow_principal("principal-enterprise")
        .with_result(evidence(
            "enterprise-a",
            "company-a",
            "assertion-enterprise",
            "source-enterprise",
            0.95,
        )),
    );
    federation.add_node(
        FederatedGraphNode::new(
            "lab",
            FederatedGraphNodeKind::LabGraph,
            TrustBoundary::Lab,
            0.97,
        )
        .allow_principal("principal-lab")
        .with_result(evidence(
            "lab-a",
            "lab:co-a",
            "assertion-lab",
            "source-lab",
            0.96,
        )),
    );
    federation.add_node(
        FederatedGraphNode::new(
            "public",
            FederatedGraphNodeKind::ExternalPublicGraph,
            TrustBoundary::ExternalPublicGraph,
            0.56,
        )
        .allow_public_read()
        .with_result(evidence(
            "public-a",
            "company-a",
            "assertion-public",
            "source-public",
            0.72,
        )),
    );
    federation.add_node(
        FederatedGraphNode::new(
            "partner",
            FederatedGraphNodeKind::PartnerGraph,
            TrustBoundary::Partner,
            0.76,
        )
        .allow_principal("principal-partner")
        .with_result(evidence(
            "partner-a",
            "company-a",
            "assertion-partner",
            "source-partner",
            0.8,
        )),
    );
    federation
}

fn evidence(
    id: &str,
    entity: &str,
    assertion: &str,
    source: &str,
    confidence: f32,
) -> FederatedEvidenceItem {
    FederatedEvidenceItem {
        id: id.to_owned(),
        entity_id: EntityId::new(entity),
        assertion_id: AssertionId::new(assertion),
        source_id: SourceId::new(source),
        text: format!("{entity} evidence from {source}"),
        confidence,
        weighted_score: confidence,
        boundary: SourceBoundaryLabel::unknown(GraphNodeId::new("pending")),
    }
}
