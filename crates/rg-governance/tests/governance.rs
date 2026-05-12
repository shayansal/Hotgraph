use rg_ai::{EvidencePack, GraphPath, SourceExcerpt};
use rg_core::{
    AgentId, Assertion, AssertionId, AssertionStatus, Confidence, ContentHash, ContextScope,
    Entity, EntityId, EntityType, GraphValue, MemoryId, PredicateId, PropertyMap, SourceId,
    SourceType, TenantId, TimeInterval, TxTime, ValidTime,
};
use rg_governance::{
    AccessMode, AuditReason, GovernanceEngine, LegalHoldStatus, MemoryAccessPolicy,
    PermissionPolicy, PermissionScope, PrincipalId, RedactionEvent, RedactionKind, RetentionPolicy,
    SourceAccessPolicy, SourceSignature, WritePolicy,
};
use rg_index::{Contradiction, ContradictionType, Severity};
use rg_query::QueryResult;

#[test]
fn no_cross_tenant_leakage_when_filtering_evidence_pack() {
    let policy = PermissionPolicy::new(TenantId::new("tenant-a"))
        .allow_read(PermissionScope::Tenant(TenantId::new("tenant-a")));
    let engine = GovernanceEngine::new(policy);

    let governed = engine.enforce_evidence_pack(
        principal("analyst", "tenant-a"),
        &pack_with_two_tenants(),
        AuditReason::AiContextPack,
    );

    assert_eq!(governed.pack.assertions.len(), 1);
    assert_eq!(
        governed.pack.assertions[0].id,
        AssertionId::new("assertion-a")
    );
    assert_eq!(governed.pack.sources.len(), 1);
    assert_eq!(
        governed.pack.sources[0].source_id,
        SourceId::new("source-a")
    );
    assert!(governed.pack.paths.iter().all(|path| path
        .hops
        .iter()
        .all(|hop| hop.context == tenant("tenant-a"))));
    assert!(governed
        .denials
        .iter()
        .any(|denial| denial.resource_id == "assertion-b"));
}

#[test]
fn evidence_packs_respect_source_permissions() {
    let policy = PermissionPolicy::new(TenantId::new("tenant-a"))
        .allow_read(PermissionScope::Tenant(TenantId::new("tenant-a")))
        .with_source_policy(SourceAccessPolicy::restricted(
            SourceId::new("source-a"),
            vec![PrincipalId::new("legal-reviewer")],
        ));
    let engine = GovernanceEngine::new(policy);

    let blocked = engine.enforce_evidence_pack(
        principal("analyst", "tenant-a"),
        &pack_with_two_tenants(),
        AuditReason::UserQuestion,
    );
    assert!(blocked.pack.assertions.is_empty());
    assert!(blocked.pack.sources.is_empty());
    assert!(blocked
        .denials
        .iter()
        .any(|denial| denial.resource_id == "source-a"));

    let allowed = engine.enforce_evidence_pack(
        principal("legal-reviewer", "tenant-a"),
        &pack_with_two_tenants(),
        AuditReason::LegalReview,
    );
    assert_eq!(allowed.pack.assertions.len(), 1);
    assert_eq!(allowed.pack.sources[0].source_id, SourceId::new("source-a"));
}

#[test]
fn redacted_data_is_not_retrievable_through_sources_paths_contradictions_or_summaries() {
    let policy = PermissionPolicy::new(TenantId::new("tenant-a"))
        .allow_read(PermissionScope::Tenant(TenantId::new("tenant-a")))
        .with_redaction(RedactionEvent::source(
            SourceId::new("source-a"),
            PrincipalId::new("privacy-officer"),
            "PII erasure request",
            TxTime::new(20260512),
        ));
    let engine = GovernanceEngine::new(policy);

    let governed = engine.enforce_evidence_pack(
        principal("analyst", "tenant-a"),
        &pack_with_two_tenants(),
        AuditReason::AiContextPack,
    );

    assert!(governed.pack.assertions.is_empty());
    assert!(governed.pack.sources.is_empty());
    assert!(governed.pack.paths.is_empty());
    assert!(governed.pack.contradictions.is_empty());
    assert!(!governed.pack.to_golden_string().contains("Alice SSN"));
    assert!(governed
        .denials
        .iter()
        .any(|denial| denial.reason.contains("redacted")));
}

#[test]
fn memory_permissions_gate_agent_memory_access() {
    let policy = PermissionPolicy::new(TenantId::new("tenant-a")).with_memory_policy(
        MemoryAccessPolicy::private(MemoryId::new("memory-private"), AgentId::new("agent-owner"))
            .allow_agent(AgentId::new("agent-auditor")),
    );
    let engine = GovernanceEngine::new(policy);

    assert!(engine.can_read_memory(
        &principal("analyst", "tenant-a"),
        &MemoryId::new("memory-private"),
        Some(&AgentId::new("agent-owner")),
    ));
    assert!(engine.can_read_memory(
        &principal("analyst", "tenant-a"),
        &MemoryId::new("memory-private"),
        Some(&AgentId::new("agent-auditor")),
    ));
    assert!(!engine.can_read_memory(
        &principal("analyst", "tenant-a"),
        &MemoryId::new("memory-private"),
        Some(&AgentId::new("agent-outsider")),
    ));
}

#[test]
fn audit_logs_show_who_accessed_what_and_why() {
    let policy = PermissionPolicy::new(TenantId::new("tenant-a"))
        .allow_read(PermissionScope::Tenant(TenantId::new("tenant-a")));
    let mut engine = GovernanceEngine::new(policy);

    let governed = engine.enforce_evidence_pack_mut(
        principal("analyst", "tenant-a"),
        &pack_with_two_tenants(),
        AuditReason::AiContextPack,
    );

    assert_eq!(governed.audit_events.len(), 2);
    assert!(governed.audit_events.iter().any(|event| {
        event.principal_id == PrincipalId::new("analyst")
            && event.tenant_id == TenantId::new("tenant-a")
            && event.access_mode == AccessMode::Read
            && event.resource_id == "assertion-a"
            && event.allowed
            && event.reason == AuditReason::AiContextPack
    }));
    assert!(engine.audit_log().iter().any(|event| {
        event.resource_id == "assertion-b"
            && !event.allowed
            && event.reason == AuditReason::AiContextPack
    }));
}

#[test]
fn retention_legal_hold_deletion_redaction_pii_and_source_signing_are_policy_objects() {
    let active = RetentionPolicy::retain_until(TxTime::new(20270101));
    assert!(active.is_retainable_at(TxTime::new(20260512)));
    assert!(!active.is_retainable_at(TxTime::new(20280101)));

    let held = active.with_legal_hold(LegalHoldStatus::Held {
        reason: "litigation".to_owned(),
    });
    assert!(held.blocks_deletion());

    let pii_redaction = RedactionEvent::source_field(
        SourceId::new("source-a"),
        "snippet",
        RedactionKind::PiiDetected {
            detector: "fixture-detector".to_owned(),
        },
        PrincipalId::new("privacy-officer"),
        TxTime::new(20260512),
    );
    assert_eq!(pii_redaction.resource_id(), "source-a");
    assert!(pii_redaction.reason().contains("PII"));

    let signature = SourceSignature::new(
        SourceId::new("source-a"),
        PrincipalId::new("signer"),
        "ed25519:test-key",
        "sig-fixture",
        TxTime::new(20260512),
    );
    assert!(signature.verifies("sig-fixture"));
    assert!(!signature.verifies("tampered"));

    let write_policy = WritePolicy::review_required().allow_writer(PrincipalId::new("writer"));
    assert!(write_policy.can_write(&PrincipalId::new("writer")));
    assert!(!write_policy.can_commit_without_review());
}

fn pack_with_two_tenants() -> EvidencePack {
    let assertion_a = assertion(AssertionFixture {
        id: "assertion-a",
        subject: "person-a",
        object: GraphValue::Entity(EntityId::new("company-a")),
        context: tenant("tenant-a"),
        source: "source-a",
    });
    let assertion_b = assertion(AssertionFixture {
        id: "assertion-b",
        subject: "person-b",
        object: GraphValue::Entity(EntityId::new("company-b")),
        context: tenant("tenant-b"),
        source: "source-b",
    });

    EvidencePack {
        query: "Who did Alice work for?".to_owned(),
        entities: vec![
            entity("person-a"),
            entity("company-a"),
            entity("person-b"),
            entity("company-b"),
        ],
        assertions: vec![assertion_a.clone(), assertion_b.clone()],
        sources: vec![
            SourceExcerpt {
                source_id: SourceId::new("source-a"),
                source_type: SourceType::Document,
                uri: Some("file://tenant-a/hr.md".to_owned()),
                content_hash: ContentHash::new("sha256:tenant-a"),
                snippet: "Alice SSN 123-45-6789 worked at Oracle in tenant A.".to_owned(),
                trust_score: Some(0.98),
            },
            SourceExcerpt {
                source_id: SourceId::new("source-b"),
                source_type: SourceType::Document,
                uri: Some("file://tenant-b/hr.md".to_owned()),
                content_hash: ContentHash::new("sha256:tenant-b"),
                snippet: "Bob worked at Acme in tenant B.".to_owned(),
                trust_score: Some(0.91),
            },
        ],
        paths: vec![
            GraphPath {
                start: EntityId::new("person-a"),
                end: EntityId::new("company-a"),
                hops: vec![query_result(&assertion_a)],
            },
            GraphPath {
                start: EntityId::new("person-b"),
                end: EntityId::new("company-b"),
                hops: vec![query_result(&assertion_b)],
            },
        ],
        contradictions: vec![Contradiction {
            id: rg_core::ContradictionId::new("cross-tenant-conflict"),
            assertion_a: AssertionId::new("assertion-a"),
            assertion_b: AssertionId::new("assertion-b"),
            contradiction_type: ContradictionType::ExactPredicateConflict,
            severity: Severity::High,
            explanation: "Should disappear because one side is unauthorized.".to_owned(),
        }],
        generated_at: TxTime::new(20260512),
    }
}

fn principal(id: &str, tenant_id: &str) -> rg_governance::Principal {
    rg_governance::Principal {
        id: PrincipalId::new(id),
        tenant_id: TenantId::new(tenant_id),
        agent_id: None,
    }
}

fn tenant(id: &str) -> ContextScope {
    ContextScope::Named(format!("tenant:{id}"))
}

struct AssertionFixture<'a> {
    id: &'a str,
    subject: &'a str,
    object: GraphValue,
    context: ContextScope,
    source: &'a str,
}

fn assertion(fixture: AssertionFixture<'_>) -> Assertion {
    Assertion {
        id: AssertionId::new(fixture.id),
        subject: EntityId::new(fixture.subject),
        predicate: PredicateId::new("WORKED_AT"),
        object: fixture.object,
        valid_time: TimeInterval::new(ValidTime::new(20240101), Some(ValidTime::new(20250101)))
            .expect("valid time"),
        transaction_time: TimeInterval::new(TxTime::new(20260501), None).expect("tx time"),
        confidence: Confidence::new(0.93).expect("confidence"),
        source_ids: vec![SourceId::new(fixture.source)],
        context: fixture.context,
        status: AssertionStatus::Active,
    }
}

fn entity(id: &str) -> Entity {
    Entity {
        id: EntityId::new(id),
        entity_type: EntityType::Person,
        canonical_name: Some(id.to_owned()),
        properties: PropertyMap::default(),
        created_tx: TxTime::new(20260501),
    }
}

fn query_result(assertion: &Assertion) -> QueryResult {
    QueryResult {
        assertion_id: assertion.id.clone(),
        subject: assertion.subject.clone(),
        predicate: assertion.predicate.clone(),
        object: assertion.object.clone(),
        valid_from: assertion.valid_time.start,
        valid_to: assertion.valid_time.end,
        tx_from: assertion.transaction_time.start,
        tx_to: assertion.transaction_time.end,
        confidence: assertion.confidence,
        source_ids: assertion.source_ids.clone(),
        context: assertion.context.clone(),
    }
}
