//! Enterprise governance controls for Reality Graph.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rg_ai::{EvidencePack, SourceExcerpt};
use rg_core::{AgentId, Assertion, EntityId, MemoryId, SourceId, TenantId, TxTime};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PrincipalId(String);

impl PrincipalId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PrincipalId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Principal {
    pub id: PrincipalId,
    pub tenant_id: TenantId,
    pub agent_id: Option<AgentId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessMode {
    Read,
    Write,
    Delete,
    Redact,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PermissionScope {
    Tenant(TenantId),
    Source(SourceId),
    Memory(MemoryId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PermissionPolicy {
    tenant_id: TenantId,
    read_scopes: BTreeSet<PermissionScope>,
    source_policies: BTreeMap<SourceId, SourceAccessPolicy>,
    memory_policies: BTreeMap<MemoryId, MemoryAccessPolicy>,
    redactions: Vec<RedactionEvent>,
}

impl PermissionPolicy {
    pub fn new(tenant_id: TenantId) -> Self {
        Self {
            tenant_id,
            read_scopes: BTreeSet::new(),
            source_policies: BTreeMap::new(),
            memory_policies: BTreeMap::new(),
            redactions: Vec::new(),
        }
    }

    pub fn allow_read(mut self, scope: PermissionScope) -> Self {
        self.read_scopes.insert(scope);
        self
    }

    pub fn with_source_policy(mut self, policy: SourceAccessPolicy) -> Self {
        self.source_policies
            .insert(policy.source_id.clone(), policy);
        self
    }

    pub fn with_memory_policy(mut self, policy: MemoryAccessPolicy) -> Self {
        self.memory_policies
            .insert(policy.memory_id.clone(), policy);
        self
    }

    pub fn with_redaction(mut self, redaction: RedactionEvent) -> Self {
        self.redactions.push(redaction);
        self
    }

    fn can_read_tenant(&self, principal: &Principal) -> bool {
        principal.tenant_id == self.tenant_id
            && self
                .read_scopes
                .contains(&PermissionScope::Tenant(principal.tenant_id.clone()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceAccessPolicy {
    source_id: SourceId,
    allowed_principals: BTreeSet<PrincipalId>,
}

impl SourceAccessPolicy {
    pub fn restricted(source_id: SourceId, allowed_principals: Vec<PrincipalId>) -> Self {
        Self {
            source_id,
            allowed_principals: allowed_principals.into_iter().collect(),
        }
    }

    pub fn can_read(&self, principal: &Principal) -> bool {
        self.allowed_principals.contains(&principal.id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryAccessPolicy {
    memory_id: MemoryId,
    owner_agent_id: AgentId,
    reader_agent_ids: BTreeSet<AgentId>,
    public_read: bool,
}

impl MemoryAccessPolicy {
    pub fn private(memory_id: MemoryId, owner_agent_id: AgentId) -> Self {
        Self {
            memory_id,
            owner_agent_id,
            reader_agent_ids: BTreeSet::new(),
            public_read: false,
        }
    }

    pub fn allow_agent(mut self, agent_id: AgentId) -> Self {
        self.reader_agent_ids.insert(agent_id);
        self
    }

    pub fn public(mut self) -> Self {
        self.public_read = true;
        self
    }

    pub fn can_read(&self, agent_id: Option<&AgentId>) -> bool {
        let Some(agent_id) = agent_id else {
            return self.public_read;
        };
        self.public_read
            || &self.owner_agent_id == agent_id
            || self.reader_agent_ids.contains(agent_id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WritePolicy {
    allowed_writers: BTreeSet<PrincipalId>,
    review_required: bool,
}

impl WritePolicy {
    pub fn review_required() -> Self {
        Self {
            allowed_writers: BTreeSet::new(),
            review_required: true,
        }
    }

    pub fn allow_writer(mut self, principal_id: PrincipalId) -> Self {
        self.allowed_writers.insert(principal_id);
        self
    }

    pub fn can_write(&self, principal_id: &PrincipalId) -> bool {
        self.allowed_writers.contains(principal_id)
    }

    pub fn can_commit_without_review(&self) -> bool {
        !self.review_required
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LegalHoldStatus {
    None,
    Held { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetentionPolicy {
    retain_until: Option<TxTime>,
    legal_hold: LegalHoldStatus,
}

impl RetentionPolicy {
    pub fn retain_until(retain_until: TxTime) -> Self {
        Self {
            retain_until: Some(retain_until),
            legal_hold: LegalHoldStatus::None,
        }
    }

    pub fn is_retainable_at(&self, now: TxTime) -> bool {
        match self.retain_until {
            Some(retain_until) => now <= retain_until,
            None => true,
        }
    }

    pub fn with_legal_hold(mut self, legal_hold: LegalHoldStatus) -> Self {
        self.legal_hold = legal_hold;
        self
    }

    pub fn blocks_deletion(&self) -> bool {
        matches!(self.legal_hold, LegalHoldStatus::Held { .. })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RedactionKind {
    FullDeletion,
    FieldRedaction { reason: String },
    PiiDetected { detector: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RedactionTarget {
    Source(SourceId),
    SourceField { source_id: SourceId, field: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedactionEvent {
    pub target: RedactionTarget,
    pub kind: RedactionKind,
    pub redacted_by: PrincipalId,
    pub reason: String,
    pub transaction_time: TxTime,
}

impl RedactionEvent {
    pub fn source(
        source_id: SourceId,
        redacted_by: PrincipalId,
        reason: impl Into<String>,
        transaction_time: TxTime,
    ) -> Self {
        Self {
            target: RedactionTarget::Source(source_id),
            kind: RedactionKind::FullDeletion,
            redacted_by,
            reason: reason.into(),
            transaction_time,
        }
    }

    pub fn source_field(
        source_id: SourceId,
        field: impl Into<String>,
        kind: RedactionKind,
        redacted_by: PrincipalId,
        transaction_time: TxTime,
    ) -> Self {
        let reason = redaction_reason(&kind);
        Self {
            target: RedactionTarget::SourceField {
                source_id,
                field: field.into(),
            },
            kind,
            redacted_by,
            reason,
            transaction_time,
        }
    }

    pub fn resource_id(&self) -> String {
        match &self.target {
            RedactionTarget::Source(source_id) => source_id.to_string(),
            RedactionTarget::SourceField { source_id, .. } => source_id.to_string(),
        }
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    fn full_source_id(&self) -> Option<&SourceId> {
        match &self.target {
            RedactionTarget::Source(source_id) => Some(source_id),
            RedactionTarget::SourceField { .. } => None,
        }
    }

    fn field_for_source(&self, source_id: &SourceId) -> Option<&str> {
        match &self.target {
            RedactionTarget::SourceField {
                source_id: redacted_source_id,
                field,
            } if redacted_source_id == source_id => Some(field),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSignature {
    pub source_id: SourceId,
    pub signed_by: PrincipalId,
    pub key_id: String,
    signature: String,
    pub transaction_time: TxTime,
}

impl SourceSignature {
    pub fn new(
        source_id: SourceId,
        signed_by: PrincipalId,
        key_id: impl Into<String>,
        signature: impl Into<String>,
        transaction_time: TxTime,
    ) -> Self {
        Self {
            source_id,
            signed_by,
            key_id: key_id.into(),
            signature: signature.into(),
            transaction_time,
        }
    }

    pub fn verifies(&self, signature: &str) -> bool {
        constant_time_eq(self.signature.as_bytes(), signature.as_bytes())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuditReason {
    AiContextPack,
    UserQuestion,
    LegalReview,
    Maintenance,
    Other(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditEvent {
    pub principal_id: PrincipalId,
    pub tenant_id: TenantId,
    pub access_mode: AccessMode,
    pub resource_type: String,
    pub resource_id: String,
    pub reason: AuditReason,
    pub allowed: bool,
    pub decision_reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccessDenial {
    pub resource_type: String,
    pub resource_id: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GovernedEvidencePack {
    pub pack: EvidencePack,
    pub audit_events: Vec<AuditEvent>,
    pub denials: Vec<AccessDenial>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AssertionDecision {
    allowed: bool,
    reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GovernanceEngine {
    policy: PermissionPolicy,
    audit_log: Vec<AuditEvent>,
}

impl GovernanceEngine {
    pub fn new(policy: PermissionPolicy) -> Self {
        Self {
            policy,
            audit_log: Vec::new(),
        }
    }

    pub fn enforce_evidence_pack(
        &self,
        principal: Principal,
        pack: &EvidencePack,
        reason: AuditReason,
    ) -> GovernedEvidencePack {
        self.enforce_evidence_pack_inner(&principal, pack, reason)
    }

    pub fn enforce_evidence_pack_mut(
        &mut self,
        principal: Principal,
        pack: &EvidencePack,
        reason: AuditReason,
    ) -> GovernedEvidencePack {
        let governed = self.enforce_evidence_pack_inner(&principal, pack, reason);
        self.audit_log.extend(governed.audit_events.clone());
        governed
    }

    pub fn audit_log(&self) -> &[AuditEvent] {
        &self.audit_log
    }

    pub fn can_read_memory(
        &self,
        _principal: &Principal,
        memory_id: &MemoryId,
        agent_id: Option<&AgentId>,
    ) -> bool {
        self.policy
            .memory_policies
            .get(memory_id)
            .map_or(true, |policy| policy.can_read(agent_id))
    }

    pub fn check_source_access(
        &self,
        principal: &Principal,
        source_id: &SourceId,
    ) -> Option<AccessDenial> {
        let decision = self.can_read_source(principal, source_id);
        (!decision.allowed).then(|| AccessDenial {
            resource_type: "source".to_owned(),
            resource_id: source_id.to_string(),
            reason: decision.reason,
        })
    }

    fn enforce_evidence_pack_inner(
        &self,
        principal: &Principal,
        pack: &EvidencePack,
        reason: AuditReason,
    ) -> GovernedEvidencePack {
        let mut audit_events = Vec::new();
        let mut denials = Vec::new();
        let mut allowed_assertion_ids = BTreeSet::new();

        for assertion in &pack.assertions {
            let decision = self.can_read_assertion(principal, assertion);
            if decision.allowed {
                allowed_assertion_ids.insert(assertion.id.clone());
            } else {
                denials.push(AccessDenial {
                    resource_type: "assertion".to_owned(),
                    resource_id: assertion.id.to_string(),
                    reason: decision.reason.clone(),
                });
            }
            audit_events.push(AuditEvent {
                principal_id: principal.id.clone(),
                tenant_id: principal.tenant_id.clone(),
                access_mode: AccessMode::Read,
                resource_type: "assertion".to_owned(),
                resource_id: assertion.id.to_string(),
                reason: reason.clone(),
                allowed: decision.allowed,
                decision_reason: decision.reason,
            });
        }

        let assertions = pack
            .assertions
            .iter()
            .filter(|assertion| allowed_assertion_ids.contains(&assertion.id))
            .cloned()
            .collect::<Vec<_>>();
        let allowed_source_ids = assertions
            .iter()
            .flat_map(|assertion| assertion.source_ids.iter().cloned())
            .collect::<BTreeSet<_>>();
        let allowed_entity_ids = assertions
            .iter()
            .flat_map(assertion_entity_ids)
            .collect::<BTreeSet<_>>();

        for source in &pack.sources {
            if allowed_source_ids.contains(&source.source_id) {
                continue;
            }
            if !self.can_read_source(principal, &source.source_id).allowed {
                denials.push(AccessDenial {
                    resource_type: "source".to_owned(),
                    resource_id: source.source_id.to_string(),
                    reason: self.can_read_source(principal, &source.source_id).reason,
                });
            }
        }

        let sources = pack
            .sources
            .iter()
            .filter(|source| allowed_source_ids.contains(&source.source_id))
            .filter_map(|source| self.redact_source_excerpt(source))
            .collect::<Vec<_>>();
        let entities = pack
            .entities
            .iter()
            .filter(|entity| allowed_entity_ids.contains(&entity.id))
            .cloned()
            .collect::<Vec<_>>();
        let paths = pack
            .paths
            .iter()
            .filter(|path| {
                path.hops
                    .iter()
                    .all(|hop| allowed_assertion_ids.contains(&hop.assertion_id))
            })
            .cloned()
            .collect::<Vec<_>>();
        let contradictions = pack
            .contradictions
            .iter()
            .filter(|contradiction| {
                allowed_assertion_ids.contains(&contradiction.assertion_a)
                    && allowed_assertion_ids.contains(&contradiction.assertion_b)
            })
            .cloned()
            .collect::<Vec<_>>();

        GovernedEvidencePack {
            pack: EvidencePack {
                query: pack.query.clone(),
                entities,
                assertions,
                sources,
                paths,
                contradictions,
                generated_at: pack.generated_at,
            },
            audit_events,
            denials,
        }
    }

    fn can_read_assertion(
        &self,
        principal: &Principal,
        assertion: &Assertion,
    ) -> AssertionDecision {
        if !self.policy.can_read_tenant(principal) {
            return AssertionDecision {
                allowed: false,
                reason: "principal is outside policy tenant or lacks tenant read scope".to_owned(),
            };
        }
        if assertion.context != tenant_context(&principal.tenant_id) {
            return AssertionDecision {
                allowed: false,
                reason: "assertion belongs to a different tenant".to_owned(),
            };
        }
        for source_id in &assertion.source_ids {
            let source_decision = self.can_read_source(principal, source_id);
            if !source_decision.allowed {
                return source_decision;
            }
        }
        AssertionDecision {
            allowed: true,
            reason: "allowed by tenant and source policies".to_owned(),
        }
    }

    fn can_read_source(&self, principal: &Principal, source_id: &SourceId) -> AssertionDecision {
        if self.is_source_deleted(source_id) {
            return AssertionDecision {
                allowed: false,
                reason: "source is deleted or redacted".to_owned(),
            };
        }
        if let Some(policy) = self.policy.source_policies.get(source_id) {
            if !policy.can_read(principal) {
                return AssertionDecision {
                    allowed: false,
                    reason: "source policy denies this principal".to_owned(),
                };
            }
        }
        AssertionDecision {
            allowed: true,
            reason: "source is readable".to_owned(),
        }
    }

    fn redact_source_excerpt(&self, source: &SourceExcerpt) -> Option<SourceExcerpt> {
        if self.is_source_deleted(&source.source_id) {
            return None;
        }
        let mut redacted = source.clone();
        for redaction in &self.policy.redactions {
            let Some(field) = redaction.field_for_source(&source.source_id) else {
                continue;
            };
            match field {
                "snippet" => redacted.snippet = format!("[redacted: {}]", redaction.reason),
                "uri" => redacted.uri = None,
                _ => {}
            }
        }
        Some(redacted)
    }

    fn is_source_deleted(&self, source_id: &SourceId) -> bool {
        self.policy
            .redactions
            .iter()
            .any(|redaction| redaction.full_source_id() == Some(source_id))
    }
}

fn assertion_entity_ids(assertion: &Assertion) -> Vec<EntityId> {
    let mut entity_ids = vec![assertion.subject.clone()];
    if let rg_core::GraphValue::Entity(entity_id) = &assertion.object {
        entity_ids.push(entity_id.clone());
    }
    entity_ids
}

fn tenant_context(tenant_id: &TenantId) -> rg_core::ContextScope {
    rg_core::ContextScope::Named(format!("tenant:{tenant_id}"))
}

fn redaction_reason(kind: &RedactionKind) -> String {
    match kind {
        RedactionKind::FullDeletion => "deleted".to_owned(),
        RedactionKind::FieldRedaction { reason } => reason.clone(),
        RedactionKind::PiiDetected { detector } => format!("PII detected by {detector}"),
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |accumulator, (left, right)| {
            accumulator | (*left ^ *right)
        })
        == 0
}
