//! Agent tool-use security primitives for Reality Graph.

use std::collections::BTreeSet;

use rg_core::{AgentId, AssertionId, MemoryId, SourceId, TenantId, TxTime};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Capability {
    Remember,
    Recall,
    Verify,
    Explain,
    Timeline,
    EvidencePack,
    ContradictionCheck,
    Simulate,
    ReadSource,
    WriteMemory,
}

impl Capability {
    pub fn slug(self) -> &'static str {
        match self {
            Self::Remember => "remember",
            Self::Recall => "recall",
            Self::Verify => "verify",
            Self::Explain => "explain",
            Self::Timeline => "timeline",
            Self::EvidencePack => "evidence_pack",
            Self::ContradictionCheck => "contradiction_check",
            Self::Simulate => "simulate",
            Self::ReadSource => "read_source",
            Self::WriteMemory => "write_memory",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityToken {
    pub id: String,
    pub agent_id: AgentId,
    pub tenant_id: TenantId,
    pub issued_at: TxTime,
    pub expires_at: TxTime,
    capabilities: BTreeSet<Capability>,
    tool_allowlist: BTreeSet<String>,
    source_allowlist: BTreeSet<SourceId>,
    pub signature: Option<String>,
}

impl CapabilityToken {
    pub fn new(
        id: impl Into<String>,
        agent_id: AgentId,
        tenant_id: TenantId,
        issued_at: TxTime,
        expires_at: TxTime,
    ) -> Self {
        Self {
            id: id.into(),
            agent_id,
            tenant_id,
            issued_at,
            expires_at,
            capabilities: BTreeSet::new(),
            tool_allowlist: BTreeSet::new(),
            source_allowlist: BTreeSet::new(),
            signature: None,
        }
    }

    pub fn grant(mut self, capability: Capability) -> Self {
        self.capabilities.insert(capability);
        self
    }

    pub fn allow_tool(mut self, tool_name: impl Into<String>) -> Self {
        self.tool_allowlist.insert(tool_name.into());
        self
    }

    pub fn allow_source(mut self, source_id: SourceId) -> Self {
        self.source_allowlist.insert(source_id);
        self
    }

    pub fn signed(mut self, signature: impl Into<String>) -> Self {
        self.signature = Some(signature.into());
        self
    }

    pub fn permits_tool(&self, tool_name: &str) -> bool {
        self.tool_allowlist.contains(tool_name)
    }

    pub fn permits_source(&self, source_id: &SourceId) -> bool {
        self.source_allowlist.contains(source_id)
    }

    pub fn has_capability(&self, capability: Capability) -> bool {
        self.capabilities.contains(&capability)
    }

    pub fn is_valid_at(&self, now: TxTime) -> bool {
        self.issued_at <= now && now <= self.expires_at
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolPermissionPolicy {
    allowed_tools: BTreeSet<String>,
    denied_tools: BTreeSet<String>,
    write_tools_requiring_review: BTreeSet<String>,
}

impl ToolPermissionPolicy {
    pub fn new() -> Self {
        Self {
            allowed_tools: BTreeSet::new(),
            denied_tools: BTreeSet::new(),
            write_tools_requiring_review: BTreeSet::new(),
        }
    }

    pub fn allow_tool(mut self, tool_name: impl Into<String>) -> Self {
        self.allowed_tools.insert(tool_name.into());
        self
    }

    pub fn deny_tool(mut self, tool_name: impl Into<String>) -> Self {
        self.denied_tools.insert(tool_name.into());
        self
    }

    pub fn require_review_for_write_tool(mut self, tool_name: impl Into<String>) -> Self {
        self.write_tools_requiring_review.insert(tool_name.into());
        self
    }

    pub fn evaluate(
        &self,
        token: &CapabilityToken,
        tool_name: &str,
        now: TxTime,
    ) -> PermissionDecision {
        if !token.is_valid_at(now) {
            return PermissionDecision::Denied {
                reason: "capability token expired or not yet valid".to_owned(),
            };
        }
        if self.denied_tools.contains(tool_name) {
            return PermissionDecision::Denied {
                reason: "tool is explicitly denied".to_owned(),
            };
        }
        if !self.allowed_tools.is_empty() && !self.allowed_tools.contains(tool_name) {
            return PermissionDecision::Denied {
                reason: "tool is not in the policy allowlist".to_owned(),
            };
        }
        if !token.permits_tool(tool_name) {
            return PermissionDecision::Denied {
                reason: "tool is not granted by capability token".to_owned(),
            };
        }
        if self.write_tools_requiring_review.contains(tool_name) {
            return PermissionDecision::RequiresHumanReview;
        }
        PermissionDecision::Allowed
    }
}

impl Default for ToolPermissionPolicy {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PermissionDecision {
    Allowed,
    RequiresHumanReview,
    Denied { reason: String },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SourceTaintLabel {
    Trusted,
    ExternalUnverified,
    PromptInjectionSuspected,
    ToolOutput,
    SecretAdjacent,
    Quarantined,
}

impl SourceTaintLabel {
    pub fn slug(self) -> &'static str {
        match self {
            Self::Trusted => "trusted",
            Self::ExternalUnverified => "external_unverified",
            Self::PromptInjectionSuspected => "prompt_injection_suspected",
            Self::ToolOutput => "tool_output",
            Self::SecretAdjacent => "secret_adjacent",
            Self::Quarantined => "quarantined",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceTaintStatus {
    pub labels: BTreeSet<SourceTaintLabel>,
}

impl SourceTaintStatus {
    pub fn trusted() -> Self {
        Self {
            labels: BTreeSet::new(),
        }
    }

    pub fn from_risk(risk: &PromptInjectionRiskScore) -> Self {
        let mut status = Self::trusted();
        if risk.quarantine_recommended() {
            status
                .labels
                .insert(SourceTaintLabel::PromptInjectionSuspected);
            status.labels.insert(SourceTaintLabel::Quarantined);
        }
        status
    }

    pub fn with_label(mut self, label: SourceTaintLabel) -> Self {
        if label != SourceTaintLabel::Trusted {
            self.labels.insert(label);
        }
        self
    }

    pub fn is_tainted(&self) -> bool {
        !self.labels.is_empty()
    }

    pub fn to_json(&self) -> Value {
        json!({
            "tainted": self.is_tainted(),
            "labels": self.labels.iter().map(|label| label.slug()).collect::<Vec<_>>(),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PromptInjectionRiskScore {
    pub score: f32,
    pub reasons: Vec<String>,
}

impl PromptInjectionRiskScore {
    pub fn assess_text(text: &str) -> Self {
        let lowered = text.to_ascii_lowercase();
        let mut score = 0.0_f32;
        let mut reasons = Vec::new();
        for (needle, reason, weight) in [
            ("ignore previous", "ignore previous instructions", 0.28),
            ("system prompt", "system prompt extraction", 0.22),
            ("hidden prompt", "hidden prompt extraction", 0.22),
            ("exfiltrate", "exfiltration request", 0.3),
            ("api key", "secret extraction request", 0.2),
            ("api_key", "secret extraction request", 0.2),
            ("reveal", "reveal hidden data request", 0.12),
            ("disable safety", "safety bypass request", 0.2),
        ] {
            if lowered.contains(needle) {
                score += weight;
                reasons.push(reason.to_owned());
            }
        }
        Self {
            score: score.min(1.0),
            reasons,
        }
    }

    pub fn assess_json(value: &Value) -> Self {
        Self::assess_text(&value.to_string())
    }

    pub fn quarantine_recommended(&self) -> bool {
        self.score >= 0.65
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PermissionScope {
    Public,
    Tool { tool_name: String },
    Agent { agent_id: AgentId },
    Tenant { tenant_id: TenantId },
    Quarantine,
    Denied,
}

impl PermissionScope {
    pub fn to_json(&self) -> Value {
        match self {
            Self::Public => json!({ "kind": "public" }),
            Self::Tool { tool_name } => json!({ "kind": "tool", "tool_name": tool_name }),
            Self::Agent { agent_id } => {
                json!({ "kind": "agent", "agent_id": agent_id.to_string() })
            }
            Self::Tenant { tenant_id } => {
                json!({ "kind": "tenant", "tenant_id": tenant_id.to_string() })
            }
            Self::Quarantine => json!({ "kind": "quarantine" }),
            Self::Denied => json!({ "kind": "denied" }),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataProvenance {
    pub source_ids: Vec<SourceId>,
    pub assertion_ids: Vec<AssertionId>,
    pub memory_ids: Vec<MemoryId>,
    pub derived_from_tool: Option<String>,
}

impl DataProvenance {
    pub fn empty() -> Self {
        Self {
            source_ids: Vec::new(),
            assertion_ids: Vec::new(),
            memory_ids: Vec::new(),
            derived_from_tool: None,
        }
    }

    pub fn from_json(tool_name: impl Into<String>, value: &Value) -> Self {
        let mut source_ids = BTreeSet::new();
        let mut assertion_ids = BTreeSet::new();
        let mut memory_ids = BTreeSet::new();
        collect_provenance(value, &mut source_ids, &mut assertion_ids, &mut memory_ids);
        Self {
            source_ids: source_ids.into_iter().collect(),
            assertion_ids: assertion_ids.into_iter().collect(),
            memory_ids: memory_ids.into_iter().collect(),
            derived_from_tool: Some(tool_name.into()),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "source_ids": self.source_ids.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "assertion_ids": self.assertion_ids.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "memory_ids": self.memory_ids.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "derived_from_tool": self.derived_from_tool,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceTrustStatus {
    Trusted,
    Mixed,
    Untrusted,
    Unknown,
}

impl SourceTrustStatus {
    fn slug(self) -> &'static str {
        match self {
            Self::Trusted => "trusted",
            Self::Mixed => "mixed",
            Self::Untrusted => "untrusted",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceTrustSummary {
    pub status: SourceTrustStatus,
    pub min_score: Option<f32>,
    pub source_count: usize,
}

impl SourceTrustSummary {
    pub fn trusted(score: f32) -> Self {
        Self {
            status: SourceTrustStatus::Trusted,
            min_score: Some(score),
            source_count: 1,
        }
    }

    pub fn unknown() -> Self {
        Self {
            status: SourceTrustStatus::Unknown,
            min_score: None,
            source_count: 0,
        }
    }

    pub fn from_scores(scores: Vec<Option<f32>>) -> Self {
        if scores.is_empty() {
            return Self::unknown();
        }
        let known = scores.iter().flatten().copied().collect::<Vec<_>>();
        if known.is_empty() {
            return Self {
                status: SourceTrustStatus::Unknown,
                min_score: None,
                source_count: scores.len(),
            };
        }
        let min_score = known
            .iter()
            .copied()
            .fold(f32::INFINITY, |left, right| left.min(right));
        let status = if min_score >= 0.8 {
            SourceTrustStatus::Trusted
        } else if min_score >= 0.5 {
            SourceTrustStatus::Mixed
        } else {
            SourceTrustStatus::Untrusted
        };
        Self {
            status,
            min_score: Some(min_score),
            source_count: scores.len(),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "status": self.status.slug(),
            "min_score": self.min_score,
            "source_count": self.source_count,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolCallAuditEvent {
    pub id: String,
    pub agent_id: AgentId,
    pub tool_name: String,
    pub source_ids: Vec<SourceId>,
    pub allowed: bool,
    pub decision_reason: String,
    pub occurred_at: TxTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolCallAuditLog {
    prefix: String,
    next_id: u64,
    events: Vec<ToolCallAuditEvent>,
}

impl ToolCallAuditLog {
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            next_id: 1,
            events: Vec::new(),
        }
    }

    pub fn record_allowed(
        &mut self,
        agent_id: AgentId,
        tool_name: impl Into<String>,
        source_ids: Vec<SourceId>,
        occurred_at: TxTime,
    ) -> ToolCallAuditEvent {
        self.record(
            agent_id,
            tool_name,
            source_ids,
            true,
            "allowed",
            occurred_at,
        )
    }

    pub fn record_denied(
        &mut self,
        agent_id: AgentId,
        tool_name: impl Into<String>,
        source_ids: Vec<SourceId>,
        reason: impl Into<String>,
        occurred_at: TxTime,
    ) -> ToolCallAuditEvent {
        self.record(agent_id, tool_name, source_ids, false, reason, occurred_at)
    }

    pub fn events(&self) -> &[ToolCallAuditEvent] {
        &self.events
    }

    fn record(
        &mut self,
        agent_id: AgentId,
        tool_name: impl Into<String>,
        source_ids: Vec<SourceId>,
        allowed: bool,
        reason: impl Into<String>,
        occurred_at: TxTime,
    ) -> ToolCallAuditEvent {
        let event = ToolCallAuditEvent {
            id: format!("{}-{:06}", self.prefix, self.next_id),
            agent_id,
            tool_name: tool_name.into(),
            source_ids,
            allowed,
            decision_reason: reason.into(),
            occurred_at,
        };
        self.next_id += 1;
        self.events.push(event.clone());
        event
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SandboxedMcpInvocation {
    pub tool_name: String,
    pub network_access: bool,
    pub filesystem_access: bool,
    pub command_execution: bool,
    pub timeout_ms: u64,
}

impl SandboxedMcpInvocation {
    pub fn new(tool_name: impl Into<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            network_access: false,
            filesystem_access: false,
            command_execution: false,
            timeout_ms: 5_000,
        }
    }

    pub fn with_network_access(mut self, enabled: bool) -> Self {
        self.network_access = enabled;
        self
    }

    pub fn with_filesystem_access(mut self, enabled: bool) -> Self {
        self.filesystem_access = enabled;
        self
    }

    pub fn with_command_execution(mut self, enabled: bool) -> Self {
        self.command_execution = enabled;
        self
    }

    pub fn allowed_by_default_sandbox(&self) -> bool {
        !self.network_access && !self.filesystem_access && !self.command_execution
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WriteRequest {
    pub agent_id: AgentId,
    pub source_ids: Vec<SourceId>,
    pub signature: Option<String>,
    pub human_approved: bool,
    pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WriteApprovalPolicy {
    require_signature: bool,
    require_source_ids: bool,
    require_human_review: bool,
}

impl WriteApprovalPolicy {
    pub fn human_review_mode() -> Self {
        Self {
            require_signature: true,
            require_source_ids: true,
            require_human_review: true,
        }
    }

    pub fn evaluate(&self, request: &WriteRequest) -> WriteApprovalDecision {
        let mut reasons = Vec::new();
        if self.require_source_ids && request.source_ids.is_empty() {
            reasons.push("missing source evidence".to_owned());
        }
        if self.require_signature && request.signature.is_none() {
            reasons.push("missing signed memory write".to_owned());
        }
        if self.require_human_review && !request.human_approved {
            reasons.push("human review required".to_owned());
        }
        let allowed = reasons.is_empty();
        WriteApprovalDecision {
            allowed,
            requires_human_review: !allowed && self.require_human_review,
            reasons,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WriteApprovalDecision {
    pub allowed: bool,
    pub requires_human_review: bool,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MemoryExfiltrationDetector;

impl MemoryExfiltrationDetector {
    pub fn inspect_text(&self, text: &str) -> ExfiltrationFinding {
        let memory_id_mentions = text.matches("memory_id=").count();
        let secret_patterns = ["api_key=", "password=", "secret=", "BEGIN PRIVATE KEY"];
        let mut reasons = Vec::new();
        if memory_id_mentions >= 3 {
            reasons.push("bulk memory export pattern".to_owned());
        }
        if secret_patterns.iter().any(|pattern| {
            text.to_ascii_lowercase()
                .contains(&pattern.to_ascii_lowercase())
        }) {
            reasons.push("secret-like material detected".to_owned());
        }
        let redacted_text = redact_secret_like_text(text);
        ExfiltrationFinding {
            exfiltration_suspected: !reasons.is_empty(),
            reasons,
            redacted_text,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExfiltrationFinding {
    pub exfiltration_suspected: bool,
    pub reasons: Vec<String>,
    pub redacted_text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolResponseSecurityMetadata {
    pub permission_scope: PermissionScope,
    pub data_provenance: DataProvenance,
    pub taint_status: SourceTaintStatus,
    pub source_trust: SourceTrustSummary,
    pub audit_event_id: String,
}

impl ToolResponseSecurityMetadata {
    pub fn tool(
        tool_name: impl Into<String>,
        data_provenance: DataProvenance,
        source_trust: SourceTrustSummary,
        audit_event_id: impl Into<String>,
    ) -> Self {
        Self {
            permission_scope: PermissionScope::Tool {
                tool_name: tool_name.into(),
            },
            data_provenance,
            taint_status: SourceTaintStatus::trusted(),
            source_trust,
            audit_event_id: audit_event_id.into(),
        }
    }

    pub fn quarantine(
        tool_name: impl Into<String>,
        risk: &PromptInjectionRiskScore,
        audit_event_id: impl Into<String>,
    ) -> Self {
        Self {
            permission_scope: PermissionScope::Quarantine,
            data_provenance: DataProvenance {
                derived_from_tool: Some(tool_name.into()),
                ..DataProvenance::empty()
            },
            taint_status: SourceTaintStatus::from_risk(risk),
            source_trust: SourceTrustSummary::unknown(),
            audit_event_id: audit_event_id.into(),
        }
    }

    pub fn denied(tool_name: impl Into<String>, audit_event_id: impl Into<String>) -> Self {
        Self {
            permission_scope: PermissionScope::Denied,
            data_provenance: DataProvenance {
                derived_from_tool: Some(tool_name.into()),
                ..DataProvenance::empty()
            },
            taint_status: SourceTaintStatus::trusted(),
            source_trust: SourceTrustSummary::unknown(),
            audit_event_id: audit_event_id.into(),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "permission_scope": self.permission_scope.to_json(),
            "data_provenance": self.data_provenance.to_json(),
            "taint_status": self.taint_status.to_json(),
            "source_trust": self.source_trust.to_json(),
            "audit_event_id": self.audit_event_id,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SecureToolResponse<T> {
    pub payload: T,
    pub security: ToolResponseSecurityMetadata,
}

fn collect_provenance(
    value: &Value,
    source_ids: &mut BTreeSet<SourceId>,
    assertion_ids: &mut BTreeSet<AssertionId>,
    memory_ids: &mut BTreeSet<MemoryId>,
) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                match (key.as_str(), value) {
                    ("source_id", Value::String(id)) => {
                        source_ids.insert(SourceId::new(id));
                    }
                    ("source_ids", Value::Array(ids)) => {
                        for id in ids.iter().filter_map(Value::as_str) {
                            source_ids.insert(SourceId::new(id));
                        }
                    }
                    ("assertion_id", Value::String(id)) => {
                        assertion_ids.insert(AssertionId::new(id));
                    }
                    ("assertion_ids", Value::Array(ids)) => {
                        for id in ids.iter().filter_map(Value::as_str) {
                            assertion_ids.insert(AssertionId::new(id));
                        }
                    }
                    ("memory_id", Value::String(id)) => {
                        memory_ids.insert(MemoryId::new(id));
                    }
                    ("memory_ids", Value::Array(ids)) => {
                        for id in ids.iter().filter_map(Value::as_str) {
                            memory_ids.insert(MemoryId::new(id));
                        }
                    }
                    _ => collect_provenance(value, source_ids, assertion_ids, memory_ids),
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_provenance(value, source_ids, assertion_ids, memory_ids);
            }
        }
        _ => {}
    }
}

fn redact_secret_like_text(text: &str) -> String {
    let mut redacted = text.to_owned();
    for key in ["api_key", "password", "secret"] {
        redacted = redact_key_value(&redacted, key);
    }
    if redacted.contains("BEGIN PRIVATE KEY") {
        redacted = "[REDACTED PRIVATE KEY]".to_owned();
    }
    redacted
}

fn redact_key_value(text: &str, key: &str) -> String {
    let needle = format!("{key}=");
    let Some(start) = text.find(&needle) else {
        return text.to_owned();
    };
    let value_start = start + needle.len();
    let value_end = text[value_start..]
        .find(char::is_whitespace)
        .map(|offset| value_start + offset)
        .unwrap_or(text.len());
    format!(
        "{}{}[REDACTED]{}",
        &text[..start],
        needle,
        &text[value_end..]
    )
}
