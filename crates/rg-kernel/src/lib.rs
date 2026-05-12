//! Core Graph 2.0 Reality Kernel primitives.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;
use std::time::Duration;

use rg_core::{
    AgentId, Confidence, EventId, GraphValue, PredicateId, SourceId, TenantId, TimeInterval,
    TxTime, ValidTime,
};

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

string_newtype!(RealityAtomId);
pub type AtomId = RealityAtomId;
string_newtype!(EntityRef);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TransactionTime(TxTime);

impl TransactionTime {
    pub fn new(value: i64) -> Self {
        Self(TxTime::new(value))
    }

    pub fn as_i64(self) -> i64 {
        self.0.as_i64()
    }

    pub fn as_tx_time(self) -> TxTime {
        self.0
    }
}

impl From<TxTime> for TransactionTime {
    fn from(value: TxTime) -> Self {
        Self(value)
    }
}

impl From<TransactionTime> for TxTime {
    fn from(value: TransactionTime) -> Self {
        value.0
    }
}

impl fmt::Display for TransactionTime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0.as_i64())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ValueOrEntity {
    Entity(EntityRef),
    Value(GraphValue),
}

impl ValueOrEntity {
    pub fn entity(id: impl Into<String>) -> Self {
        Self::Entity(EntityRef::new(id))
    }

    pub fn text(value: impl Into<String>) -> Self {
        Self::Value(GraphValue::Text(value.into()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClaimType {
    Observation,
    Assertion,
    AgentMemory,
    Event,
    Summary,
    Simulation,
    Hypothesis,
    Derived,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BeliefState {
    Candidate,
    Accepted,
    Disputed,
    Superseded,
    Retracted,
    Refuted,
    Simulated,
    Unknown,
}

impl BeliefState {
    pub fn ai_supported(&self) -> bool {
        matches!(self, Self::Accepted | Self::Disputed)
    }

    pub fn is_rejected_or_retired(&self) -> bool {
        matches!(self, Self::Superseded | Self::Retracted | Self::Refuted)
    }

    pub fn is_simulation(&self) -> bool {
        matches!(self, Self::Simulated)
    }

    pub fn is_uncertain(&self) -> bool {
        matches!(self, Self::Candidate | Self::Disputed | Self::Unknown)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceRef {
    pub source_id: SourceId,
    pub uri: Option<String>,
    pub content_hash: Option<String>,
}

impl SourceRef {
    pub fn new(source_id: SourceId) -> Self {
        Self {
            source_id,
            uri: None,
            content_hash: None,
        }
    }

    pub fn with_uri(mut self, uri: impl Into<String>) -> Self {
        self.uri = Some(uri.into());
        self
    }

    pub fn with_content_hash(mut self, content_hash: impl Into<String>) -> Self {
        self.content_hash = Some(content_hash.into());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceSpan {
    pub source_id: SourceId,
    pub start: usize,
    pub end: usize,
    pub quote: String,
}

impl EvidenceSpan {
    pub fn new(source_id: SourceId, start: usize, end: usize, quote: impl Into<String>) -> Self {
        Self {
            source_id,
            start,
            end,
            quote: quote.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractionTrace {
    pub extractor: String,
    pub rule_or_model: String,
}

impl ExtractionTrace {
    pub fn new(extractor: impl Into<String>, rule_or_model: impl Into<String>) -> Self {
        Self {
            extractor: extractor.into(),
            rule_or_model: rule_or_model.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PermissionLabel {
    Public,
    Internal,
    Restricted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TaintLabel {
    Trusted,
    Untrusted,
    PromptInjectionRisk,
    Poisoned,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AiUsage {
    SafeForPlanning { caveat: Option<String> },
    UseWithCaution(String),
    UnsafeForPlanning(String),
    SimulationOnly(String),
}

impl AiUsage {
    fn ai_supported(&self) -> bool {
        matches!(self, Self::SafeForPlanning { .. } | Self::UseWithCaution(_))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RealityAtom {
    pub id: AtomId,
    pub subject: EntityRef,
    pub predicate: PredicateId,
    pub object: ValueOrEntity,
    pub valid_time: TimeInterval<ValidTime>,
    pub transaction_time: TimeInterval<TxTime>,
    pub observed_time: Option<ValidTime>,
    pub claim_type: ClaimType,
    pub belief_state: BeliefState,
    pub confidence: Confidence,
    pub source_refs: Vec<SourceRef>,
    pub evidence_spans: Vec<EvidenceSpan>,
    pub extraction_trace: Option<ExtractionTrace>,
    pub context: KernelContextScope,
    pub tenant_id: TenantId,
    pub agent_scope: Option<AgentId>,
    pub dependencies: Vec<AtomId>,
    pub contradicts: Vec<AtomId>,
    pub supersedes: Vec<AtomId>,
    pub permissions: PermissionLabel,
    pub taint: TaintLabel,
    pub ai_usage: AiUsage,
}

impl RealityAtom {
    pub fn builder(
        id: AtomId,
        subject: EntityRef,
        predicate: PredicateId,
        object: ValueOrEntity,
    ) -> RealityAtomBuilder {
        RealityAtomBuilder::new(id, subject, predicate, object)
    }

    pub fn is_visible_at(&self, valid_at: ValidTime, known_at: TxTime) -> bool {
        self.valid_time.contains(valid_at) && self.transaction_time.contains(known_at)
    }

    pub fn is_supported_for_ai(&self) -> bool {
        self.belief_state.ai_supported()
            && !self.source_refs.is_empty()
            && !self.evidence_spans.is_empty()
            && self.ai_usage.ai_supported()
            && !matches!(
                self.claim_type,
                ClaimType::Simulation | ClaimType::Hypothesis
            )
            && !matches!(
                self.taint,
                TaintLabel::PromptInjectionRisk | TaintLabel::Poisoned
            )
    }

    pub fn with_belief_state(mut self, belief_state: BeliefState) -> Self {
        self.belief_state = belief_state;
        self
    }

    pub fn with_confidence(mut self, confidence: Confidence) -> Self {
        self.confidence = confidence;
        self
    }

    pub fn superseding(mut self, atom_ids: Vec<AtomId>) -> Self {
        self.supersedes = atom_ids;
        self
    }

    pub fn depending_on(mut self, atom_ids: Vec<AtomId>) -> Self {
        self.dependencies = atom_ids;
        self
    }
}

pub fn visible_at(atom: &RealityAtom, valid_at: ValidTime, known_at: TransactionTime) -> bool {
    atom.is_visible_at(valid_at, known_at.into())
}

pub fn known_at(atom: &RealityAtom, known_at: TransactionTime) -> bool {
    atom.transaction_time.contains(known_at.into())
}

pub fn active_during(atom: &RealityAtom, interval: &TimeInterval<ValidTime>) -> bool {
    atom.valid_time.overlaps(interval)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BeliefPolicy {
    AcceptedOnly,
    IncludeDisputed,
    IncludeSuperseded,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BeliefView {
    pub valid_at: ValidTime,
    pub known_at: TransactionTime,
    pub accepted_atoms: Vec<AtomId>,
    pub disputed_atoms: Vec<AtomId>,
    pub superseded_atoms: Vec<AtomId>,
    pub rejected_atoms: Vec<AtomId>,
    pub visible_atoms: Vec<AtomId>,
}

pub fn current_belief(
    atoms: &[RealityAtom],
    valid_at: ValidTime,
    known_at: TransactionTime,
    policy: BeliefPolicy,
) -> BeliefView {
    let mut accepted_atoms = Vec::new();
    let mut disputed_atoms = Vec::new();
    let mut superseded_atoms = Vec::new();
    let mut rejected_atoms = Vec::new();
    let mut visible_atoms = Vec::new();

    for atom in atoms
        .iter()
        .filter(|atom| visible_at(atom, valid_at, known_at))
    {
        match atom.belief_state {
            BeliefState::Accepted => {
                accepted_atoms.push(atom.id.clone());
                visible_atoms.push(atom.id.clone());
            }
            BeliefState::Disputed => {
                disputed_atoms.push(atom.id.clone());
                if matches!(
                    policy,
                    BeliefPolicy::IncludeDisputed | BeliefPolicy::IncludeSuperseded
                ) {
                    visible_atoms.push(atom.id.clone());
                }
            }
            BeliefState::Superseded => {
                superseded_atoms.push(atom.id.clone());
                if matches!(policy, BeliefPolicy::IncludeSuperseded) {
                    visible_atoms.push(atom.id.clone());
                }
            }
            BeliefState::Retracted | BeliefState::Refuted => {
                rejected_atoms.push(atom.id.clone());
            }
            BeliefState::Candidate | BeliefState::Simulated | BeliefState::Unknown => {}
        }
    }

    sort_and_dedup_atom_ids(&mut accepted_atoms);
    sort_and_dedup_atom_ids(&mut disputed_atoms);
    sort_and_dedup_atom_ids(&mut superseded_atoms);
    sort_and_dedup_atom_ids(&mut rejected_atoms);
    sort_and_dedup_atom_ids(&mut visible_atoms);

    BeliefView {
        valid_at,
        known_at,
        accepted_atoms,
        disputed_atoms,
        superseded_atoms,
        rejected_atoms,
        visible_atoms,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelContextScope {
    Global,
    Named(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RealityAtomBuilder {
    id: AtomId,
    subject: EntityRef,
    predicate: PredicateId,
    object: ValueOrEntity,
    valid_time: Option<TimeInterval<ValidTime>>,
    transaction_time: Option<TimeInterval<TxTime>>,
    observed_time: Option<ValidTime>,
    claim_type: ClaimType,
    belief_state: BeliefState,
    confidence: Option<Confidence>,
    source_refs: Vec<SourceRef>,
    evidence_spans: Vec<EvidenceSpan>,
    extraction_trace: Option<ExtractionTrace>,
    context: KernelContextScope,
    tenant_id: TenantId,
    agent_scope: Option<AgentId>,
    dependencies: Vec<AtomId>,
    contradicts: Vec<AtomId>,
    supersedes: Vec<AtomId>,
    permissions: PermissionLabel,
    taint: TaintLabel,
    ai_usage: AiUsage,
}

impl RealityAtomBuilder {
    fn new(id: AtomId, subject: EntityRef, predicate: PredicateId, object: ValueOrEntity) -> Self {
        Self {
            id,
            subject,
            predicate,
            object,
            valid_time: None,
            transaction_time: None,
            observed_time: None,
            claim_type: ClaimType::Assertion,
            belief_state: BeliefState::Candidate,
            confidence: None,
            source_refs: Vec::new(),
            evidence_spans: Vec::new(),
            extraction_trace: None,
            context: KernelContextScope::Global,
            tenant_id: TenantId::new("default"),
            agent_scope: None,
            dependencies: Vec::new(),
            contradicts: Vec::new(),
            supersedes: Vec::new(),
            permissions: PermissionLabel::Internal,
            taint: TaintLabel::Untrusted,
            ai_usage: AiUsage::UseWithCaution("no explicit AI usage policy".to_owned()),
        }
    }

    pub fn valid_time(mut self, valid_time: TimeInterval<ValidTime>) -> Self {
        self.valid_time = Some(valid_time);
        self
    }

    pub fn transaction_time(mut self, transaction_time: TimeInterval<TxTime>) -> Self {
        self.transaction_time = Some(transaction_time);
        self
    }

    pub fn observed_time(mut self, observed_time: ValidTime) -> Self {
        self.observed_time = Some(observed_time);
        self
    }

    pub fn claim_type(mut self, claim_type: ClaimType) -> Self {
        self.claim_type = claim_type;
        self
    }

    pub fn belief_state(mut self, belief_state: BeliefState) -> Self {
        self.belief_state = belief_state;
        self
    }

    pub fn confidence(mut self, confidence: Confidence) -> Self {
        self.confidence = Some(confidence);
        self
    }

    pub fn source_ref(mut self, source_ref: SourceRef) -> Self {
        self.source_refs.push(source_ref);
        self
    }

    pub fn evidence_span(mut self, evidence_span: EvidenceSpan) -> Self {
        self.evidence_spans.push(evidence_span);
        self
    }

    pub fn extraction_trace(mut self, extraction_trace: ExtractionTrace) -> Self {
        self.extraction_trace = Some(extraction_trace);
        self
    }

    pub fn context(mut self, context: KernelContextScope) -> Self {
        self.context = context;
        self
    }

    pub fn tenant_id(mut self, tenant_id: TenantId) -> Self {
        self.tenant_id = tenant_id;
        self
    }

    pub fn agent_scope(mut self, agent_id: AgentId) -> Self {
        self.agent_scope = Some(agent_id);
        self
    }

    pub fn dependencies(mut self, dependencies: Vec<AtomId>) -> Self {
        self.dependencies = dependencies;
        self
    }

    pub fn contradicts(mut self, contradicts: Vec<AtomId>) -> Self {
        self.contradicts = contradicts;
        self
    }

    pub fn supersedes(mut self, supersedes: Vec<AtomId>) -> Self {
        self.supersedes = supersedes;
        self
    }

    pub fn permissions(mut self, permissions: PermissionLabel) -> Self {
        self.permissions = permissions;
        self
    }

    pub fn taint(mut self, taint: TaintLabel) -> Self {
        self.taint = taint;
        self
    }

    pub fn ai_usage(mut self, ai_usage: AiUsage) -> Self {
        self.ai_usage = ai_usage;
        self
    }

    pub fn build(self) -> Result<RealityAtom, KernelError> {
        let valid_time = self.valid_time.ok_or(KernelError::MissingValidTime)?;
        let transaction_time = self
            .transaction_time
            .ok_or(KernelError::MissingTransactionTime)?;
        let confidence = self.confidence.ok_or(KernelError::MissingConfidence)?;
        if self.source_refs.is_empty() || self.evidence_spans.is_empty() {
            return Err(KernelError::MissingProvenance);
        }
        if matches!(self.claim_type, ClaimType::Derived) && self.dependencies.is_empty() {
            return Err(KernelError::MissingDependencies);
        }
        if matches!(self.claim_type, ClaimType::AgentMemory) && self.extraction_trace.is_none() {
            return Err(KernelError::MissingMemoryTrace);
        }
        if matches!(self.claim_type, ClaimType::Simulation)
            && !matches!(self.belief_state, BeliefState::Simulated)
        {
            return Err(KernelError::SimulationLabeledAsFact);
        }

        Ok(RealityAtom {
            id: self.id,
            subject: self.subject,
            predicate: self.predicate,
            object: self.object,
            valid_time,
            transaction_time,
            observed_time: self.observed_time,
            claim_type: self.claim_type,
            belief_state: self.belief_state,
            confidence,
            source_refs: self.source_refs,
            evidence_spans: self.evidence_spans,
            extraction_trace: self.extraction_trace,
            context: self.context,
            tenant_id: self.tenant_id,
            agent_scope: self.agent_scope,
            dependencies: self.dependencies,
            contradicts: self.contradicts,
            supersedes: self.supersedes,
            permissions: self.permissions,
            taint: self.taint,
            ai_usage: self.ai_usage,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelError {
    MissingValidTime,
    MissingTransactionTime,
    MissingProvenance,
    MissingConfidence,
    MissingDependencies,
    MissingMemoryTrace,
    SimulationLabeledAsFact,
    InvalidDependencyStrength,
    UnknownAtom(AtomId),
    MissingCausalEvidence,
    SelfCausation,
}

impl fmt::Display for KernelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingValidTime => formatter.write_str("reality atoms require valid_time"),
            Self::MissingTransactionTime => {
                formatter.write_str("reality atoms require transaction_time")
            }
            Self::MissingProvenance => formatter.write_str("reality atoms require provenance"),
            Self::MissingConfidence => formatter.write_str("reality atoms require confidence"),
            Self::MissingDependencies => {
                formatter.write_str("derived reality atoms require dependencies")
            }
            Self::MissingMemoryTrace => {
                formatter.write_str("agent memory atoms require an extraction/write trace")
            }
            Self::SimulationLabeledAsFact => {
                formatter.write_str("simulation atoms must not be labeled as fact")
            }
            Self::InvalidDependencyStrength => {
                formatter.write_str("dependency strength must be between 0.0 and 1.0")
            }
            Self::UnknownAtom(atom_id) => write!(formatter, "unknown atom {atom_id}"),
            Self::MissingCausalEvidence => {
                formatter.write_str("causal atoms require evidence source ids")
            }
            Self::SelfCausation => formatter.write_str("causal atoms cannot cause themselves"),
        }
    }
}

impl Error for KernelError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConflictType {
    ExactPredicateConflict,
    ValidTimeOverlap,
    MutuallyExclusiveClaim,
    SourceDisagreement,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConflictStatus {
    Unresolved,
    Preferred(AtomId),
    Resolved,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConflictSet {
    pub id: String,
    pub atom_ids: Vec<AtomId>,
    pub conflict_type: ConflictType,
    pub status: ConflictStatus,
    pub explanation: String,
}

impl ConflictSet {
    pub fn new(
        id: impl Into<String>,
        mut atom_ids: Vec<AtomId>,
        conflict_type: ConflictType,
        status: ConflictStatus,
        explanation: impl Into<String>,
    ) -> Self {
        atom_ids.sort();
        atom_ids.dedup();
        Self {
            id: id.into(),
            atom_ids,
            conflict_type,
            status,
            explanation: explanation.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BeliefRevision {
    pub atom_id: AtomId,
    pub known_at: TxTime,
    pub previous: BeliefState,
    pub next: BeliefState,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevisionReason {
    pub known_at: TransactionTime,
    pub description: String,
}

impl RevisionReason {
    pub fn new(known_at: TransactionTime, description: impl Into<String>) -> Self {
        Self {
            known_at,
            description: description.into(),
        }
    }
}

pub fn revise_belief(old_atom: AtomId, new_atom: AtomId, reason: RevisionReason) -> BeliefRevision {
    supersede_atom(old_atom, new_atom, reason)
}

pub fn supersede_atom(
    old_atom: AtomId,
    new_atom: AtomId,
    reason: RevisionReason,
) -> BeliefRevision {
    BeliefRevision {
        atom_id: old_atom,
        known_at: reason.known_at.into(),
        previous: BeliefState::Accepted,
        next: BeliefState::Superseded,
        reason: format!("{}; superseded by {new_atom}", reason.description),
    }
}

pub fn dispute_atom(atom_id: AtomId, reason: RevisionReason) -> BeliefRevision {
    BeliefRevision {
        atom_id,
        known_at: reason.known_at.into(),
        previous: BeliefState::Accepted,
        next: BeliefState::Disputed,
        reason: reason.description,
    }
}

pub fn retract_atom(atom_id: AtomId, reason: RevisionReason) -> BeliefRevision {
    BeliefRevision {
        atom_id,
        known_at: reason.known_at.into(),
        previous: BeliefState::Accepted,
        next: BeliefState::Retracted,
        reason: reason.description,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DependencyType {
    DerivedFrom,
    SupportedBy,
    ContradictedBy,
    SupersededBy,
    Assumes,
    Causes,
    Enables,
    Invalidates,
}

impl DependencyType {
    fn label(self) -> &'static str {
        match self {
            Self::DerivedFrom => "derives",
            Self::SupportedBy => "supports",
            Self::ContradictedBy => "contradicts",
            Self::SupersededBy => "supersedes",
            Self::Assumes => "assumes",
            Self::Causes => "causes",
            Self::Enables => "enables",
            Self::Invalidates => "invalidates",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DependencyEdge {
    pub from: AtomId,
    pub to: AtomId,
    pub dependency_type: DependencyType,
    pub strength: f32,
}

impl DependencyEdge {
    pub fn validate(&self) -> Result<(), KernelError> {
        if (0.0..=1.0).contains(&self.strength) {
            Ok(())
        } else {
            Err(KernelError::InvalidDependencyStrength)
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DependencyNode {
    Atom(AtomId),
    Answer(String),
    Simulation(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct DependencyStep {
    pub from: DependencyNode,
    pub to: DependencyNode,
    pub dependency_type: DependencyType,
    pub strength: f32,
    pub explanation: String,
}

#[derive(Clone, Debug, PartialEq)]
struct DependencyLink {
    dependency_type: DependencyType,
    strength: f32,
    explanation: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DependencyGraph {
    edges: BTreeMap<DependencyNode, BTreeMap<DependencyNode, DependencyLink>>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_dependency(
        &mut self,
        from: DependencyNode,
        to: DependencyNode,
        explanation: impl Into<String>,
    ) {
        self.insert_dependency(from, to, DependencyType::DerivedFrom, 1.0, explanation);
    }

    pub fn add_dependency_edge(&mut self, edge: DependencyEdge) -> Result<(), KernelError> {
        edge.validate()?;
        let explanation = format!("{} {} {}", edge.from, edge.dependency_type.label(), edge.to);
        self.insert_dependency(
            DependencyNode::Atom(edge.from),
            DependencyNode::Atom(edge.to),
            edge.dependency_type,
            edge.strength,
            explanation,
        );
        Ok(())
    }

    pub fn add_typed_dependency(
        &mut self,
        from: DependencyNode,
        to: DependencyNode,
        dependency_type: DependencyType,
        strength: f32,
        explanation: impl Into<String>,
    ) -> Result<(), KernelError> {
        if !(0.0..=1.0).contains(&strength) {
            return Err(KernelError::InvalidDependencyStrength);
        }
        self.insert_dependency(from, to, dependency_type, strength, explanation);
        Ok(())
    }

    fn insert_dependency(
        &mut self,
        from: DependencyNode,
        to: DependencyNode,
        dependency_type: DependencyType,
        strength: f32,
        explanation: impl Into<String>,
    ) {
        self.edges.entry(from).or_default().insert(
            to,
            DependencyLink {
                dependency_type,
                strength,
                explanation: explanation.into(),
            },
        );
    }

    pub fn transitive_dependents(&self, root: &DependencyNode) -> Vec<DependencyNode> {
        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::new();
        let mut dependents = Vec::new();
        if let Some(edges) = self.edges.get(root) {
            for node in edges.keys() {
                queue.push_back(node.clone());
            }
        }
        while let Some(node) = queue.pop_front() {
            if !seen.insert(node.clone()) {
                continue;
            }
            dependents.push(node.clone());
            if let Some(edges) = self.edges.get(&node) {
                for next in edges.keys() {
                    queue.push_back(next.clone());
                }
            }
        }
        dependents.sort();
        dependents
    }

    pub fn trace_from(&self, root: &DependencyNode) -> Vec<DependencyStep> {
        let mut steps = Vec::new();
        let mut queue = VecDeque::from([root.clone()]);
        let mut seen = BTreeSet::new();
        while let Some(node) = queue.pop_front() {
            if !seen.insert(node.clone()) {
                continue;
            }
            if let Some(edges) = self.edges.get(&node) {
                for (to, link) in edges {
                    steps.push(DependencyStep {
                        from: node.clone(),
                        to: to.clone(),
                        dependency_type: link.dependency_type,
                        strength: link.strength,
                        explanation: link.explanation.clone(),
                    });
                    queue.push_back(to.clone());
                }
            }
        }
        steps.sort_by(|left, right| {
            left.from
                .cmp(&right.from)
                .then_with(|| left.to.cmp(&right.to))
        });
        steps
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct InvalidationReport {
    pub root: DependencyNode,
    pub reason: String,
    pub invalidated_nodes: Vec<DependencyNode>,
    pub steps: Vec<DependencyStep>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TruthMaintenance {
    graph: DependencyGraph,
}

impl TruthMaintenance {
    pub fn new(graph: DependencyGraph) -> Self {
        Self { graph }
    }

    pub fn invalidate(
        &self,
        root: DependencyNode,
        reason: impl Into<String>,
    ) -> InvalidationReport {
        InvalidationReport {
            invalidated_nodes: self.graph.transitive_dependents(&root),
            steps: self.graph.trace_from(&root),
            root,
            reason: reason.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SupportSet {
    pub atom_id: AtomId,
    pub supporting_atoms: Vec<AtomId>,
    pub source_ids: Vec<SourceId>,
    pub evidence: Vec<EvidenceSpan>,
    pub dependency_trace: Vec<DependencyStep>,
}

pub type InvalidationTrace = InvalidationReport;

#[derive(Clone, Debug, PartialEq)]
pub struct ImpactCone {
    pub root: AtomId,
    pub impacted_atoms: Vec<AtomId>,
    pub impacted_answers: Vec<String>,
    pub impacted_simulations: Vec<String>,
    pub invalidation_trace: InvalidationTrace,
    pub warning: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EntityState {
    pub entity: EntityRef,
    pub valid_at: ValidTime,
    pub known_at: TxTime,
    pub accepted_atoms: Vec<RealityAtom>,
    pub disputed_atoms: Vec<RealityAtom>,
    pub superseded_atoms: Vec<RealityAtom>,
    pub conflicts: Vec<ConflictSet>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtomImpactReport {
    pub root: AtomId,
    pub impacted_atoms: Vec<AtomId>,
    pub impacted_answers: Vec<String>,
    pub impacted_simulations: Vec<String>,
    pub warning: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TruthCollapseReport {
    pub root_source: AtomId,
    pub collapsed_atoms: Vec<AtomId>,
    pub collapsed_beliefs: Vec<AtomId>,
    pub collapsed_memories: Vec<AtomId>,
    pub collapsed_plans: Vec<AtomId>,
    pub collapsed_answers: Vec<String>,
    pub collapsed_simulations: Vec<String>,
    pub dependency_steps: Vec<DependencyStep>,
    pub warning: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CausalAtom {
    pub cause: EventId,
    pub effect: EventId,
    pub mechanism: Option<String>,
    pub lag: Option<Duration>,
    pub confidence: Confidence,
    pub evidence: Vec<SourceId>,
    pub counterfactual_notes: Vec<String>,
}

impl CausalAtom {
    pub fn validate(&self) -> Result<(), KernelError> {
        if self.cause == self.effect {
            return Err(KernelError::SelfCausation);
        }
        if self.evidence.is_empty() {
            return Err(KernelError::MissingCausalEvidence);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CausalPath {
    pub start: EventId,
    pub end: EventId,
    pub atoms: Vec<CausalAtom>,
    pub confidence: Confidence,
    pub mechanisms: Vec<String>,
    pub evidence: Vec<SourceId>,
    pub counterfactual_notes: Vec<String>,
}

impl CausalPath {
    pub fn event_ids(&self) -> Vec<EventId> {
        let mut ids = Vec::new();
        ids.push(self.start.clone());
        ids.extend(self.atoms.iter().map(|atom| atom.effect.clone()));
        ids
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CausalImpactReport {
    pub intervention: EventId,
    pub affected_events: Vec<EventId>,
    pub affected_paths: Vec<CausalPath>,
    pub downstream_risks: Vec<String>,
    pub counterfactual_notes: Vec<String>,
    pub warning: String,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SelfRevisionJob {
    EntityDeduplication,
    SourceTrustRecalibration,
    OntologyDriftDetection,
    ContradictionClustering,
    SummaryInvalidation,
    MemoryConsolidation,
    StaleBeliefDetection,
    DependencyInvalidation,
    CausalHypothesisRefinement,
}

impl SelfRevisionJob {
    pub fn all() -> Vec<Self> {
        vec![
            Self::EntityDeduplication,
            Self::SourceTrustRecalibration,
            Self::OntologyDriftDetection,
            Self::ContradictionClustering,
            Self::SummaryInvalidation,
            Self::MemoryConsolidation,
            Self::StaleBeliefDetection,
            Self::DependencyInvalidation,
            Self::CausalHypothesisRefinement,
        ]
    }

    fn slug(self) -> &'static str {
        match self {
            Self::EntityDeduplication => "entity-deduplication",
            Self::SourceTrustRecalibration => "source-trust-recalibration",
            Self::OntologyDriftDetection => "ontology-drift-detection",
            Self::ContradictionClustering => "contradiction-clustering",
            Self::SummaryInvalidation => "summary-invalidation",
            Self::MemoryConsolidation => "memory-consolidation",
            Self::StaleBeliefDetection => "stale-belief-detection",
            Self::DependencyInvalidation => "dependency-invalidation",
            Self::CausalHypothesisRefinement => "causal-hypothesis-refinement",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelfRevisionCursor {
    pub after_tx: Option<TxTime>,
}

impl SelfRevisionCursor {
    pub fn from_tx(after_tx: TxTime) -> Self {
        Self {
            after_tx: Some(after_tx),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelfRevisionPolicy {
    pub run_at: TxTime,
    pub stale_tx_lag: i64,
    pub low_confidence_threshold: f32,
    pub known_predicates: BTreeSet<PredicateId>,
}

impl SelfRevisionPolicy {
    pub fn review_only(run_at: TxTime) -> Self {
        Self {
            run_at,
            stale_tx_lag: 10_000,
            low_confidence_threshold: 0.35,
            known_predicates: BTreeSet::new(),
        }
    }

    pub fn with_stale_tx_lag(mut self, stale_tx_lag: i64) -> Self {
        self.stale_tx_lag = stale_tx_lag.max(0);
        self
    }

    pub fn with_low_confidence_threshold(mut self, threshold: f32) -> Self {
        self.low_confidence_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    pub fn with_known_predicates(mut self, predicates: Vec<PredicateId>) -> Self {
        self.known_predicates = predicates.into_iter().collect();
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SelfRevisionReviewStatus {
    Pending,
    Approved,
    Rejected,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SelfRevisionSuggestionKind {
    SuggestEntityDeduplication,
    RecalibrateSourceTrust,
    FlagOntologyDrift,
    ClusterContradictions,
    InvalidateSummary,
    ConsolidateMemory,
    MarkStaleBelief,
    InvalidateDependencies,
    RefineCausalHypothesis,
}

impl SelfRevisionSuggestionKind {
    fn slug(self) -> &'static str {
        match self {
            Self::SuggestEntityDeduplication => "suggest-entity-deduplication",
            Self::RecalibrateSourceTrust => "recalibrate-source-trust",
            Self::FlagOntologyDrift => "flag-ontology-drift",
            Self::ClusterContradictions => "cluster-contradictions",
            Self::InvalidateSummary => "invalidate-summary",
            Self::ConsolidateMemory => "consolidate-memory",
            Self::MarkStaleBelief => "mark-stale-belief",
            Self::InvalidateDependencies => "invalidate-dependencies",
            Self::RefineCausalHypothesis => "refine-causal-hypothesis",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SelfRevisionTarget {
    EntityPair {
        left: EntityRef,
        right: EntityRef,
    },
    Source(SourceId),
    Predicate(PredicateId),
    Conflict(String),
    Summary(AtomId),
    MemorySet {
        agent_id: AgentId,
        atom_ids: Vec<AtomId>,
    },
    Atom(AtomId),
    DependencyRoot(AtomId),
    CausalHypothesis {
        cause: EventId,
        effect: EventId,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelfRevisionSuggestion {
    pub id: String,
    pub job: SelfRevisionJob,
    pub kind: SelfRevisionSuggestionKind,
    pub target: SelfRevisionTarget,
    pub requires_review: bool,
    pub destructive_if_applied: bool,
    pub auto_applied: bool,
    pub audit_event_id: String,
    pub explanation: String,
    pub evidence: Vec<String>,
    pub dependency_trace: Vec<DependencyStep>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelfRevisionAuditEntry {
    pub id: String,
    pub at: TxTime,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelfRevisionReport {
    pub id: String,
    pub jobs: Vec<SelfRevisionJob>,
    pub run_at: TxTime,
    pub cursor: SelfRevisionCursor,
    pub next_cursor: SelfRevisionCursor,
    pub incremental: bool,
    pub review_status: SelfRevisionReviewStatus,
    pub suggestions: Vec<SelfRevisionSuggestion>,
    pub audit_log: Vec<SelfRevisionAuditEntry>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelfRevisionEngine {
    policy: SelfRevisionPolicy,
}

impl SelfRevisionEngine {
    pub fn new(policy: SelfRevisionPolicy) -> Self {
        Self { policy }
    }

    pub fn run_all(
        &self,
        kernel: &RealityKernel,
        cursor: SelfRevisionCursor,
    ) -> SelfRevisionReport {
        self.report(SelfRevisionJob::all(), kernel, cursor)
    }

    pub fn run_job(
        &self,
        job: SelfRevisionJob,
        kernel: &RealityKernel,
        cursor: SelfRevisionCursor,
    ) -> SelfRevisionReport {
        self.report(vec![job], kernel, cursor)
    }

    fn report(
        &self,
        jobs: Vec<SelfRevisionJob>,
        kernel: &RealityKernel,
        cursor: SelfRevisionCursor,
    ) -> SelfRevisionReport {
        let mut suggestions = Vec::new();
        for job in &jobs {
            suggestions.extend(self.suggestions_for_job(*job, kernel, cursor));
        }
        sort_self_revision_suggestions(&mut suggestions);

        let id = self_revision_report_id(&jobs, self.policy.run_at);
        let mut audit_log = vec![SelfRevisionAuditEntry {
            id: format!("audit-{id}"),
            at: self.policy.run_at,
            message: format!(
                "self-revision jobs {} ran with incremental cursor {:?}; suggestions only, no truth rewrite applied",
                jobs.iter()
                    .map(|job| job.slug())
                    .collect::<Vec<_>>()
                    .join(","),
                cursor.after_tx.map(TxTime::as_i64)
            ),
        }];
        audit_log.extend(suggestions.iter().map(|suggestion| SelfRevisionAuditEntry {
            id: suggestion.audit_event_id.clone(),
            at: self.policy.run_at,
            message: format!(
                "suggested {} for {}; requires review before any graph mutation",
                suggestion.kind.slug(),
                self_revision_target_slug(&suggestion.target)
            ),
        }));

        SelfRevisionReport {
            id,
            jobs,
            run_at: self.policy.run_at,
            cursor,
            next_cursor: SelfRevisionCursor::from_tx(self.policy.run_at),
            incremental: true,
            review_status: SelfRevisionReviewStatus::Pending,
            suggestions,
            audit_log,
        }
    }

    fn suggestions_for_job(
        &self,
        job: SelfRevisionJob,
        kernel: &RealityKernel,
        cursor: SelfRevisionCursor,
    ) -> Vec<SelfRevisionSuggestion> {
        match job {
            SelfRevisionJob::EntityDeduplication => self.entity_deduplication(kernel, cursor),
            SelfRevisionJob::SourceTrustRecalibration => {
                self.source_trust_recalibration(kernel, cursor)
            }
            SelfRevisionJob::OntologyDriftDetection => {
                self.ontology_drift_detection(kernel, cursor)
            }
            SelfRevisionJob::ContradictionClustering => {
                self.contradiction_clustering(kernel, cursor)
            }
            SelfRevisionJob::SummaryInvalidation => self.summary_invalidation(kernel, cursor),
            SelfRevisionJob::MemoryConsolidation => self.memory_consolidation(kernel, cursor),
            SelfRevisionJob::StaleBeliefDetection => self.stale_belief_detection(kernel, cursor),
            SelfRevisionJob::DependencyInvalidation => self.dependency_invalidation(kernel, cursor),
            SelfRevisionJob::CausalHypothesisRefinement => {
                self.causal_hypothesis_refinement(kernel)
            }
        }
    }

    fn entity_deduplication(
        &self,
        kernel: &RealityKernel,
        cursor: SelfRevisionCursor,
    ) -> Vec<SelfRevisionSuggestion> {
        let mut buckets: BTreeMap<(TenantId, String), Vec<&RealityAtom>> = BTreeMap::new();
        for atom in kernel.atoms.values() {
            if atom.predicate.as_str() != "ENTITY_NAME"
                || !self_revision_changed_since(atom.transaction_time.start, cursor)
            {
                continue;
            }
            if let Some(name) = self_revision_text_value(atom) {
                buckets
                    .entry((atom.tenant_id.clone(), normalize_self_revision_text(&name)))
                    .or_default()
                    .push(atom);
            }
        }

        let mut suggestions = Vec::new();
        for atoms in buckets.values_mut() {
            atoms.sort_by(|left, right| left.subject.cmp(&right.subject));
            for left_index in 0..atoms.len() {
                for right in atoms.iter().skip(left_index + 1) {
                    if atoms[left_index].subject == right.subject {
                        continue;
                    }
                    suggestions.push(self_revision_suggestion(
                        SelfRevisionJob::EntityDeduplication,
                        SelfRevisionSuggestionKind::SuggestEntityDeduplication,
                        SelfRevisionTarget::EntityPair {
                            left: atoms[left_index].subject.clone(),
                            right: right.subject.clone(),
                        },
                        true,
                        "possible duplicate entities share normalized source-backed name; no merge was applied".to_owned(),
                        vec![
                            atoms[left_index].id.to_string(),
                            right.id.to_string(),
                            format!(
                                "normalized_name={}",
                                normalize_self_revision_text(
                                    &self_revision_text_value(atoms[left_index])
                                        .unwrap_or_default()
                                )
                            ),
                        ],
                        Vec::new(),
                    ));
                }
            }
        }
        suggestions
    }

    fn source_trust_recalibration(
        &self,
        kernel: &RealityKernel,
        cursor: SelfRevisionCursor,
    ) -> Vec<SelfRevisionSuggestion> {
        let conflicted_atoms = conflicted_atom_ids(kernel);
        let mut source_evidence: BTreeMap<SourceId, Vec<String>> = BTreeMap::new();
        for atom in kernel.atoms.values() {
            if !self_revision_changed_since(atom.transaction_time.start, cursor) {
                continue;
            }
            let needs_recalibration = conflicted_atoms.contains(&atom.id)
                || atom.confidence.as_f32() <= self.policy.low_confidence_threshold
                || !matches!(atom.taint, TaintLabel::Trusted);
            if !needs_recalibration {
                continue;
            }
            for source_ref in &atom.source_refs {
                let evidence = source_evidence
                    .entry(source_ref.source_id.clone())
                    .or_default();
                evidence.push(format!("atom={}", atom.id));
                evidence.push(format!("confidence={:.2}", atom.confidence.as_f32()));
                evidence.push(format!("taint={:?}", atom.taint));
            }
        }

        source_evidence
            .into_iter()
            .map(|(source_id, mut evidence)| {
                evidence.sort();
                evidence.dedup();
                self_revision_suggestion(
                    SelfRevisionJob::SourceTrustRecalibration,
                    SelfRevisionSuggestionKind::RecalibrateSourceTrust,
                    SelfRevisionTarget::Source(source_id.clone()),
                    false,
                    format!(
                        "source {source_id} participates in low-confidence, tainted, or contradicted atoms; recalibrate trust through a reviewable event"
                    ),
                    evidence,
                    Vec::new(),
                )
            })
            .collect()
    }

    fn ontology_drift_detection(
        &self,
        kernel: &RealityKernel,
        cursor: SelfRevisionCursor,
    ) -> Vec<SelfRevisionSuggestion> {
        if self.policy.known_predicates.is_empty() {
            return Vec::new();
        }

        let mut predicates: BTreeMap<PredicateId, Vec<AtomId>> = BTreeMap::new();
        for atom in kernel.atoms.values() {
            if self.policy.known_predicates.contains(&atom.predicate)
                || !self_revision_changed_since(atom.transaction_time.start, cursor)
            {
                continue;
            }
            predicates
                .entry(atom.predicate.clone())
                .or_default()
                .push(atom.id.clone());
        }

        predicates
            .into_iter()
            .map(|(predicate, mut atom_ids)| {
                sort_and_dedup_atom_ids(&mut atom_ids);
                self_revision_suggestion(
                    SelfRevisionJob::OntologyDriftDetection,
                    SelfRevisionSuggestionKind::FlagOntologyDrift,
                    SelfRevisionTarget::Predicate(predicate.clone()),
                    false,
                    format!(
                        "predicate {predicate} is not in the approved kernel predicate set; propose ontology review instead of auto-promoting it"
                    ),
                    atom_ids.into_iter().map(|atom_id| atom_id.to_string()).collect(),
                    Vec::new(),
                )
            })
            .collect()
    }

    fn contradiction_clustering(
        &self,
        kernel: &RealityKernel,
        cursor: SelfRevisionCursor,
    ) -> Vec<SelfRevisionSuggestion> {
        kernel
            .conflicts
            .iter()
            .filter(|conflict| {
                conflict.status == ConflictStatus::Unresolved
                    && conflict.atom_ids.iter().any(|atom_id| {
                        kernel
                            .atom(atom_id)
                            .is_some_and(|atom| {
                                self_revision_changed_since(atom.transaction_time.start, cursor)
                            })
                    })
            })
            .map(|conflict| {
                self_revision_suggestion(
                    SelfRevisionJob::ContradictionClustering,
                    SelfRevisionSuggestionKind::ClusterContradictions,
                    SelfRevisionTarget::Conflict(conflict.id.clone()),
                    false,
                    format!(
                        "conflict {} should be clustered for belief review; competing claims remain preserved",
                        conflict.id
                    ),
                    vec![
                        format!("{:?}", conflict.conflict_type),
                        conflict.explanation.clone(),
                    ],
                    conflict
                        .atom_ids
                        .first()
                        .map(|atom_id| {
                            kernel
                                .dependencies
                                .trace_from(&DependencyNode::Atom(atom_id.clone()))
                        })
                        .unwrap_or_default(),
                )
            })
            .collect()
    }

    fn summary_invalidation(
        &self,
        kernel: &RealityKernel,
        cursor: SelfRevisionCursor,
    ) -> Vec<SelfRevisionSuggestion> {
        let conflicted_atoms = conflicted_atom_ids(kernel);
        kernel
            .atoms
            .values()
            .filter(|atom| matches!(atom.claim_type, ClaimType::Summary))
            .filter(|summary| {
                self_revision_changed_since(summary.transaction_time.start, cursor)
                    || summary.dependencies.iter().any(|dependency| {
                        conflicted_atoms.contains(dependency)
                            || !kernel.belief_revisions(dependency).is_empty()
                            || kernel
                                .atom(dependency)
                                .is_some_and(|atom| {
                                    !matches!(atom.belief_state, BeliefState::Accepted)
                                })
                    })
            })
            .map(|summary| {
                self_revision_suggestion(
                    SelfRevisionJob::SummaryInvalidation,
                    SelfRevisionSuggestionKind::InvalidateSummary,
                    SelfRevisionTarget::Summary(summary.id.clone()),
                    false,
                    format!(
                        "summary {} depends on changed, revised, or disputed atoms; mark stale through an auditable invalidation",
                        summary.id
                    ),
                    summary
                        .dependencies
                        .iter()
                        .map(|atom_id| atom_id.to_string())
                        .collect(),
                    summary
                        .dependencies
                        .first()
                        .map(|atom_id| {
                            kernel
                                .dependencies
                                .trace_from(&DependencyNode::Atom(atom_id.clone()))
                        })
                        .unwrap_or_default(),
                )
            })
            .collect()
    }

    fn memory_consolidation(
        &self,
        kernel: &RealityKernel,
        cursor: SelfRevisionCursor,
    ) -> Vec<SelfRevisionSuggestion> {
        let mut buckets: BTreeMap<(AgentId, String), Vec<AtomId>> = BTreeMap::new();
        for atom in kernel.atoms.values() {
            if !matches!(atom.claim_type, ClaimType::AgentMemory)
                || !self_revision_changed_since(atom.transaction_time.start, cursor)
            {
                continue;
            }
            let Some(agent_id) = atom.agent_scope.clone() else {
                continue;
            };
            if let Some(content) = self_revision_text_value(atom) {
                buckets
                    .entry((agent_id, normalize_self_revision_text(&content)))
                    .or_default()
                    .push(atom.id.clone());
            }
        }

        buckets
            .into_iter()
            .filter_map(|((agent_id, _), mut atom_ids)| {
                sort_and_dedup_atom_ids(&mut atom_ids);
                if atom_ids.len() < 2 {
                    return None;
                }
                Some(self_revision_suggestion(
                    SelfRevisionJob::MemoryConsolidation,
                    SelfRevisionSuggestionKind::ConsolidateMemory,
                    SelfRevisionTarget::MemorySet {
                        agent_id: agent_id.clone(),
                        atom_ids: atom_ids.clone(),
                    },
                    false,
                    format!(
                        "agent {agent_id} has duplicate or near-identical memory atoms; propose consolidation without deleting history"
                    ),
                    atom_ids.into_iter().map(|atom_id| atom_id.to_string()).collect(),
                    Vec::new(),
                ))
            })
            .collect()
    }

    fn stale_belief_detection(
        &self,
        kernel: &RealityKernel,
        cursor: SelfRevisionCursor,
    ) -> Vec<SelfRevisionSuggestion> {
        kernel
            .atoms
            .values()
            .filter(|atom| {
                matches!(
                    atom.belief_state,
                    BeliefState::Candidate | BeliefState::Disputed | BeliefState::Unknown
                ) && self_revision_changed_since(atom.transaction_time.start, cursor)
                    && self.policy.run_at.as_i64() - atom.transaction_time.start.as_i64()
                        >= self.policy.stale_tx_lag
            })
            .map(|atom| {
                self_revision_suggestion(
                    SelfRevisionJob::StaleBeliefDetection,
                    SelfRevisionSuggestionKind::MarkStaleBelief,
                    SelfRevisionTarget::Atom(atom.id.clone()),
                    false,
                    format!(
                        "belief {} has remained {:?} for at least {} transaction ticks; suggest review, not retraction",
                        atom.id, atom.belief_state, self.policy.stale_tx_lag
                    ),
                    vec![
                        format!("tx_start={}", atom.transaction_time.start.as_i64()),
                        format!("run_at={}", self.policy.run_at.as_i64()),
                    ],
                    kernel
                        .dependencies
                        .trace_from(&DependencyNode::Atom(atom.id.clone())),
                )
            })
            .collect()
    }

    fn dependency_invalidation(
        &self,
        kernel: &RealityKernel,
        cursor: SelfRevisionCursor,
    ) -> Vec<SelfRevisionSuggestion> {
        kernel
            .atoms
            .values()
            .filter(|atom| self_revision_changed_since(atom.transaction_time.start, cursor))
            .filter_map(|atom| {
                let impacted_nodes = kernel
                    .dependencies
                    .transitive_dependents(&DependencyNode::Atom(atom.id.clone()));
                if impacted_nodes.is_empty()
                    || matches!(atom.belief_state, BeliefState::Accepted)
                {
                    return None;
                }
                let dependency_trace = kernel
                    .dependencies
                    .trace_from(&DependencyNode::Atom(atom.id.clone()));
                Some(self_revision_suggestion(
                    SelfRevisionJob::DependencyInvalidation,
                    SelfRevisionSuggestionKind::InvalidateDependencies,
                    SelfRevisionTarget::DependencyRoot(atom.id.clone()),
                    false,
                    format!(
                        "atom {} is {:?}; downstream beliefs, memories, summaries, plans, answers, or simulations need review",
                        atom.id, atom.belief_state
                    ),
                    impacted_nodes
                        .into_iter()
                        .map(|node| format!("{node:?}"))
                        .collect(),
                    dependency_trace,
                ))
            })
            .collect()
    }

    fn causal_hypothesis_refinement(&self, kernel: &RealityKernel) -> Vec<SelfRevisionSuggestion> {
        let mut suggestions = Vec::new();
        for atoms in kernel.causal_outgoing.values() {
            for atom in atoms {
                let needs_refinement = atom.confidence.as_f32()
                    <= self.policy.low_confidence_threshold
                    || atom
                        .mechanism
                        .as_ref()
                        .map_or(true, |mechanism| mechanism.trim().is_empty())
                    || atom.counterfactual_notes.is_empty();
                if !needs_refinement {
                    continue;
                }
                suggestions.push(self_revision_suggestion(
                    SelfRevisionJob::CausalHypothesisRefinement,
                    SelfRevisionSuggestionKind::RefineCausalHypothesis,
                    SelfRevisionTarget::CausalHypothesis {
                        cause: atom.cause.clone(),
                        effect: atom.effect.clone(),
                    },
                    false,
                    format!(
                        "causal hypothesis {} -> {} needs refinement before strategic use; simulation must not be labeled as fact",
                        atom.cause, atom.effect
                    ),
                    vec![
                        format!("confidence={:.2}", atom.confidence.as_f32()),
                        format!("mechanism={}", atom.mechanism.clone().unwrap_or_default()),
                    ],
                    Vec::new(),
                ));
            }
        }
        suggestions
    }
}

const OPEN_INTERVAL_END: i64 = i64::MAX;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AtomOrdinal(u32);

impl AtomOrdinal {
    pub fn new(value: u32) -> Self {
        Self(value)
    }

    pub fn as_usize(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CandidateBitmap {
    ordinals: Vec<AtomOrdinal>,
}

impl CandidateBitmap {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_unsorted(ordinals: impl IntoIterator<Item = AtomOrdinal>) -> Self {
        let mut ordinals = ordinals.into_iter().collect::<Vec<_>>();
        ordinals.sort();
        ordinals.dedup();
        Self { ordinals }
    }

    pub fn all(count: usize) -> Self {
        Self::from_unsorted((0..count).map(|ordinal| AtomOrdinal::new(ordinal as u32)))
    }

    pub fn len(&self) -> usize {
        self.ordinals.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ordinals.is_empty()
    }

    pub fn ordinals(&self) -> &[AtomOrdinal] {
        &self.ordinals
    }

    pub fn intersect(&self, other: &Self) -> Self {
        let mut left_index = 0;
        let mut right_index = 0;
        let mut intersection = Vec::new();
        while left_index < self.ordinals.len() && right_index < other.ordinals.len() {
            let left = self.ordinals[left_index];
            let right = other.ordinals[right_index];
            if left == right {
                intersection.push(left);
                left_index += 1;
                right_index += 1;
            } else if left < right {
                left_index += 1;
            } else {
                right_index += 1;
            }
        }
        Self {
            ordinals: intersection,
        }
    }

    pub fn union(&self, other: &Self) -> Self {
        let mut union = self.ordinals.clone();
        union.extend(other.ordinals.iter().copied());
        Self::from_unsorted(union)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColumnarAtomStore {
    pub atom_ids: Vec<AtomId>,
    pub subject_ids: Vec<u32>,
    pub predicate_ids: Vec<u32>,
    pub object_ids: Vec<u32>,
    pub valid_from: Vec<i64>,
    pub valid_to: Vec<i64>,
    pub tx_from: Vec<i64>,
    pub tx_to: Vec<i64>,
    pub confidence: Vec<f32>,
    pub belief_state: Vec<BeliefState>,
    pub context_ids: Vec<u32>,
    pub source_set_ids: Vec<u32>,
}

impl ColumnarAtomStore {
    pub fn is_dense(&self) -> bool {
        let len = self.atom_ids.len();
        self.subject_ids.len() == len
            && self.predicate_ids.len() == len
            && self.object_ids.len() == len
            && self.valid_from.len() == len
            && self.valid_to.len() == len
            && self.tx_from.len() == len
            && self.tx_to.len() == len
            && self.confidence.len() == len
            && self.belief_state.len() == len
            && self.context_ids.len() == len
            && self.source_set_ids.len() == len
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhysicalLayoutKind {
    AppendOnlyEventLog,
    ColumnarAtomStore,
    CompressedAdjacencyLists,
    TemporalIntervalIndexes,
    RoaringBitmapCandidateSets,
    TrieJoinIndexes,
    MemoryMappedSnapshots,
    HotWorkingSetCache,
    ColdHistoricalSegmentStore,
    VectorSourceSidecar,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MemoryMappedSnapshotDescriptor {
    pub format: String,
    pub atom_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ColdHistoricalSegmentDescriptor {
    pub segment_id: String,
    pub min_tx: i64,
    pub max_tx: i64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VectorSourceSidecarDescriptor {
    pub source_sets: usize,
    pub external_vectors: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PhysicalGraphStore {
    atoms: Vec<RealityAtom>,
    columnar: ColumnarAtomStore,
    entity_symbols: BTreeMap<EntityRef, u32>,
    predicate_symbols: BTreeMap<PredicateId, u32>,
    object_symbols: BTreeMap<String, u32>,
    source_symbols: BTreeMap<SourceId, u32>,
    context_symbols: BTreeMap<String, u32>,
    source_sets: Vec<Vec<SourceId>>,
    outgoing_by_subject: BTreeMap<u32, CandidateBitmap>,
    incoming_by_object: BTreeMap<u32, CandidateBitmap>,
    atoms_by_predicate: BTreeMap<u32, CandidateBitmap>,
    atoms_by_source: BTreeMap<u32, CandidateBitmap>,
    valid_start_index: BTreeMap<i64, CandidateBitmap>,
    tx_start_index: BTreeMap<i64, CandidateBitmap>,
    trie_spo: BTreeMap<(u32, u32, u32), CandidateBitmap>,
    contradiction_clusters: BTreeMap<String, CandidateBitmap>,
    dependency_atoms: BTreeMap<AtomOrdinal, CandidateBitmap>,
    hot_working_set: CandidateBitmap,
    cold_segments: Vec<ColdHistoricalSegmentDescriptor>,
    snapshot: MemoryMappedSnapshotDescriptor,
    sidecar: VectorSourceSidecarDescriptor,
}

impl PhysicalGraphStore {
    pub fn from_atoms(atoms: impl IntoIterator<Item = RealityAtom>) -> Self {
        let mut store = Self::default();
        let mut atoms = atoms.into_iter().collect::<Vec<_>>();
        sort_atoms(&mut atoms);
        store.snapshot.format = "rg-kernel-columnar-v1".to_owned();
        store.snapshot.atom_count = atoms.len();
        store.sidecar.external_vectors = true;

        for atom in atoms {
            store.push_atom(atom);
        }
        store.finalize_indexes();
        store
    }

    pub fn layout_manifest() -> Vec<PhysicalLayoutKind> {
        vec![
            PhysicalLayoutKind::AppendOnlyEventLog,
            PhysicalLayoutKind::ColumnarAtomStore,
            PhysicalLayoutKind::CompressedAdjacencyLists,
            PhysicalLayoutKind::TemporalIntervalIndexes,
            PhysicalLayoutKind::RoaringBitmapCandidateSets,
            PhysicalLayoutKind::TrieJoinIndexes,
            PhysicalLayoutKind::MemoryMappedSnapshots,
            PhysicalLayoutKind::HotWorkingSetCache,
            PhysicalLayoutKind::ColdHistoricalSegmentStore,
            PhysicalLayoutKind::VectorSourceSidecar,
        ]
    }

    pub fn atom_count(&self) -> usize {
        self.columnar.atom_ids.len()
    }

    pub fn columnar(&self) -> &ColumnarAtomStore {
        &self.columnar
    }

    pub fn outgoing_for_subject(&self, subject: &EntityRef) -> CandidateBitmap {
        self.entity_symbols
            .get(subject)
            .and_then(|symbol| self.outgoing_by_subject.get(symbol))
            .cloned()
            .unwrap_or_default()
    }

    pub fn incoming_for_object_entity(&self, entity: &EntityRef) -> CandidateBitmap {
        let key = object_symbol_key(&ValueOrEntity::Entity(entity.clone()));
        self.object_symbols
            .get(&key)
            .and_then(|symbol| self.incoming_by_object.get(symbol))
            .cloned()
            .unwrap_or_default()
    }

    pub fn atoms_for_predicate(&self, predicate: &PredicateId) -> CandidateBitmap {
        self.predicate_symbols
            .get(predicate)
            .and_then(|symbol| self.atoms_by_predicate.get(symbol))
            .cloned()
            .unwrap_or_default()
    }

    pub fn atoms_for_source(&self, source: &SourceId) -> CandidateBitmap {
        self.source_symbols
            .get(source)
            .and_then(|symbol| self.atoms_by_source.get(symbol))
            .cloned()
            .unwrap_or_default()
    }

    pub fn point_in_time_candidates(
        &self,
        valid_at: ValidTime,
        known_at: TxTime,
    ) -> CandidateBitmap {
        let valid_at = valid_at.as_i64();
        let known_at = known_at.as_i64();
        CandidateBitmap::from_unsorted(
            (0..self.atom_count())
                .filter(|ordinal| {
                    self.columnar.valid_from[*ordinal] <= valid_at
                        && valid_at < self.columnar.valid_to[*ordinal]
                        && self.columnar.tx_from[*ordinal] <= known_at
                        && known_at < self.columnar.tx_to[*ordinal]
                })
                .map(|ordinal| AtomOrdinal::new(ordinal as u32)),
        )
    }

    pub fn trie_candidates_for_claim(&self, pattern: &ClaimPattern) -> CandidateBitmap {
        match (&pattern.subject, &pattern.predicate, &pattern.object) {
            (Some(subject), Some(predicate), Some(object)) => {
                let Some(subject_id) = self.entity_symbols.get(subject) else {
                    return CandidateBitmap::empty();
                };
                let Some(predicate_id) = self.predicate_symbols.get(predicate) else {
                    return CandidateBitmap::empty();
                };
                let object_key = object_symbol_key(object);
                let Some(object_id) = self.object_symbols.get(&object_key) else {
                    return CandidateBitmap::empty();
                };
                self.trie_spo
                    .get(&(*subject_id, *predicate_id, *object_id))
                    .cloned()
                    .unwrap_or_default()
            }
            _ => self.candidates_for_claim_pattern(pattern),
        }
    }

    pub fn candidates_for_claim_pattern(&self, pattern: &ClaimPattern) -> CandidateBitmap {
        let mut candidates = CandidateBitmap::all(self.atom_count());
        if let Some(subject) = &pattern.subject {
            candidates = candidates.intersect(&self.outgoing_for_subject(subject));
        }
        if let Some(predicate) = &pattern.predicate {
            candidates = candidates.intersect(&self.atoms_for_predicate(predicate));
        }
        if let Some(object) = &pattern.object {
            let object_key = object_symbol_key(object);
            let object_candidates = self
                .object_symbols
                .get(&object_key)
                .and_then(|symbol| self.incoming_by_object.get(symbol))
                .cloned()
                .unwrap_or_default();
            candidates = candidates.intersect(&object_candidates);
        }
        candidates
    }

    pub fn atom_ids_for_candidates(&self, candidates: &CandidateBitmap) -> Vec<AtomId> {
        candidates
            .ordinals()
            .iter()
            .filter_map(|ordinal| self.columnar.atom_ids.get(ordinal.as_usize()).cloned())
            .collect()
    }

    pub fn atoms_for_candidates(&self, candidates: &CandidateBitmap) -> Vec<RealityAtom> {
        candidates
            .ordinals()
            .iter()
            .filter_map(|ordinal| self.atoms.get(ordinal.as_usize()).cloned())
            .collect()
    }

    fn push_atom(&mut self, atom: RealityAtom) {
        let ordinal = AtomOrdinal::new(self.columnar.atom_ids.len() as u32);
        let subject_id = intern_symbol(&mut self.entity_symbols, atom.subject.clone());
        let predicate_id = intern_symbol(&mut self.predicate_symbols, atom.predicate.clone());
        let object_id =
            intern_string_symbol(&mut self.object_symbols, object_symbol_key(&atom.object));
        let context_id =
            intern_string_symbol(&mut self.context_symbols, context_key(&atom.context));
        let source_set_id = self.intern_source_set(&atom.source_refs);

        self.columnar.atom_ids.push(atom.id.clone());
        self.columnar.subject_ids.push(subject_id);
        self.columnar.predicate_ids.push(predicate_id);
        self.columnar.object_ids.push(object_id);
        self.columnar
            .valid_from
            .push(atom.valid_time.start.as_i64());
        self.columnar.valid_to.push(
            atom.valid_time
                .end
                .map_or(OPEN_INTERVAL_END, ValidTime::as_i64),
        );
        self.columnar
            .tx_from
            .push(atom.transaction_time.start.as_i64());
        self.columnar.tx_to.push(
            atom.transaction_time
                .end
                .map_or(OPEN_INTERVAL_END, TxTime::as_i64),
        );
        self.columnar.confidence.push(atom.confidence.as_f32());
        self.columnar.belief_state.push(atom.belief_state.clone());
        self.columnar.context_ids.push(context_id);
        self.columnar.source_set_ids.push(source_set_id);

        push_candidate_index(&mut self.outgoing_by_subject, subject_id, ordinal);
        push_candidate_index(&mut self.incoming_by_object, object_id, ordinal);
        push_candidate_index(&mut self.atoms_by_predicate, predicate_id, ordinal);
        push_candidate_index(
            &mut self.valid_start_index,
            atom.valid_time.start.as_i64(),
            ordinal,
        );
        push_candidate_index(
            &mut self.tx_start_index,
            atom.transaction_time.start.as_i64(),
            ordinal,
        );
        self.trie_spo
            .entry((subject_id, predicate_id, object_id))
            .or_default()
            .ordinals
            .push(ordinal);
        for source_ref in &atom.source_refs {
            let source_id = intern_symbol(&mut self.source_symbols, source_ref.source_id.clone());
            push_candidate_index(&mut self.atoms_by_source, source_id, ordinal);
        }
        for conflict in &atom.contradicts {
            self.contradiction_clusters
                .entry(conflict.to_string())
                .or_default()
                .ordinals
                .push(ordinal);
        }
        if !atom.dependencies.is_empty() {
            self.dependency_atoms
                .entry(ordinal)
                .or_default()
                .ordinals
                .extend(atom.dependencies.iter().filter_map(|dependency| {
                    self.columnar
                        .atom_ids
                        .iter()
                        .position(|atom_id| atom_id == dependency)
                        .map(|index| AtomOrdinal::new(index as u32))
                }));
        }
        self.hot_working_set.ordinals.push(ordinal);
        self.atoms.push(atom);
    }

    fn intern_source_set(&mut self, source_refs: &[SourceRef]) -> u32 {
        let mut source_ids = source_refs
            .iter()
            .map(|source_ref| source_ref.source_id.clone())
            .collect::<Vec<_>>();
        source_ids.sort();
        source_ids.dedup();
        if let Some(index) = self
            .source_sets
            .iter()
            .position(|source_set| source_set == &source_ids)
        {
            index as u32
        } else {
            self.source_sets.push(source_ids);
            self.sidecar.source_sets = self.source_sets.len();
            (self.source_sets.len() - 1) as u32
        }
    }

    fn finalize_indexes(&mut self) {
        compact_candidate_map(&mut self.outgoing_by_subject);
        compact_candidate_map(&mut self.incoming_by_object);
        compact_candidate_map(&mut self.atoms_by_predicate);
        compact_candidate_map(&mut self.atoms_by_source);
        compact_candidate_map(&mut self.valid_start_index);
        compact_candidate_map(&mut self.tx_start_index);
        compact_candidate_map(&mut self.trie_spo);
        compact_candidate_map(&mut self.contradiction_clusters);
        compact_candidate_map(&mut self.dependency_atoms);
        self.hot_working_set =
            CandidateBitmap::from_unsorted(self.hot_working_set.ordinals.clone());
        if let (Some(min_tx), Some(max_tx)) = (
            self.columnar.tx_from.iter().min().copied(),
            self.columnar.tx_from.iter().max().copied(),
        ) {
            self.cold_segments.push(ColdHistoricalSegmentDescriptor {
                segment_id: "segment-0000".to_owned(),
                min_tx,
                max_tx,
            });
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct IncrementalSequence(u64);

impl IncrementalSequence {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MaintainedViewName {
    Graph,
    Contradictions,
    Summaries,
    SourceTrust,
    AgentMemory,
    Beliefs,
    Dependencies,
    Causality,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IncrementalEventKind {
    AtomInserted,
    BeliefRevised,
    ConflictAdded,
    DependencyAdded,
    CausalAtomInserted,
    SourceTrustUpdated,
}

#[derive(Clone, Debug, PartialEq)]
pub enum KernelEvent {
    AtomInserted(Box<RealityAtom>),
    BeliefRevised {
        atom_id: AtomId,
        next: BeliefState,
        known_at: TxTime,
        reason: String,
    },
    ConflictAdded(Box<ConflictSet>),
    DependencyAdded(DependencyEdge),
    CausalAtomInserted(Box<CausalAtom>),
    SourceTrustUpdated {
        source_id: SourceId,
        trust_score: f32,
    },
}

impl KernelEvent {
    fn kind(&self) -> IncrementalEventKind {
        match self {
            Self::AtomInserted(_) => IncrementalEventKind::AtomInserted,
            Self::BeliefRevised { .. } => IncrementalEventKind::BeliefRevised,
            Self::ConflictAdded(_) => IncrementalEventKind::ConflictAdded,
            Self::DependencyAdded(_) => IncrementalEventKind::DependencyAdded,
            Self::CausalAtomInserted(_) => IncrementalEventKind::CausalAtomInserted,
            Self::SourceTrustUpdated { .. } => IncrementalEventKind::SourceTrustUpdated,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SequencedKernelEvent {
    pub sequence: IncrementalSequence,
    pub event: KernelEvent,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MaintainedViewVersions {
    pub graph: Option<IncrementalSequence>,
    pub contradictions: Option<IncrementalSequence>,
    pub summaries: Option<IncrementalSequence>,
    pub source_trust: Option<IncrementalSequence>,
    pub agent_memory: Option<IncrementalSequence>,
    pub beliefs: Option<IncrementalSequence>,
    pub dependencies: Option<IncrementalSequence>,
    pub causality: Option<IncrementalSequence>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MaintainedSummaryStatus {
    pub atom_id: AtomId,
    pub dependencies: Vec<AtomId>,
    pub is_stale: bool,
    pub last_valid_sequence: IncrementalSequence,
    pub stale_since: Option<IncrementalSequence>,
    pub invalidation_reason: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MaintainedViews {
    atoms_by_subject: BTreeMap<EntityRef, BTreeSet<AtomId>>,
    atoms_by_source: BTreeMap<SourceId, BTreeSet<AtomId>>,
    atoms_by_agent: BTreeMap<AgentId, BTreeSet<AtomId>>,
    conflicts_by_atom: BTreeMap<AtomId, BTreeSet<String>>,
    current_beliefs: BTreeMap<AtomId, BeliefState>,
    source_trust: BTreeMap<SourceId, f32>,
    summaries: BTreeMap<AtomId, MaintainedSummaryStatus>,
    summary_dependents: BTreeMap<AtomId, BTreeSet<AtomId>>,
    versions: MaintainedViewVersions,
}

impl MaintainedViews {
    pub fn atoms_for_subject(&self, subject: &EntityRef) -> Vec<AtomId> {
        sorted_set_values(self.atoms_by_subject.get(subject))
    }

    pub fn atoms_for_source(&self, source_id: &SourceId) -> Vec<AtomId> {
        sorted_set_values(self.atoms_by_source.get(source_id))
    }

    pub fn atoms_for_agent(&self, agent_id: &AgentId) -> Vec<AtomId> {
        sorted_set_values(self.atoms_by_agent.get(agent_id))
    }

    pub fn conflicts_for_atom(&self, atom_id: &AtomId) -> Vec<String> {
        sorted_set_values(self.conflicts_by_atom.get(atom_id))
    }

    pub fn current_belief(&self, atom_id: &AtomId) -> Option<BeliefState> {
        self.current_beliefs.get(atom_id).cloned()
    }

    pub fn summary_status(&self, atom_id: &AtomId) -> Option<&MaintainedSummaryStatus> {
        self.summaries.get(atom_id)
    }

    pub fn versions(&self) -> &MaintainedViewVersions {
        &self.versions
    }

    fn touch(&mut self, view: MaintainedViewName, sequence: IncrementalSequence) {
        match view {
            MaintainedViewName::Graph => self.versions.graph = Some(sequence),
            MaintainedViewName::Contradictions => self.versions.contradictions = Some(sequence),
            MaintainedViewName::Summaries => self.versions.summaries = Some(sequence),
            MaintainedViewName::SourceTrust => self.versions.source_trust = Some(sequence),
            MaintainedViewName::AgentMemory => self.versions.agent_memory = Some(sequence),
            MaintainedViewName::Beliefs => self.versions.beliefs = Some(sequence),
            MaintainedViewName::Dependencies => self.versions.dependencies = Some(sequence),
            MaintainedViewName::Causality => self.versions.causality = Some(sequence),
        }
    }

    fn index_atom(&mut self, atom: &RealityAtom, sequence: IncrementalSequence) {
        self.atoms_by_subject
            .entry(atom.subject.clone())
            .or_default()
            .insert(atom.id.clone());
        self.current_beliefs
            .insert(atom.id.clone(), atom.belief_state.clone());
        for source_ref in &atom.source_refs {
            self.atoms_by_source
                .entry(source_ref.source_id.clone())
                .or_default()
                .insert(atom.id.clone());
            self.source_trust
                .entry(source_ref.source_id.clone())
                .or_insert(1.0);
        }
        if let Some(agent_id) = &atom.agent_scope {
            self.atoms_by_agent
                .entry(agent_id.clone())
                .or_default()
                .insert(atom.id.clone());
        }
        if atom.claim_type == ClaimType::Summary {
            let mut dependencies = atom.dependencies.clone();
            sort_and_dedup_atom_ids(&mut dependencies);
            for dependency in &dependencies {
                self.summary_dependents
                    .entry(dependency.clone())
                    .or_default()
                    .insert(atom.id.clone());
            }
            self.summaries.insert(
                atom.id.clone(),
                MaintainedSummaryStatus {
                    atom_id: atom.id.clone(),
                    dependencies,
                    is_stale: false,
                    last_valid_sequence: sequence,
                    stale_since: None,
                    invalidation_reason: None,
                },
            );
        }
    }

    fn index_conflict(&mut self, conflict: &ConflictSet) {
        for atom_id in &conflict.atom_ids {
            self.conflicts_by_atom
                .entry(atom_id.clone())
                .or_default()
                .insert(conflict.id.clone());
        }
    }

    fn mark_summaries_stale(
        &mut self,
        roots: &[AtomId],
        sequence: IncrementalSequence,
        reason: impl Into<String>,
    ) -> Vec<AtomId> {
        let reason = reason.into();
        let mut stale = BTreeSet::new();
        for root in roots {
            if let Some(summary_ids) = self.summary_dependents.get(root) {
                stale.extend(summary_ids.iter().cloned());
            }
        }
        for summary_id in &stale {
            if let Some(status) = self.summaries.get_mut(summary_id) {
                status.is_stale = true;
                status.stale_since = Some(sequence);
                status.invalidation_reason = Some(reason.clone());
            }
        }
        stale.into_iter().collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct IncrementalDelta {
    pub sequence: IncrementalSequence,
    pub event_kind: IncrementalEventKind,
    pub touched_views: BTreeSet<MaintainedViewName>,
    pub touched_atoms: Vec<AtomId>,
    pub touched_entities: Vec<EntityRef>,
    pub touched_sources: Vec<SourceId>,
    pub stale_summaries: Vec<AtomId>,
    pub risky_plans: Vec<AtomId>,
    pub impacted_nodes: Vec<DependencyNode>,
}

impl IncrementalDelta {
    fn new(sequence: IncrementalSequence, event_kind: IncrementalEventKind) -> Self {
        Self {
            sequence,
            event_kind,
            touched_views: BTreeSet::new(),
            touched_atoms: Vec::new(),
            touched_entities: Vec::new(),
            touched_sources: Vec::new(),
            stale_summaries: Vec::new(),
            risky_plans: Vec::new(),
            impacted_nodes: Vec::new(),
        }
    }

    fn touch_view(&mut self, view: MaintainedViewName) {
        self.touched_views.insert(view);
    }

    fn finalize(&mut self) {
        sort_and_dedup_atom_ids(&mut self.touched_atoms);
        self.touched_entities.sort();
        self.touched_entities.dedup();
        self.touched_sources.sort();
        self.touched_sources.dedup();
        sort_and_dedup_atom_ids(&mut self.stale_summaries);
        sort_and_dedup_atom_ids(&mut self.risky_plans);
        self.impacted_nodes.sort();
        self.impacted_nodes.dedup();
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct IncrementalComputation {
    kernel: RealityKernel,
    event_log: Vec<SequencedKernelEvent>,
    views: MaintainedViews,
    next_sequence: u64,
}

impl IncrementalComputation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn kernel(&self) -> &RealityKernel {
        &self.kernel
    }

    pub fn event_log(&self) -> &[SequencedKernelEvent] {
        &self.event_log
    }

    pub fn views(&self) -> &MaintainedViews {
        &self.views
    }

    pub fn apply_event(&mut self, event: KernelEvent) -> Result<IncrementalDelta, KernelError> {
        self.validate_event(&event)?;
        self.next_sequence += 1;
        let sequence = IncrementalSequence::new(self.next_sequence);
        let event_kind = event.kind();
        self.event_log.push(SequencedKernelEvent {
            sequence,
            event: event.clone(),
        });
        let mut delta = IncrementalDelta::new(sequence, event_kind);

        match event {
            KernelEvent::AtomInserted(atom) => self.apply_atom_inserted(*atom, &mut delta),
            KernelEvent::BeliefRevised {
                atom_id,
                next,
                known_at,
                reason,
            } => self.apply_belief_revised(atom_id, next, known_at, reason, &mut delta)?,
            KernelEvent::ConflictAdded(conflict) => {
                self.apply_conflict_added(*conflict, &mut delta)
            }
            KernelEvent::DependencyAdded(edge) => self.apply_dependency_added(edge, &mut delta)?,
            KernelEvent::CausalAtomInserted(atom) => {
                self.apply_causal_atom_inserted(*atom, &mut delta)?
            }
            KernelEvent::SourceTrustUpdated {
                source_id,
                trust_score,
            } => self.apply_source_trust_updated(source_id, trust_score, &mut delta),
        }

        delta.finalize();
        Ok(delta)
    }

    fn validate_event(&self, event: &KernelEvent) -> Result<(), KernelError> {
        match event {
            KernelEvent::BeliefRevised { atom_id, .. } => {
                if self.kernel.atom(atom_id).is_some() {
                    Ok(())
                } else {
                    Err(KernelError::UnknownAtom(atom_id.clone()))
                }
            }
            KernelEvent::DependencyAdded(edge) => edge.validate(),
            KernelEvent::CausalAtomInserted(atom) => atom.validate(),
            KernelEvent::AtomInserted(_)
            | KernelEvent::ConflictAdded(_)
            | KernelEvent::SourceTrustUpdated { .. } => Ok(()),
        }
    }

    fn apply_atom_inserted(&mut self, atom: RealityAtom, delta: &mut IncrementalDelta) {
        let atom_id = atom.id.clone();
        let superseded = atom.supersedes.clone();
        let sources = atom
            .source_refs
            .iter()
            .map(|source_ref| source_ref.source_id.clone())
            .collect::<Vec<_>>();
        let agent_scope = atom.agent_scope.clone();

        self.kernel.insert_atom(atom.clone());
        self.views.index_atom(&atom, delta.sequence);
        touch_view(&mut self.views, delta, MaintainedViewName::Graph);
        touch_view(&mut self.views, delta, MaintainedViewName::Beliefs);
        if !sources.is_empty() {
            touch_view(&mut self.views, delta, MaintainedViewName::SourceTrust);
        }
        if agent_scope.is_some() {
            touch_view(&mut self.views, delta, MaintainedViewName::AgentMemory);
        }
        if atom.claim_type == ClaimType::Summary {
            touch_view(&mut self.views, delta, MaintainedViewName::Summaries);
        }

        delta.touched_atoms.push(atom_id.clone());
        delta.touched_entities.push(atom.subject.clone());
        delta.touched_sources.extend(sources);

        if !superseded.is_empty() {
            for superseded_id in &superseded {
                if let Some(superseded_atom) = self.kernel.atom(superseded_id) {
                    self.views
                        .current_beliefs
                        .insert(superseded_id.clone(), superseded_atom.belief_state.clone());
                }
            }
            delta.touched_atoms.extend(superseded.clone());
            let stale = self.views.mark_summaries_stale(
                &superseded,
                delta.sequence,
                format!("superseded by {atom_id}"),
            );
            if !stale.is_empty() {
                delta.stale_summaries.extend(stale);
                touch_view(&mut self.views, delta, MaintainedViewName::Summaries);
            }
        }
    }

    fn apply_belief_revised(
        &mut self,
        atom_id: AtomId,
        next: BeliefState,
        known_at: TxTime,
        reason: String,
        delta: &mut IncrementalDelta,
    ) -> Result<(), KernelError> {
        self.kernel
            .revise_belief(&atom_id, next.clone(), known_at, reason.clone())?;
        self.views.current_beliefs.insert(atom_id.clone(), next);
        touch_view(&mut self.views, delta, MaintainedViewName::Beliefs);
        delta.touched_atoms.push(atom_id.clone());

        let root = DependencyNode::Atom(atom_id.clone());
        let impacted_nodes = self.kernel.dependencies.transitive_dependents(&root);
        delta.impacted_nodes.extend(impacted_nodes.iter().cloned());
        let mut impacted_atoms = vec![atom_id.clone()];
        for node in impacted_nodes {
            if let DependencyNode::Atom(impacted_atom_id) = node {
                if let Some(atom) = self.kernel.atom(&impacted_atom_id) {
                    if is_plan_atom(atom) {
                        delta.risky_plans.push(impacted_atom_id.clone());
                    }
                }
                impacted_atoms.push(impacted_atom_id);
            }
        }
        let stale = self
            .views
            .mark_summaries_stale(&impacted_atoms, delta.sequence, reason);
        if !stale.is_empty() || !delta.risky_plans.is_empty() {
            delta.stale_summaries.extend(stale);
            touch_view(&mut self.views, delta, MaintainedViewName::Summaries);
        }
        Ok(())
    }

    fn apply_conflict_added(&mut self, conflict: ConflictSet, delta: &mut IncrementalDelta) {
        let atom_ids = conflict.atom_ids.clone();
        self.kernel.add_conflict(conflict.clone());
        self.views.index_conflict(&conflict);
        for atom_id in &atom_ids {
            if let Some(atom) = self.kernel.atom(atom_id) {
                self.views
                    .current_beliefs
                    .insert(atom_id.clone(), atom.belief_state.clone());
            }
        }
        let stale = self.views.mark_summaries_stale(
            &atom_ids,
            delta.sequence,
            format!("conflict {} changed source-backed truth", conflict.id),
        );
        delta.touched_atoms.extend(atom_ids);
        delta.stale_summaries.extend(stale);
        touch_view(&mut self.views, delta, MaintainedViewName::Contradictions);
        touch_view(&mut self.views, delta, MaintainedViewName::Beliefs);
        if !delta.stale_summaries.is_empty() {
            touch_view(&mut self.views, delta, MaintainedViewName::Summaries);
        }
    }

    fn apply_dependency_added(
        &mut self,
        edge: DependencyEdge,
        delta: &mut IncrementalDelta,
    ) -> Result<(), KernelError> {
        let from = edge.from.clone();
        let to = edge.to.clone();
        self.kernel.add_dependency_edge(edge)?;
        delta.touched_atoms.push(from);
        delta.touched_atoms.push(to);
        touch_view(&mut self.views, delta, MaintainedViewName::Dependencies);
        Ok(())
    }

    fn apply_causal_atom_inserted(
        &mut self,
        atom: CausalAtom,
        delta: &mut IncrementalDelta,
    ) -> Result<(), KernelError> {
        self.kernel.insert_causal_atom(atom)?;
        touch_view(&mut self.views, delta, MaintainedViewName::Causality);
        Ok(())
    }

    fn apply_source_trust_updated(
        &mut self,
        source_id: SourceId,
        trust_score: f32,
        delta: &mut IncrementalDelta,
    ) {
        self.views
            .source_trust
            .insert(source_id.clone(), trust_score);
        delta.touched_sources.push(source_id);
        touch_view(&mut self.views, delta, MaintainedViewName::SourceTrust);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BitemporalTruth {
    pub valid_at: ValidTime,
    pub known_at: TxTime,
}

impl BitemporalTruth {
    pub fn new(valid_at: ValidTime, known_at: TxTime) -> Self {
        Self { valid_at, known_at }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BitemporalQuestion {
    EntityState,
    WhatIsTrueNow,
    WhatWasTrueAt,
    WhatDidWeBelieveAt,
    WhenDidBeliefChange,
    IfSourceFalseWhatCollapses,
    WhatCaused,
    WhatMightHappenNext,
    WhatBreaksIfEventDoesNotOccur,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClaimPattern {
    pub subject: Option<EntityRef>,
    pub predicate: Option<PredicateId>,
    pub object: Option<ValueOrEntity>,
}

impl ClaimPattern {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn subject(mut self, subject: EntityRef) -> Self {
        self.subject = Some(subject);
        self
    }

    pub fn predicate(mut self, predicate: PredicateId) -> Self {
        self.predicate = Some(predicate);
        self
    }

    pub fn object(mut self, object: ValueOrEntity) -> Self {
        self.object = Some(object);
        self
    }

    fn is_fully_bound(&self) -> bool {
        self.subject.is_some() && self.predicate.is_some() && self.object.is_some()
    }
}

pub type AtomPattern = ClaimPattern;

#[derive(Clone, Debug, PartialEq)]
pub enum KernelQuery {
    GetAtom(AtomId),
    FindAtoms(AtomPattern),
    VisibleAt {
        valid_at: ValidTime,
        known_at: TransactionTime,
        pattern: AtomPattern,
    },
    ExplainSupport {
        atom_id: AtomId,
    },
    ExplainConflict {
        atom_id: AtomId,
    },
    ImpactIfRetracted {
        atom_id: AtomId,
        max_depth: usize,
    },
    EntityState {
        entity_id: EntityRef,
        valid_at: ValidTime,
        known_at: TransactionTime,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct KernelQueryResult {
    pub atom_ids: Vec<AtomId>,
    pub evidence_ids: Vec<SourceId>,
    pub beliefs: Vec<(AtomId, BeliefState)>,
    pub valid_times: BTreeMap<AtomId, TimeInterval<ValidTime>>,
    pub transaction_times: BTreeMap<AtomId, TimeInterval<TxTime>>,
    pub dependency_trace: Vec<DependencyStep>,
    pub atoms: Vec<RealityAtom>,
    pub conflicts: Vec<ConflictSet>,
    pub support: Option<SupportSet>,
    pub impact: Option<ImpactCone>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RealityOperator {
    ValidAt(ValidTime),
    KnownAt(TxTime),
    BeliefIn(Vec<BeliefState>),
    RequireEvidence,
    IncludeContradictions,
    AllowPermissions(Vec<PermissionLabel>),
    DependencyTrace,
    CausalCauses { event_id: EventId, max_depth: usize },
    CausalEffects { event_id: EventId, max_depth: usize },
    CounterfactualAtomFalse { atom_id: AtomId },
    SimulationOnly,
}

impl RealityOperator {
    fn label(&self) -> &'static str {
        match self {
            Self::ValidAt(_) => "TemporalValidAt",
            Self::KnownAt(_) => "TemporalKnownAt",
            Self::BeliefIn(_) => "BeliefFilter",
            Self::RequireEvidence => "EvidenceLookup",
            Self::IncludeContradictions => "ContradictionLookup",
            Self::AllowPermissions(_) => "PermissionFilter",
            Self::DependencyTrace => "DependencyTrace",
            Self::CausalCauses { .. } => "CausalCauses",
            Self::CausalEffects { .. } => "CausalEffects",
            Self::CounterfactualAtomFalse { .. } => "CounterfactualAtomFalse",
            Self::SimulationOnly => "SimulationGuard",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RealityReturnField {
    Belief,
    Evidence,
    Contradictions,
    DependencyTrace,
    AffectedBeliefs,
    Plans,
    Memories,
    Summaries,
    Agents,
    CausalPaths,
    SimulationImpact,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeExecutionStrategy {
    PointInTimeLookup,
    LeapfrogTriejoinCandidate,
    CausalTraversal,
    DependencyInvalidation,
    CounterfactualImpactSearch,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NativeVmTraceStep {
    pub operator: String,
    pub detail: String,
}

impl NativeVmTraceStep {
    fn new(operator: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            operator: operator.into(),
            detail: detail.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NativeRealityPlan {
    pub strategy: NativeExecutionStrategy,
    pub operators: Vec<RealityOperator>,
    pub return_fields: Vec<RealityReturnField>,
    pub execution_trace: Vec<NativeVmTraceStep>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NativeRealityQueryKind {
    VerifyClaim(ClaimPattern),
    WhatBreaksIfFalse(AtomId),
    CausalCauses { event_id: EventId, max_depth: usize },
    CausalEffects { event_id: EventId, max_depth: usize },
}

#[derive(Clone, Debug, PartialEq)]
pub struct NativeRealityQuery {
    pub kind: NativeRealityQueryKind,
    pub operators: Vec<RealityOperator>,
    pub return_fields: Vec<RealityReturnField>,
}

impl NativeRealityQuery {
    pub fn verify_claim(pattern: ClaimPattern) -> Self {
        Self {
            kind: NativeRealityQueryKind::VerifyClaim(pattern),
            operators: Vec::new(),
            return_fields: Vec::new(),
        }
    }

    pub fn what_breaks_if_false(atom_id: AtomId) -> Self {
        Self {
            kind: NativeRealityQueryKind::WhatBreaksIfFalse(atom_id.clone()),
            operators: vec![RealityOperator::CounterfactualAtomFalse { atom_id }],
            return_fields: Vec::new(),
        }
    }

    pub fn causes_of(event_id: EventId, max_depth: usize) -> Self {
        Self {
            kind: NativeRealityQueryKind::CausalCauses {
                event_id: event_id.clone(),
                max_depth,
            },
            operators: vec![RealityOperator::CausalCauses {
                event_id,
                max_depth,
            }],
            return_fields: vec![RealityReturnField::CausalPaths],
        }
    }

    pub fn effects_of(event_id: EventId, max_depth: usize) -> Self {
        Self {
            kind: NativeRealityQueryKind::CausalEffects {
                event_id: event_id.clone(),
                max_depth,
            },
            operators: vec![RealityOperator::CausalEffects {
                event_id,
                max_depth,
            }],
            return_fields: vec![RealityReturnField::CausalPaths],
        }
    }

    pub fn with_operator(mut self, operator: RealityOperator) -> Self {
        self.operators.push(operator);
        self
    }

    pub fn returning(mut self, mut return_fields: Vec<RealityReturnField>) -> Self {
        return_fields.sort();
        return_fields.dedup();
        self.return_fields = return_fields;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NativeRealityQueryResult {
    pub plan: NativeRealityPlan,
    pub atoms: Vec<RealityAtom>,
    pub beliefs: Vec<(AtomId, BeliefState)>,
    pub evidence: Vec<EvidenceSpan>,
    pub contradictions: Vec<ConflictSet>,
    pub dependency_trace: Vec<DependencyStep>,
    pub permission_filtered_atoms: Vec<AtomId>,
    pub causal_paths: Vec<CausalPath>,
    pub impact_report: Option<AtomImpactReport>,
    pub causal_impact: Option<CausalImpactReport>,
    pub affected_beliefs: Vec<AtomId>,
    pub affected_plans: Vec<AtomId>,
    pub affected_memories: Vec<AtomId>,
    pub affected_summaries: Vec<AtomId>,
    pub affected_agents: Vec<AgentId>,
    pub warnings: Vec<String>,
    pub execution_trace: Vec<NativeVmTraceStep>,
}

impl NativeRealityQueryResult {
    fn new(plan: NativeRealityPlan) -> Self {
        Self {
            execution_trace: plan.execution_trace.clone(),
            plan,
            atoms: Vec::new(),
            beliefs: Vec::new(),
            evidence: Vec::new(),
            contradictions: Vec::new(),
            dependency_trace: Vec::new(),
            permission_filtered_atoms: Vec::new(),
            causal_paths: Vec::new(),
            impact_report: None,
            causal_impact: None,
            affected_beliefs: Vec::new(),
            affected_plans: Vec::new(),
            affected_memories: Vec::new(),
            affected_summaries: Vec::new(),
            affected_agents: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelContextRequest {
    pub task: String,
    pub agent_id: AgentId,
    pub current_goal: Option<String>,
    pub valid_at: Option<ValidTime>,
    pub known_at: Option<TxTime>,
    pub permission_scope: Vec<PermissionLabel>,
    pub token_budget: usize,
    pub risk_level: RiskLevel,
}

impl ModelContextRequest {
    pub fn new(task: impl Into<String>, agent_id: AgentId) -> Self {
        Self {
            task: task.into(),
            agent_id,
            current_goal: None,
            valid_at: None,
            known_at: None,
            permission_scope: Vec::new(),
            token_budget: 1024,
            risk_level: RiskLevel::Medium,
        }
    }

    pub fn current_goal(mut self, current_goal: impl Into<String>) -> Self {
        self.current_goal = Some(current_goal.into());
        self
    }

    pub fn valid_at(mut self, valid_at: ValidTime) -> Self {
        self.valid_at = Some(valid_at);
        self
    }

    pub fn known_at(mut self, known_at: TxTime) -> Self {
        self.known_at = Some(known_at);
        self
    }

    pub fn permission_scope(mut self, permission_scope: Vec<PermissionLabel>) -> Self {
        self.permission_scope = permission_scope;
        self
    }

    pub fn token_budget(mut self, token_budget: usize) -> Self {
        self.token_budget = token_budget;
        self
    }

    pub fn risk_level(mut self, risk_level: RiskLevel) -> Self {
        self.risk_level = risk_level;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelEvidencePack {
    pub atoms: Vec<RealityAtom>,
    pub evidence: Vec<EvidenceSpan>,
    pub source_ids: Vec<SourceId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelBeliefContext {
    pub atom_id: AtomId,
    pub belief_state: BeliefState,
    pub confidence: Confidence,
    pub valid_time: TimeInterval<ValidTime>,
    pub transaction_time: TimeInterval<TxTime>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MissingInformation {
    pub description: String,
    pub why_it_matters: String,
    pub risk_level: RiskLevel,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SafeAssumption {
    pub statement: String,
    pub atom_ids: Vec<AtomId>,
    pub confidence: Confidence,
    pub how_we_know: String,
    pub caveat: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecommendedActionKind {
    RetrieveEvidence,
    ReviewContradiction,
    VerifyClaim,
    AskClarifyingQuestion,
    RunCounterfactual,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecommendedContextAction {
    pub kind: RecommendedActionKind,
    pub reason: String,
    pub target: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledModelContext {
    pub task: String,
    pub agent_id: AgentId,
    pub current_goal: Option<String>,
    pub valid_at: Option<ValidTime>,
    pub known_at: Option<TxTime>,
    pub token_budget: usize,
    pub estimated_tokens: usize,
    pub risk_level: RiskLevel,
    pub evidence_pack: ModelEvidencePack,
    pub current_belief_state: Vec<ModelBeliefContext>,
    pub relevant_memories: Vec<RealityAtom>,
    pub contradictions: Vec<ConflictSet>,
    pub missing_information: Vec<MissingInformation>,
    pub safe_assumptions: Vec<SafeAssumption>,
    pub recommended_actions: Vec<RecommendedContextAction>,
    pub permission_filtered_atoms: Vec<AtomId>,
    pub warnings: Vec<String>,
}

pub struct ModelContextCompiler<'a> {
    kernel: &'a RealityKernel,
}

impl<'a> ModelContextCompiler<'a> {
    pub fn new(kernel: &'a RealityKernel) -> Self {
        Self { kernel }
    }

    pub fn compile(&self, request: ModelContextRequest) -> CompiledModelContext {
        let mut permission_filtered_atoms = Vec::new();
        let mut candidate_atoms = Vec::new();
        let mut warnings = Vec::new();

        for atom in self.kernel.atoms.values() {
            if !model_context_time_visible(atom, &request) {
                continue;
            }
            if !self
                .kernel
                .atom_visible_with_permissions(atom, &request.permission_scope)
            {
                permission_filtered_atoms.push(atom.id.clone());
                continue;
            }
            if matches!(
                atom.claim_type,
                ClaimType::Simulation | ClaimType::Hypothesis
            ) {
                warnings.push(format!(
                    "{} excluded because it is not factual model context",
                    atom.id
                ));
                continue;
            }
            candidate_atoms.push(atom.clone());
        }

        candidate_atoms.sort_by(|left, right| {
            model_context_relevance(right, &request)
                .cmp(&model_context_relevance(left, &request))
                .then_with(|| {
                    right
                        .confidence
                        .partial_cmp(&left.confidence)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| left.id.cmp(&right.id))
        });

        let mut atoms = Vec::new();
        let mut estimated_tokens = 0;
        let mut budget_excluded_atoms = Vec::new();
        for atom in candidate_atoms {
            let atom_tokens = estimate_model_context_tokens(&atom);
            if estimated_tokens + atom_tokens <= request.token_budget {
                estimated_tokens += atom_tokens;
                atoms.push(atom);
            } else {
                budget_excluded_atoms.push(atom.id);
            }
        }
        sort_atoms(&mut atoms);

        let atom_ids = atoms
            .iter()
            .map(|atom| atom.id.clone())
            .collect::<BTreeSet<_>>();
        let contradictions = self
            .kernel
            .conflicts
            .iter()
            .filter(|conflict| {
                conflict
                    .atom_ids
                    .iter()
                    .any(|atom_id| atom_ids.contains(atom_id))
            })
            .cloned()
            .collect::<Vec<_>>();

        let evidence = atoms
            .iter()
            .flat_map(|atom| atom.evidence_spans.iter().cloned())
            .collect::<Vec<_>>();
        let mut source_ids = atoms
            .iter()
            .flat_map(|atom| {
                atom.source_refs
                    .iter()
                    .map(|source_ref| source_ref.source_id.clone())
            })
            .collect::<Vec<_>>();
        source_ids.sort();
        source_ids.dedup();

        let current_belief_state = atoms
            .iter()
            .filter_map(|atom| {
                let known_at = request.known_at.unwrap_or(atom.transaction_time.start);
                self.kernel
                    .belief_at(&atom.id, known_at)
                    .map(|belief_state| ModelBeliefContext {
                        atom_id: atom.id.clone(),
                        belief_state,
                        confidence: atom.confidence,
                        valid_time: atom.valid_time.clone(),
                        transaction_time: atom.transaction_time.clone(),
                    })
            })
            .collect::<Vec<_>>();

        let relevant_memories = atoms
            .iter()
            .filter(|atom| {
                matches!(atom.claim_type, ClaimType::AgentMemory)
                    && atom.agent_scope.as_ref() == Some(&request.agent_id)
            })
            .cloned()
            .collect::<Vec<_>>();

        let mut missing_information = Vec::new();
        if atoms.is_empty() {
            missing_information.push(MissingInformation {
                description: "no source-backed graph context matched the task".to_owned(),
                why_it_matters:
                    "the model should not answer from random chunks or unsupported memory"
                        .to_owned(),
                risk_level: request.risk_level,
            });
        }
        if !contradictions.is_empty() && request.risk_level >= RiskLevel::High {
            missing_information.push(MissingInformation {
                description: "unresolved contradiction requires review".to_owned(),
                why_it_matters: "high-risk model context must preserve conflicting evidence"
                    .to_owned(),
                risk_level: request.risk_level,
            });
        }
        if !permission_filtered_atoms.is_empty() {
            missing_information.push(MissingInformation {
                description: "permission scope excluded potentially relevant atoms".to_owned(),
                why_it_matters: "the model may need authorized evidence before acting".to_owned(),
                risk_level: request.risk_level,
            });
        }
        if !budget_excluded_atoms.is_empty() {
            missing_information.push(MissingInformation {
                description: "token budget excluded relevant atoms".to_owned(),
                why_it_matters:
                    "the model received compressed context and may need follow-up retrieval"
                        .to_owned(),
                risk_level: request.risk_level,
            });
        }

        let safe_assumptions = atoms
            .iter()
            .filter(|atom| atom.belief_state.ai_supported() && atom.confidence.as_f32() >= 0.7)
            .map(|atom| SafeAssumption {
                statement: format!(
                    "{} {} {}",
                    atom.subject,
                    atom.predicate,
                    value_or_entity_label(&atom.object)
                ),
                atom_ids: vec![atom.id.clone()],
                confidence: atom.confidence,
                how_we_know: "source-backed evidence in Reality Kernel".to_owned(),
                caveat: model_context_caveat(atom),
            })
            .collect::<Vec<_>>();

        let recommended_actions = model_context_recommended_actions(
            &request,
            &contradictions,
            &missing_information,
            &permission_filtered_atoms,
        );

        sort_and_dedup_atom_ids(&mut permission_filtered_atoms);
        warnings.sort();
        warnings.dedup();

        CompiledModelContext {
            task: request.task,
            agent_id: request.agent_id,
            current_goal: request.current_goal,
            valid_at: request.valid_at,
            known_at: request.known_at,
            token_budget: request.token_budget,
            estimated_tokens,
            risk_level: request.risk_level,
            evidence_pack: ModelEvidencePack {
                atoms,
                evidence,
                source_ids,
            },
            current_belief_state,
            relevant_memories,
            contradictions,
            missing_information,
            safe_assumptions,
            recommended_actions,
            permission_filtered_atoms,
            warnings,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RealityKernel {
    atoms: BTreeMap<AtomId, RealityAtom>,
    revisions: BTreeMap<AtomId, Vec<BeliefRevision>>,
    conflicts: Vec<ConflictSet>,
    dependencies: DependencyGraph,
    causal_outgoing: BTreeMap<EventId, Vec<CausalAtom>>,
    causal_incoming: BTreeMap<EventId, Vec<CausalAtom>>,
}

impl RealityKernel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_atom(&mut self, atom: RealityAtom) {
        let new_atom_id = atom.id.clone();
        for dependency in &atom.dependencies {
            self.dependencies
                .add_typed_dependency(
                    DependencyNode::Atom(dependency.clone()),
                    DependencyNode::Atom(new_atom_id.clone()),
                    DependencyType::DerivedFrom,
                    1.0,
                    "atom depends on upstream atom",
                )
                .expect("literal dependency strength is valid");
        }
        for superseded in &atom.supersedes {
            if let Some(existing) = self.atoms.get_mut(superseded) {
                let previous = existing.belief_state.clone();
                existing.belief_state = BeliefState::Superseded;
                self.revisions
                    .entry(superseded.clone())
                    .or_default()
                    .push(BeliefRevision {
                        atom_id: superseded.clone(),
                        known_at: atom.transaction_time.start,
                        previous,
                        next: BeliefState::Superseded,
                        reason: format!("superseded by {}", atom.id),
                    });
            }
            self.dependencies
                .add_typed_dependency(
                    DependencyNode::Atom(superseded.clone()),
                    DependencyNode::Atom(new_atom_id.clone()),
                    DependencyType::SupersededBy,
                    1.0,
                    "supersession depends on prior atom history",
                )
                .expect("literal dependency strength is valid");
        }
        self.atoms.insert(new_atom_id, atom);
    }

    pub fn atom(&self, atom_id: &AtomId) -> Option<&RealityAtom> {
        self.atoms.get(atom_id)
    }

    pub fn add_conflict(&mut self, conflict: ConflictSet) {
        for atom_id in &conflict.atom_ids {
            if let Some(atom) = self.atoms.get_mut(atom_id) {
                if !atom
                    .contradicts
                    .iter()
                    .any(|other| conflict.atom_ids.iter().any(|candidate| candidate == other))
                {
                    atom.contradicts.extend(
                        conflict
                            .atom_ids
                            .iter()
                            .filter(|other| *other != atom_id)
                            .cloned(),
                    );
                }
                if atom.belief_state == BeliefState::Accepted {
                    atom.belief_state = BeliefState::Disputed;
                }
            }
        }
        for from in &conflict.atom_ids {
            for to in conflict
                .atom_ids
                .iter()
                .filter(|candidate| *candidate != from)
            {
                self.dependencies
                    .add_typed_dependency(
                        DependencyNode::Atom(from.clone()),
                        DependencyNode::Atom(to.clone()),
                        DependencyType::ContradictedBy,
                        1.0,
                        format!("{} conflicts with {}", from, to),
                    )
                    .expect("literal dependency strength is valid");
            }
        }
        self.conflicts.push(conflict);
    }

    pub fn add_dependency(
        &mut self,
        from: DependencyNode,
        to: DependencyNode,
        explanation: impl Into<String>,
    ) {
        self.dependencies.add_dependency(from, to, explanation);
    }

    pub fn add_dependency_edge(&mut self, edge: DependencyEdge) -> Result<(), KernelError> {
        self.dependencies.add_dependency_edge(edge)
    }

    pub fn explain_support(&self, atom_id: &AtomId) -> Option<SupportSet> {
        let atom = self.atoms.get(atom_id)?;
        let mut supporting_atoms = atom.dependencies.clone();
        sort_and_dedup_atom_ids(&mut supporting_atoms);

        let mut source_ids = atom
            .source_refs
            .iter()
            .map(|source_ref| source_ref.source_id.clone())
            .collect::<Vec<_>>();
        let mut evidence = atom.evidence_spans.clone();
        for supporting_atom_id in &supporting_atoms {
            if let Some(supporting_atom) = self.atoms.get(supporting_atom_id) {
                source_ids.extend(
                    supporting_atom
                        .source_refs
                        .iter()
                        .map(|source_ref| source_ref.source_id.clone()),
                );
                evidence.extend(supporting_atom.evidence_spans.iter().cloned());
            }
        }
        source_ids.sort();
        source_ids.dedup();
        evidence.sort_by(|left, right| {
            left.source_id
                .cmp(&right.source_id)
                .then_with(|| left.start.cmp(&right.start))
                .then_with(|| left.end.cmp(&right.end))
                .then_with(|| left.quote.cmp(&right.quote))
        });
        evidence.dedup();

        Some(SupportSet {
            atom_id: atom_id.clone(),
            supporting_atoms,
            source_ids,
            evidence,
            dependency_trace: self
                .dependencies
                .trace_from(&DependencyNode::Atom(atom_id.clone())),
        })
    }

    pub fn explain_conflict(&self, atom_id: &AtomId) -> Vec<ConflictSet> {
        let mut conflicts = self
            .conflicts
            .iter()
            .filter(|conflict| conflict.atom_ids.iter().any(|id| id == atom_id))
            .cloned()
            .collect::<Vec<_>>();
        conflicts.sort_by(|left, right| left.id.cmp(&right.id));
        conflicts
    }

    pub fn compute_downstream_dependencies(&self, atom_id: &AtomId) -> Vec<DependencyNode> {
        self.dependencies
            .transitive_dependents(&DependencyNode::Atom(atom_id.clone()))
    }

    pub fn compute_impact_if_retracted(&self, atom_id: &AtomId) -> ImpactCone {
        let report = self.impact_if_retracted(atom_id);
        let root = DependencyNode::Atom(atom_id.clone());
        let invalidation_trace =
            TruthMaintenance::new(self.dependencies.clone()).invalidate(root, "atom retracted");
        ImpactCone {
            root: report.root,
            impacted_atoms: report.impacted_atoms,
            impacted_answers: report.impacted_answers,
            impacted_simulations: report.impacted_simulations,
            invalidation_trace,
            warning: report.warning,
        }
    }

    pub fn summary_permission_leaks(
        &self,
        summary_id: &AtomId,
        permission_scope: &[PermissionLabel],
    ) -> Vec<AtomId> {
        let Some(summary) = self.atoms.get(summary_id) else {
            return Vec::new();
        };
        if !matches!(summary.claim_type, ClaimType::Summary) {
            return Vec::new();
        }

        let mut leaks = summary
            .dependencies
            .iter()
            .filter_map(|dependency| {
                let upstream = self.atoms.get(dependency)?;
                (!permission_scope_allows(&upstream.permissions, permission_scope))
                    .then(|| dependency.clone())
            })
            .collect::<Vec<_>>();
        sort_and_dedup_atom_ids(&mut leaks);
        leaks
    }

    pub fn summary_is_permission_safe(
        &self,
        summary_id: &AtomId,
        permission_scope: &[PermissionLabel],
    ) -> bool {
        self.summary_permission_leaks(summary_id, permission_scope)
            .is_empty()
    }

    pub fn atom_visible_with_permissions(
        &self,
        atom: &RealityAtom,
        permission_scope: &[PermissionLabel],
    ) -> bool {
        permission_scope_allows(&atom.permissions, permission_scope)
            && (!matches!(atom.claim_type, ClaimType::Summary)
                || self.summary_is_permission_safe(&atom.id, permission_scope))
    }

    pub fn belief_at(&self, atom_id: &AtomId, known_at: TxTime) -> Option<BeliefState> {
        let atom = self.atoms.get(atom_id)?;
        let mut state = atom.belief_state.clone();
        if let Some(revisions) = self.revisions.get(atom_id) {
            for revision in revisions.iter().rev() {
                if known_at < revision.known_at {
                    state = revision.previous.clone();
                }
            }
        }
        Some(state)
    }

    pub fn revise_belief(
        &mut self,
        atom_id: &AtomId,
        next: BeliefState,
        known_at: TxTime,
        reason: impl Into<String>,
    ) -> Result<(), KernelError> {
        let atom = self
            .atoms
            .get_mut(atom_id)
            .ok_or_else(|| KernelError::UnknownAtom(atom_id.clone()))?;
        let previous = atom.belief_state.clone();
        atom.belief_state = next.clone();
        let revisions = self.revisions.entry(atom_id.clone()).or_default();
        revisions.push(BeliefRevision {
            atom_id: atom_id.clone(),
            known_at,
            previous,
            next,
            reason: reason.into(),
        });
        revisions.sort_by_key(|revision| revision.known_at);
        Ok(())
    }

    pub fn belief_revisions(&self, atom_id: &AtomId) -> &[BeliefRevision] {
        self.revisions.get(atom_id).map_or(&[], Vec::as_slice)
    }

    pub fn entity_state(
        &self,
        entity: EntityRef,
        valid_at: ValidTime,
        known_at: TxTime,
    ) -> EntityState {
        let mut accepted_atoms = Vec::new();
        let mut disputed_atoms = Vec::new();
        let mut superseded_atoms = Vec::new();
        for atom in self.atoms.values() {
            if atom.subject != entity || !atom.is_visible_at(valid_at, known_at) {
                continue;
            }
            match self.belief_at(&atom.id, known_at) {
                Some(BeliefState::Accepted) => accepted_atoms.push(atom.clone()),
                Some(BeliefState::Disputed) => disputed_atoms.push(atom.clone()),
                Some(BeliefState::Superseded) => superseded_atoms.push(atom.clone()),
                Some(
                    BeliefState::Candidate
                    | BeliefState::Retracted
                    | BeliefState::Refuted
                    | BeliefState::Simulated
                    | BeliefState::Unknown,
                )
                | None => {}
            }
        }
        sort_atoms(&mut accepted_atoms);
        sort_atoms(&mut disputed_atoms);
        sort_atoms(&mut superseded_atoms);

        let visible_atom_ids = accepted_atoms
            .iter()
            .chain(disputed_atoms.iter())
            .chain(superseded_atoms.iter())
            .map(|atom| atom.id.clone())
            .collect::<BTreeSet<_>>();
        let conflicts = self
            .conflicts
            .iter()
            .filter(|conflict| {
                conflict
                    .atom_ids
                    .iter()
                    .any(|atom_id| visible_atom_ids.contains(atom_id))
            })
            .cloned()
            .collect();

        EntityState {
            entity,
            valid_at,
            known_at,
            accepted_atoms,
            disputed_atoms,
            superseded_atoms,
            conflicts,
        }
    }

    pub fn impact_if_retracted(&self, atom_id: &AtomId) -> AtomImpactReport {
        let nodes = self
            .dependencies
            .transitive_dependents(&DependencyNode::Atom(atom_id.clone()));
        let mut impacted_atoms = Vec::new();
        let mut impacted_answers = Vec::new();
        let mut impacted_simulations = Vec::new();
        for node in nodes {
            match node {
                DependencyNode::Atom(id) => impacted_atoms.push(id),
                DependencyNode::Answer(id) => impacted_answers.push(id),
                DependencyNode::Simulation(id) => impacted_simulations.push(id),
            }
        }
        impacted_atoms.sort();
        impacted_answers.sort();
        impacted_simulations.sort();
        AtomImpactReport {
            root: atom_id.clone(),
            impacted_atoms,
            impacted_answers,
            impacted_simulations,
            warning: "impact analysis is dependency reasoning, not fact or simulation truth"
                .to_owned(),
        }
    }

    pub fn collapse_if_source_false(&self, source_id: &AtomId) -> Option<TruthCollapseReport> {
        self.atoms.get(source_id)?;
        let root = DependencyNode::Atom(source_id.clone());
        let nodes = self.dependencies.transitive_dependents(&root);
        let mut collapsed_atoms = Vec::new();
        let mut collapsed_beliefs = Vec::new();
        let mut collapsed_memories = Vec::new();
        let mut collapsed_plans = Vec::new();
        let mut collapsed_answers = Vec::new();
        let mut collapsed_simulations = Vec::new();

        for node in nodes {
            match node {
                DependencyNode::Atom(atom_id) => {
                    collapsed_atoms.push(atom_id.clone());
                    if let Some(atom) = self.atoms.get(&atom_id) {
                        if is_belief_atom(atom) {
                            collapsed_beliefs.push(atom_id.clone());
                        } else if is_plan_atom(atom) {
                            collapsed_plans.push(atom_id.clone());
                        } else if matches!(atom.claim_type, ClaimType::AgentMemory) {
                            collapsed_memories.push(atom_id.clone());
                        }
                    }
                }
                DependencyNode::Answer(answer_id) => collapsed_answers.push(answer_id),
                DependencyNode::Simulation(simulation_id) => {
                    collapsed_simulations.push(simulation_id);
                }
            }
        }

        sort_and_dedup_atom_ids(&mut collapsed_atoms);
        sort_and_dedup_atom_ids(&mut collapsed_beliefs);
        sort_and_dedup_atom_ids(&mut collapsed_memories);
        sort_and_dedup_atom_ids(&mut collapsed_plans);
        collapsed_answers.sort();
        collapsed_answers.dedup();
        collapsed_simulations.sort();
        collapsed_simulations.dedup();

        Some(TruthCollapseReport {
            root_source: source_id.clone(),
            collapsed_atoms,
            collapsed_beliefs,
            collapsed_memories,
            collapsed_plans,
            collapsed_answers,
            collapsed_simulations,
            dependency_steps: self.dependencies.trace_from(&root),
            warning:
                "source-false collapse analysis is dependency reasoning, not a factual conclusion"
                    .to_owned(),
        })
    }

    pub fn insert_causal_atom(&mut self, atom: CausalAtom) -> Result<(), KernelError> {
        atom.validate()?;
        self.causal_outgoing
            .entry(atom.cause.clone())
            .or_default()
            .push(atom.clone());
        self.causal_incoming
            .entry(atom.effect.clone())
            .or_default()
            .push(atom);
        for atoms in self.causal_outgoing.values_mut() {
            sort_causal_atoms(atoms);
        }
        for atoms in self.causal_incoming.values_mut() {
            sort_causal_atoms(atoms);
        }
        Ok(())
    }

    pub fn causal_atoms_from(&self, event_id: &EventId) -> &[CausalAtom] {
        self.causal_outgoing
            .get(event_id)
            .map_or(&[], Vec::as_slice)
    }

    pub fn what_caused(&self, event_id: &EventId, max_depth: usize) -> Vec<CausalPath> {
        let mut paths = Vec::new();
        self.walk_upstream(
            event_id,
            max_depth,
            Vec::new(),
            &mut BTreeSet::new(),
            &mut paths,
        );
        sort_causal_paths_longest_first(&mut paths);
        paths
    }

    pub fn what_might_happen_next(&self, event_id: &EventId, max_depth: usize) -> Vec<CausalPath> {
        let mut paths = Vec::new();
        self.walk_downstream(
            event_id,
            max_depth,
            Vec::new(),
            &mut BTreeSet::new(),
            &mut paths,
        );
        sort_causal_paths_shortest_first(&mut paths);
        paths
    }

    pub fn what_breaks_if_event_does_not_occur(
        &self,
        event_id: &EventId,
        max_depth: usize,
    ) -> CausalImpactReport {
        let affected_paths = self.what_might_happen_next(event_id, max_depth);
        let mut affected_events = Vec::new();
        let mut downstream_risks = Vec::new();
        let mut counterfactual_notes = Vec::new();

        for path in &affected_paths {
            for affected_event in path.event_ids().into_iter().skip(1) {
                push_unique_event(&mut affected_events, affected_event.clone());
                push_unique_string(&mut downstream_risks, affected_event.to_string());
            }
            for note in &path.counterfactual_notes {
                push_unique_string(&mut counterfactual_notes, note.clone());
            }
        }

        CausalImpactReport {
            intervention: event_id.clone(),
            affected_events,
            affected_paths,
            downstream_risks,
            counterfactual_notes,
            warning: "counterfactual causal impact is simulation and strategy support, not fact"
                .to_owned(),
        }
    }

    fn walk_upstream(
        &self,
        current: &EventId,
        remaining_depth: usize,
        suffix: Vec<CausalAtom>,
        visited: &mut BTreeSet<EventId>,
        paths: &mut Vec<CausalPath>,
    ) {
        if remaining_depth == 0 || !visited.insert(current.clone()) {
            return;
        }
        if let Some(incoming) = self.causal_incoming.get(current) {
            for atom in incoming {
                let mut atoms = Vec::with_capacity(suffix.len() + 1);
                atoms.push(atom.clone());
                atoms.extend(suffix.iter().cloned());
                paths.push(causal_path_from_atoms(&atoms));
                self.walk_upstream(&atom.cause, remaining_depth - 1, atoms, visited, paths);
            }
        }
        visited.remove(current);
    }

    fn walk_downstream(
        &self,
        current: &EventId,
        remaining_depth: usize,
        prefix: Vec<CausalAtom>,
        visited: &mut BTreeSet<EventId>,
        paths: &mut Vec<CausalPath>,
    ) {
        if remaining_depth == 0 || !visited.insert(current.clone()) {
            return;
        }
        if let Some(outgoing) = self.causal_outgoing.get(current) {
            for atom in outgoing {
                let mut atoms = prefix.clone();
                atoms.push(atom.clone());
                paths.push(causal_path_from_atoms(&atoms));
                self.walk_downstream(&atom.effect, remaining_depth - 1, atoms, visited, paths);
            }
        }
        visited.remove(current);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RealityQuery {
    EntityState {
        entity: EntityRef,
        valid_at: ValidTime,
        known_at: TxTime,
        ai_facing: bool,
    },
    WhatIsTrueNow {
        entity: EntityRef,
        now: BitemporalTruth,
        ai_facing: bool,
    },
    WhatWasTrueAt {
        entity: EntityRef,
        valid_at: ValidTime,
        known_at: TxTime,
        ai_facing: bool,
    },
    WhatDidWeBelieveAt {
        entity: EntityRef,
        valid_at: ValidTime,
        believed_at: TxTime,
        ai_facing: bool,
    },
    WhenDidBeliefChange {
        atom_id: AtomId,
    },
    IfSourceFalseWhatCollapses {
        source_atom_id: AtomId,
    },
    WhatCaused {
        event_id: EventId,
        max_depth: usize,
    },
    WhatMightHappenNext {
        event_id: EventId,
        max_depth: usize,
    },
    WhatBreaksIfEventDoesNotOccur {
        event_id: EventId,
        max_depth: usize,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct RealityQueryResult {
    pub question: BitemporalQuestion,
    pub truth: Option<BitemporalTruth>,
    pub atoms: Vec<RealityAtom>,
    pub conflicts: Vec<ConflictSet>,
    pub evidence: Vec<EvidenceSpan>,
    pub unsupported_conclusions: Vec<RealityAtom>,
    pub belief_changes: Vec<BeliefRevision>,
    pub collapse_report: Option<TruthCollapseReport>,
    pub causal_paths: Vec<CausalPath>,
    pub causal_impact: Option<CausalImpactReport>,
}

pub struct RealityQueryVm<'a> {
    kernel: &'a RealityKernel,
}

impl<'a> RealityQueryVm<'a> {
    pub fn new(kernel: &'a RealityKernel) -> Self {
        Self { kernel }
    }

    pub fn execute(&self, query: RealityQuery) -> RealityQueryResult {
        match query {
            RealityQuery::EntityState {
                entity,
                valid_at,
                known_at,
                ai_facing,
            } => self.execute_entity_state(
                entity,
                BitemporalTruth::new(valid_at, known_at),
                ai_facing,
                BitemporalQuestion::EntityState,
            ),
            RealityQuery::WhatIsTrueNow {
                entity,
                now,
                ai_facing,
            } => {
                self.execute_entity_state(entity, now, ai_facing, BitemporalQuestion::WhatIsTrueNow)
            }
            RealityQuery::WhatWasTrueAt {
                entity,
                valid_at,
                known_at,
                ai_facing,
            } => self.execute_entity_state(
                entity,
                BitemporalTruth::new(valid_at, known_at),
                ai_facing,
                BitemporalQuestion::WhatWasTrueAt,
            ),
            RealityQuery::WhatDidWeBelieveAt {
                entity,
                valid_at,
                believed_at,
                ai_facing,
            } => self.execute_entity_state(
                entity,
                BitemporalTruth::new(valid_at, believed_at),
                ai_facing,
                BitemporalQuestion::WhatDidWeBelieveAt,
            ),
            RealityQuery::WhenDidBeliefChange { atom_id } => self.execute_belief_changes(atom_id),
            RealityQuery::IfSourceFalseWhatCollapses { source_atom_id } => {
                self.execute_source_false_collapse(source_atom_id)
            }
            RealityQuery::WhatCaused {
                event_id,
                max_depth,
            } => self.execute_what_caused(event_id, max_depth),
            RealityQuery::WhatMightHappenNext {
                event_id,
                max_depth,
            } => self.execute_what_might_happen_next(event_id, max_depth),
            RealityQuery::WhatBreaksIfEventDoesNotOccur {
                event_id,
                max_depth,
            } => self.execute_what_breaks_if_event_does_not_occur(event_id, max_depth),
        }
    }

    pub fn execute_kernel(&self, query: KernelQuery) -> KernelQueryResult {
        match query {
            KernelQuery::GetAtom(atom_id) => {
                let mut result = KernelQueryResult::default();
                if let Some(atom) = self.kernel.atom(&atom_id).cloned() {
                    self.push_kernel_result_atom(&mut result, atom);
                }
                result
            }
            KernelQuery::FindAtoms(pattern) => {
                let mut result = KernelQueryResult::default();
                for atom in self.kernel_atoms_matching(&pattern) {
                    self.push_kernel_result_atom(&mut result, (*atom).clone());
                }
                self.sort_kernel_result(&mut result);
                result
            }
            KernelQuery::VisibleAt {
                valid_at,
                known_at,
                pattern,
            } => {
                let mut result = KernelQueryResult::default();
                for atom in self
                    .kernel_atoms_matching(&pattern)
                    .into_iter()
                    .filter(|atom| visible_at(atom, valid_at, known_at))
                {
                    self.push_kernel_result_atom(&mut result, (*atom).clone());
                }
                self.sort_kernel_result(&mut result);
                result
            }
            KernelQuery::ExplainSupport { atom_id } => {
                let mut result = KernelQueryResult::default();
                if let Some(atom) = self.kernel.atom(&atom_id).cloned() {
                    self.push_kernel_result_atom(&mut result, atom);
                }
                result.support = self.kernel.explain_support(&atom_id);
                if let Some(support) = &result.support {
                    result
                        .dependency_trace
                        .extend(support.dependency_trace.iter().cloned());
                }
                self.sort_kernel_result(&mut result);
                result
            }
            KernelQuery::ExplainConflict { atom_id } => {
                let mut result = KernelQueryResult::default();
                if let Some(atom) = self.kernel.atom(&atom_id).cloned() {
                    self.push_kernel_result_atom(&mut result, atom);
                }
                result.conflicts = self.kernel.explain_conflict(&atom_id);
                self.sort_kernel_result(&mut result);
                result
            }
            KernelQuery::ImpactIfRetracted { atom_id, max_depth } => {
                let mut result = KernelQueryResult::default();
                let mut impact = self.kernel.compute_impact_if_retracted(&atom_id);
                if max_depth == 0 {
                    impact.impacted_atoms.clear();
                    impact.impacted_answers.clear();
                    impact.impacted_simulations.clear();
                    impact.invalidation_trace.invalidated_nodes.clear();
                    impact.invalidation_trace.steps.clear();
                }
                result
                    .dependency_trace
                    .extend(impact.invalidation_trace.steps.iter().cloned());
                result.impact = Some(impact);
                self.sort_kernel_result(&mut result);
                result
            }
            KernelQuery::EntityState {
                entity_id,
                valid_at,
                known_at,
            } => {
                let mut result = KernelQueryResult::default();
                let state = self
                    .kernel
                    .entity_state(entity_id, valid_at, known_at.into());
                for atom in state
                    .accepted_atoms
                    .into_iter()
                    .chain(state.disputed_atoms)
                    .chain(state.superseded_atoms)
                {
                    self.push_kernel_result_atom(&mut result, atom);
                }
                result.conflicts = state.conflicts;
                self.sort_kernel_result(&mut result);
                result
            }
        }
    }

    pub fn compile_native(&self, query: &NativeRealityQuery) -> NativeRealityPlan {
        let strategy = match &query.kind {
            NativeRealityQueryKind::VerifyClaim(pattern) if pattern.is_fully_bound() => {
                NativeExecutionStrategy::LeapfrogTriejoinCandidate
            }
            NativeRealityQueryKind::VerifyClaim(_) => NativeExecutionStrategy::PointInTimeLookup,
            NativeRealityQueryKind::WhatBreaksIfFalse(_) => {
                NativeExecutionStrategy::CounterfactualImpactSearch
            }
            NativeRealityQueryKind::CausalCauses { .. }
            | NativeRealityQueryKind::CausalEffects { .. } => {
                NativeExecutionStrategy::CausalTraversal
            }
        };
        let mut execution_trace = vec![NativeVmTraceStep::new(
            native_strategy_label(strategy),
            native_strategy_detail(strategy),
        )];
        for operator in &query.operators {
            execution_trace.push(NativeVmTraceStep::new(
                operator.label(),
                native_operator_detail(operator),
            ));
        }
        for return_field in &query.return_fields {
            if *return_field == RealityReturnField::DependencyTrace
                && !query
                    .operators
                    .iter()
                    .any(|operator| matches!(operator, RealityOperator::DependencyTrace))
            {
                execution_trace.push(NativeVmTraceStep::new(
                    "DependencyTrace",
                    "return field requires dependency trace walk",
                ));
            }
        }
        NativeRealityPlan {
            strategy,
            operators: query.operators.clone(),
            return_fields: query.return_fields.clone(),
            execution_trace,
        }
    }

    pub fn execute_native(&self, query: NativeRealityQuery) -> NativeRealityQueryResult {
        let plan = self.compile_native(&query);
        let operators = query.operators.clone();
        let return_fields = query.return_fields.clone();
        let mut result = NativeRealityQueryResult::new(plan);

        match query.kind {
            NativeRealityQueryKind::VerifyClaim(pattern) => {
                self.execute_native_verify(pattern, &operators, &return_fields, &mut result)
            }
            NativeRealityQueryKind::WhatBreaksIfFalse(atom_id) => {
                self.execute_native_what_breaks(atom_id, &return_fields, &mut result)
            }
            NativeRealityQueryKind::CausalCauses {
                event_id,
                max_depth,
            } => {
                result.causal_paths = self.kernel.what_caused(&event_id, max_depth);
            }
            NativeRealityQueryKind::CausalEffects {
                event_id,
                max_depth,
            } => {
                result.causal_paths = self.kernel.what_might_happen_next(&event_id, max_depth);
            }
        }

        result
    }

    fn kernel_atoms_matching(&self, pattern: &AtomPattern) -> Vec<&RealityAtom> {
        self.kernel
            .atoms
            .values()
            .filter(|atom| {
                pattern
                    .subject
                    .as_ref()
                    .map_or(true, |subject| &atom.subject == subject)
                    && pattern
                        .predicate
                        .as_ref()
                        .map_or(true, |predicate| &atom.predicate == predicate)
                    && pattern
                        .object
                        .as_ref()
                        .map_or(true, |object| &atom.object == object)
            })
            .collect()
    }

    fn push_kernel_result_atom(&self, result: &mut KernelQueryResult, atom: RealityAtom) {
        let atom_id = atom.id.clone();
        result.atom_ids.push(atom_id.clone());
        for source_ref in &atom.source_refs {
            result.evidence_ids.push(source_ref.source_id.clone());
        }
        let belief_known_at = atom.transaction_time.start;
        if let Some(belief) = self.kernel.belief_at(&atom_id, belief_known_at) {
            result.beliefs.push((atom_id.clone(), belief));
        }
        result
            .valid_times
            .insert(atom_id.clone(), atom.valid_time.clone());
        result
            .transaction_times
            .insert(atom_id, atom.transaction_time.clone());
        result.atoms.push(atom);
    }

    fn sort_kernel_result(&self, result: &mut KernelQueryResult) {
        sort_and_dedup_atom_ids(&mut result.atom_ids);
        result.evidence_ids.sort();
        result.evidence_ids.dedup();
        result.beliefs.sort_by(|left, right| left.0.cmp(&right.0));
        result
            .beliefs
            .dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
        sort_atoms(&mut result.atoms);
        result
            .conflicts
            .sort_by(|left, right| left.id.cmp(&right.id));
        result.conflicts.dedup_by(|left, right| left.id == right.id);
        sort_dependency_steps(&mut result.dependency_trace);
    }

    fn execute_entity_state(
        &self,
        entity: EntityRef,
        truth: BitemporalTruth,
        ai_facing: bool,
        question: BitemporalQuestion,
    ) -> RealityQueryResult {
        let state = self
            .kernel
            .entity_state(entity.clone(), truth.valid_at, truth.known_at);
        let visible = self
            .kernel
            .atoms
            .values()
            .filter(|atom| {
                atom.subject == entity && atom.is_visible_at(truth.valid_at, truth.known_at)
            })
            .map(|atom| self.with_historical_belief(atom, truth.known_at))
            .collect::<Vec<_>>();
        let mut atoms = Vec::new();
        let mut unsupported_conclusions = Vec::new();
        for atom in visible {
            if ai_facing && !atom.is_supported_for_ai() {
                unsupported_conclusions.push(atom);
            } else if !ai_facing || atom.is_supported_for_ai() {
                atoms.push(atom);
            }
        }
        sort_atoms(&mut atoms);
        sort_atoms(&mut unsupported_conclusions);
        let evidence = atoms
            .iter()
            .flat_map(|atom| atom.evidence_spans.iter().cloned())
            .collect::<Vec<_>>();
        RealityQueryResult {
            question,
            truth: Some(truth),
            atoms,
            conflicts: state.conflicts,
            evidence,
            unsupported_conclusions,
            belief_changes: Vec::new(),
            collapse_report: None,
            causal_paths: Vec::new(),
            causal_impact: None,
        }
    }

    fn execute_belief_changes(&self, atom_id: AtomId) -> RealityQueryResult {
        let mut belief_changes = self.kernel.belief_revisions(&atom_id).to_vec();
        belief_changes.sort_by_key(|revision| revision.known_at);
        RealityQueryResult {
            question: BitemporalQuestion::WhenDidBeliefChange,
            truth: None,
            atoms: Vec::new(),
            conflicts: Vec::new(),
            evidence: Vec::new(),
            unsupported_conclusions: Vec::new(),
            belief_changes,
            collapse_report: None,
            causal_paths: Vec::new(),
            causal_impact: None,
        }
    }

    fn execute_source_false_collapse(&self, source_atom_id: AtomId) -> RealityQueryResult {
        RealityQueryResult {
            question: BitemporalQuestion::IfSourceFalseWhatCollapses,
            truth: None,
            atoms: Vec::new(),
            conflicts: Vec::new(),
            evidence: Vec::new(),
            unsupported_conclusions: Vec::new(),
            belief_changes: Vec::new(),
            collapse_report: self.kernel.collapse_if_source_false(&source_atom_id),
            causal_paths: Vec::new(),
            causal_impact: None,
        }
    }

    fn execute_what_caused(&self, event_id: EventId, max_depth: usize) -> RealityQueryResult {
        RealityQueryResult {
            question: BitemporalQuestion::WhatCaused,
            truth: None,
            atoms: Vec::new(),
            conflicts: Vec::new(),
            evidence: Vec::new(),
            unsupported_conclusions: Vec::new(),
            belief_changes: Vec::new(),
            collapse_report: None,
            causal_paths: self.kernel.what_caused(&event_id, max_depth),
            causal_impact: None,
        }
    }

    fn execute_what_might_happen_next(
        &self,
        event_id: EventId,
        max_depth: usize,
    ) -> RealityQueryResult {
        RealityQueryResult {
            question: BitemporalQuestion::WhatMightHappenNext,
            truth: None,
            atoms: Vec::new(),
            conflicts: Vec::new(),
            evidence: Vec::new(),
            unsupported_conclusions: Vec::new(),
            belief_changes: Vec::new(),
            collapse_report: None,
            causal_paths: self.kernel.what_might_happen_next(&event_id, max_depth),
            causal_impact: None,
        }
    }

    fn execute_what_breaks_if_event_does_not_occur(
        &self,
        event_id: EventId,
        max_depth: usize,
    ) -> RealityQueryResult {
        RealityQueryResult {
            question: BitemporalQuestion::WhatBreaksIfEventDoesNotOccur,
            truth: None,
            atoms: Vec::new(),
            conflicts: Vec::new(),
            evidence: Vec::new(),
            unsupported_conclusions: Vec::new(),
            belief_changes: Vec::new(),
            collapse_report: None,
            causal_paths: Vec::new(),
            causal_impact: Some(
                self.kernel
                    .what_breaks_if_event_does_not_occur(&event_id, max_depth),
            ),
        }
    }

    fn with_historical_belief(&self, atom: &RealityAtom, known_at: TxTime) -> RealityAtom {
        let mut historical = atom.clone();
        if let Some(belief_state) = self.kernel.belief_at(&atom.id, known_at) {
            historical.belief_state = belief_state;
        }
        historical
    }

    fn execute_native_verify(
        &self,
        pattern: ClaimPattern,
        operators: &[RealityOperator],
        return_fields: &[RealityReturnField],
        result: &mut NativeRealityQueryResult,
    ) {
        let valid_at = native_valid_at(operators);
        let known_at = native_known_at(operators);
        let allowed_beliefs = native_allowed_beliefs(operators);
        let allowed_permissions = native_allowed_permissions(operators);
        let require_evidence = operators
            .iter()
            .any(|operator| matches!(operator, RealityOperator::RequireEvidence));

        let physical_store = PhysicalGraphStore::from_atoms(self.kernel.atoms.values().cloned());
        let mut physical_candidates = if pattern.is_fully_bound() {
            physical_store.trie_candidates_for_claim(&pattern)
        } else {
            physical_store.candidates_for_claim_pattern(&pattern)
        };
        if let (Some(valid_at), Some(known_at)) = (valid_at, known_at) {
            physical_candidates = physical_candidates
                .intersect(&physical_store.point_in_time_candidates(valid_at, known_at));
        }
        let mut atoms = physical_store
            .atoms_for_candidates(&physical_candidates)
            .into_iter()
            .filter(|atom| valid_at.map_or(true, |instant| atom.valid_time.contains(instant)))
            .filter(|atom| known_at.map_or(true, |instant| atom.transaction_time.contains(instant)))
            .collect::<Vec<_>>();

        if let Some(allowed) = allowed_permissions {
            let mut retained = Vec::new();
            for atom in atoms {
                if self.kernel.atom_visible_with_permissions(&atom, allowed) {
                    retained.push(atom);
                } else {
                    result.permission_filtered_atoms.push(atom.id);
                }
            }
            atoms = retained;
        }

        if let Some(allowed) = allowed_beliefs {
            atoms.retain(|atom| {
                let belief_known_at = known_at.unwrap_or(atom.transaction_time.start);
                self.kernel
                    .belief_at(&atom.id, belief_known_at)
                    .is_some_and(|belief| allowed.contains(&belief))
            });
        }

        if require_evidence {
            atoms.retain(|atom| !atom.source_refs.is_empty() && !atom.evidence_spans.is_empty());
        }

        sort_atoms(&mut atoms);
        sort_and_dedup_atom_ids(&mut result.permission_filtered_atoms);

        let matched_atom_ids = atoms
            .iter()
            .map(|atom| atom.id.clone())
            .collect::<BTreeSet<_>>();

        if native_wants(return_fields, RealityReturnField::Belief) {
            for atom in &atoms {
                let belief_known_at = known_at.unwrap_or(atom.transaction_time.start);
                if let Some(belief) = self.kernel.belief_at(&atom.id, belief_known_at) {
                    result.beliefs.push((atom.id.clone(), belief));
                }
            }
            result.beliefs.sort_by(|left, right| left.0.cmp(&right.0));
        }

        if native_wants(return_fields, RealityReturnField::Evidence) {
            result.evidence = atoms
                .iter()
                .flat_map(|atom| atom.evidence_spans.iter().cloned())
                .collect();
        }

        if native_wants(return_fields, RealityReturnField::Contradictions)
            || operators
                .iter()
                .any(|operator| matches!(operator, RealityOperator::IncludeContradictions))
        {
            result.contradictions = self
                .kernel
                .conflicts
                .iter()
                .filter(|conflict| {
                    conflict
                        .atom_ids
                        .iter()
                        .any(|atom_id| matched_atom_ids.contains(atom_id))
                })
                .cloned()
                .collect();
            result
                .contradictions
                .sort_by(|left, right| left.id.cmp(&right.id));
        }

        if native_wants(return_fields, RealityReturnField::DependencyTrace)
            || operators
                .iter()
                .any(|operator| matches!(operator, RealityOperator::DependencyTrace))
        {
            for atom_id in &matched_atom_ids {
                result.dependency_trace.extend(
                    self.kernel
                        .dependencies
                        .trace_from(&DependencyNode::Atom(atom_id.clone())),
                );
            }
            sort_dependency_steps(&mut result.dependency_trace);
        }

        result.atoms = atoms;
    }

    fn execute_native_what_breaks(
        &self,
        atom_id: AtomId,
        return_fields: &[RealityReturnField],
        result: &mut NativeRealityQueryResult,
    ) {
        result.execution_trace.push(NativeVmTraceStep::new(
            "DependencyInvalidation",
            "walk transitive dependents from atom assumed false",
        ));
        let report = self.kernel.impact_if_retracted(&atom_id);
        result.warnings.push(report.warning.clone());

        for impacted_atom_id in &report.impacted_atoms {
            if let Some(atom) = self.kernel.atom(impacted_atom_id) {
                if native_wants(return_fields, RealityReturnField::AffectedBeliefs)
                    && is_belief_atom(atom)
                {
                    result.affected_beliefs.push(impacted_atom_id.clone());
                }
                if native_wants(return_fields, RealityReturnField::Plans) && is_plan_atom(atom) {
                    result.affected_plans.push(impacted_atom_id.clone());
                } else if native_wants(return_fields, RealityReturnField::Memories)
                    && matches!(atom.claim_type, ClaimType::AgentMemory)
                {
                    result.affected_memories.push(impacted_atom_id.clone());
                }
                if native_wants(return_fields, RealityReturnField::Summaries)
                    && matches!(atom.claim_type, ClaimType::Summary)
                {
                    result.affected_summaries.push(impacted_atom_id.clone());
                }
                if native_wants(return_fields, RealityReturnField::Agents) {
                    if let Some(agent_id) = &atom.agent_scope {
                        result.affected_agents.push(agent_id.clone());
                    }
                }
            }
        }

        sort_and_dedup_atom_ids(&mut result.affected_beliefs);
        sort_and_dedup_atom_ids(&mut result.affected_plans);
        sort_and_dedup_atom_ids(&mut result.affected_memories);
        sort_and_dedup_atom_ids(&mut result.affected_summaries);
        sort_and_dedup_agents(&mut result.affected_agents);
        result.impact_report = Some(report);
    }
}

fn sorted_set_values<T: Clone + Ord>(values: Option<&BTreeSet<T>>) -> Vec<T> {
    values
        .map(|set| set.iter().cloned().collect())
        .unwrap_or_default()
}

fn self_revision_suggestion(
    job: SelfRevisionJob,
    kind: SelfRevisionSuggestionKind,
    target: SelfRevisionTarget,
    destructive_if_applied: bool,
    explanation: String,
    mut evidence: Vec<String>,
    dependency_trace: Vec<DependencyStep>,
) -> SelfRevisionSuggestion {
    evidence.sort();
    evidence.dedup();
    let id = format!(
        "self-revision-{}-{}-{}",
        job.slug(),
        kind.slug(),
        self_revision_target_slug(&target)
    );
    SelfRevisionSuggestion {
        audit_event_id: format!("audit-{id}"),
        id,
        job,
        kind,
        target,
        requires_review: true,
        destructive_if_applied,
        auto_applied: false,
        explanation,
        evidence,
        dependency_trace,
    }
}

fn self_revision_report_id(jobs: &[SelfRevisionJob], run_at: TxTime) -> String {
    format!(
        "self-revision-{}-{}",
        jobs.iter()
            .map(|job| job.slug())
            .collect::<Vec<_>>()
            .join("-"),
        run_at.as_i64()
    )
}

fn self_revision_target_slug(target: &SelfRevisionTarget) -> String {
    match target {
        SelfRevisionTarget::EntityPair { left, right } => {
            format!("entity-pair-{left}-{right}")
        }
        SelfRevisionTarget::Source(source_id) => format!("source-{source_id}"),
        SelfRevisionTarget::Predicate(predicate_id) => format!("predicate-{predicate_id}"),
        SelfRevisionTarget::Conflict(conflict_id) => format!("conflict-{conflict_id}"),
        SelfRevisionTarget::Summary(atom_id) => format!("summary-{atom_id}"),
        SelfRevisionTarget::MemorySet { agent_id, atom_ids } => format!(
            "memory-set-{}-{}",
            agent_id,
            atom_ids
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("-")
        ),
        SelfRevisionTarget::Atom(atom_id) => format!("atom-{atom_id}"),
        SelfRevisionTarget::DependencyRoot(atom_id) => format!("dependency-root-{atom_id}"),
        SelfRevisionTarget::CausalHypothesis { cause, effect } => {
            format!("causal-hypothesis-{cause}-{effect}")
        }
    }
}

fn sort_self_revision_suggestions(suggestions: &mut [SelfRevisionSuggestion]) {
    suggestions.sort_by(|left, right| left.id.cmp(&right.id));
}

fn self_revision_changed_since(transaction_time: TxTime, cursor: SelfRevisionCursor) -> bool {
    match cursor.after_tx {
        Some(after_tx) => transaction_time > after_tx,
        None => true,
    }
}

fn self_revision_text_value(atom: &RealityAtom) -> Option<String> {
    match &atom.object {
        ValueOrEntity::Value(GraphValue::Text(value)) => Some(value.clone()),
        ValueOrEntity::Entity(entity_ref) => Some(entity_ref.to_string()),
        ValueOrEntity::Value(value) => Some(format!("{value:?}")),
    }
}

fn normalize_self_revision_text(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn conflicted_atom_ids(kernel: &RealityKernel) -> BTreeSet<AtomId> {
    kernel
        .conflicts
        .iter()
        .flat_map(|conflict| conflict.atom_ids.iter().cloned())
        .collect()
}

fn touch_view(views: &mut MaintainedViews, delta: &mut IncrementalDelta, view: MaintainedViewName) {
    views.touch(view, delta.sequence);
    delta.touch_view(view);
}

fn intern_symbol<T: Clone + Ord>(symbols: &mut BTreeMap<T, u32>, value: T) -> u32 {
    if let Some(symbol) = symbols.get(&value) {
        *symbol
    } else {
        let symbol = symbols.len() as u32;
        symbols.insert(value, symbol);
        symbol
    }
}

fn intern_string_symbol(symbols: &mut BTreeMap<String, u32>, value: String) -> u32 {
    intern_symbol(symbols, value)
}

fn push_candidate_index<K: Ord>(
    index: &mut BTreeMap<K, CandidateBitmap>,
    key: K,
    ordinal: AtomOrdinal,
) {
    index.entry(key).or_default().ordinals.push(ordinal);
}

fn compact_candidate_map<K: Ord>(index: &mut BTreeMap<K, CandidateBitmap>) {
    for candidates in index.values_mut() {
        *candidates = CandidateBitmap::from_unsorted(candidates.ordinals.clone());
    }
}

fn object_symbol_key(object: &ValueOrEntity) -> String {
    match object {
        ValueOrEntity::Entity(entity) => format!("entity:{entity}"),
        ValueOrEntity::Value(GraphValue::Entity(entity)) => format!("entity:{entity}"),
        ValueOrEntity::Value(GraphValue::Text(value)) => format!("text:{value}"),
        ValueOrEntity::Value(GraphValue::Integer(value)) => format!("int:{value}"),
        ValueOrEntity::Value(GraphValue::Decimal(value)) => format!("decimal:{value}"),
        ValueOrEntity::Value(GraphValue::Boolean(value)) => format!("bool:{value}"),
        ValueOrEntity::Value(GraphValue::Time(value)) => format!("time:{}", value.as_i64()),
        ValueOrEntity::Value(GraphValue::Null) => "null".to_owned(),
    }
}

fn context_key(context: &KernelContextScope) -> String {
    match context {
        KernelContextScope::Global => "global".to_owned(),
        KernelContextScope::Named(name) => format!("named:{name}"),
    }
}

fn model_context_time_visible(atom: &RealityAtom, request: &ModelContextRequest) -> bool {
    let valid_visible = request.valid_at.map_or(true, |valid_at| {
        atom.valid_time.contains(valid_at) || matches!(atom.claim_type, ClaimType::AgentMemory)
    });
    let known_visible = request
        .known_at
        .map_or(true, |known_at| atom.transaction_time.contains(known_at));
    valid_visible && known_visible
}

fn permission_scope_allows(
    atom_permission: &PermissionLabel,
    permission_scope: &[PermissionLabel],
) -> bool {
    permission_scope.is_empty() || permission_scope.contains(atom_permission)
}

fn model_context_relevance(atom: &RealityAtom, request: &ModelContextRequest) -> usize {
    let task = format!(
        "{} {}",
        request.task,
        request.current_goal.clone().unwrap_or_default()
    )
    .to_ascii_lowercase();
    let mut score = 0;
    if task.contains(&atom.subject.as_str().to_ascii_lowercase()) {
        score += 4;
    }
    if task.contains(&atom.predicate.as_str().to_ascii_lowercase()) {
        score += 3;
    }
    let object = value_or_entity_label(&atom.object).to_ascii_lowercase();
    if task.contains(&object) {
        score += 4;
    }
    if matches!(atom.claim_type, ClaimType::AgentMemory)
        && atom.agent_scope.as_ref() == Some(&request.agent_id)
    {
        score += 5;
    }
    if atom.belief_state == BeliefState::Disputed {
        score += 2;
    }
    score
}

fn estimate_model_context_tokens(atom: &RealityAtom) -> usize {
    let evidence_cost = atom
        .evidence_spans
        .iter()
        .map(|span| (span.quote.len() / 16).max(1))
        .sum::<usize>();
    16 + evidence_cost + atom.source_refs.len() * 2
}

fn value_or_entity_label(value: &ValueOrEntity) -> String {
    match value {
        ValueOrEntity::Entity(entity) => entity.to_string(),
        ValueOrEntity::Value(GraphValue::Entity(entity)) => entity.to_string(),
        ValueOrEntity::Value(GraphValue::Text(value)) => value.clone(),
        ValueOrEntity::Value(GraphValue::Integer(value)) => value.to_string(),
        ValueOrEntity::Value(GraphValue::Decimal(value)) => value.to_string(),
        ValueOrEntity::Value(GraphValue::Boolean(value)) => value.to_string(),
        ValueOrEntity::Value(GraphValue::Time(value)) => value.as_i64().to_string(),
        ValueOrEntity::Value(GraphValue::Null) => "null".to_owned(),
    }
}

fn model_context_caveat(atom: &RealityAtom) -> Option<String> {
    if atom.belief_state == BeliefState::Disputed {
        Some("belief is disputed; preserve contradiction context".to_owned())
    } else {
        match &atom.ai_usage {
            AiUsage::SafeForPlanning { caveat } => caveat.clone(),
            AiUsage::UseWithCaution(caveat)
            | AiUsage::UnsafeForPlanning(caveat)
            | AiUsage::SimulationOnly(caveat) => Some(caveat.clone()),
        }
    }
}

fn model_context_recommended_actions(
    request: &ModelContextRequest,
    contradictions: &[ConflictSet],
    missing_information: &[MissingInformation],
    permission_filtered_atoms: &[AtomId],
) -> Vec<RecommendedContextAction> {
    let mut actions = Vec::new();
    if !contradictions.is_empty() {
        actions.push(RecommendedContextAction {
            kind: RecommendedActionKind::ReviewContradiction,
            reason: "contradictory evidence is present in the compiled context".to_owned(),
            target: contradictions.first().map(|conflict| conflict.id.clone()),
        });
    }
    if !missing_information.is_empty() || !permission_filtered_atoms.is_empty() {
        actions.push(RecommendedContextAction {
            kind: RecommendedActionKind::RetrieveEvidence,
            reason: "compiled context has missing or permission-filtered evidence".to_owned(),
            target: None,
        });
    }
    if request.risk_level >= RiskLevel::High {
        actions.push(RecommendedContextAction {
            kind: RecommendedActionKind::VerifyClaim,
            reason: "high-risk task should verify claims before model action".to_owned(),
            target: request.current_goal.clone(),
        });
    }
    if request.task.to_ascii_lowercase().contains("what if")
        || request.task.to_ascii_lowercase().contains("simulate")
    {
        actions.push(RecommendedContextAction {
            kind: RecommendedActionKind::RunCounterfactual,
            reason: "task asks for simulation-shaped reasoning".to_owned(),
            target: request.current_goal.clone(),
        });
    }
    actions
}

fn native_valid_at(operators: &[RealityOperator]) -> Option<ValidTime> {
    operators.iter().find_map(|operator| match operator {
        RealityOperator::ValidAt(instant) => Some(*instant),
        _ => None,
    })
}

fn native_known_at(operators: &[RealityOperator]) -> Option<TxTime> {
    operators.iter().find_map(|operator| match operator {
        RealityOperator::KnownAt(instant) => Some(*instant),
        _ => None,
    })
}

fn native_allowed_beliefs(operators: &[RealityOperator]) -> Option<&[BeliefState]> {
    operators.iter().find_map(|operator| match operator {
        RealityOperator::BeliefIn(beliefs) => Some(beliefs.as_slice()),
        _ => None,
    })
}

fn native_allowed_permissions(operators: &[RealityOperator]) -> Option<&[PermissionLabel]> {
    operators.iter().find_map(|operator| match operator {
        RealityOperator::AllowPermissions(permissions) => Some(permissions.as_slice()),
        _ => None,
    })
}

fn native_wants(return_fields: &[RealityReturnField], field: RealityReturnField) -> bool {
    return_fields.is_empty() || return_fields.contains(&field)
}

fn native_strategy_label(strategy: NativeExecutionStrategy) -> &'static str {
    match strategy {
        NativeExecutionStrategy::PointInTimeLookup => "PointInTimeLookup",
        NativeExecutionStrategy::LeapfrogTriejoinCandidate => "LeapfrogTriejoinCandidate",
        NativeExecutionStrategy::CausalTraversal => "CausalTraversal",
        NativeExecutionStrategy::DependencyInvalidation => "DependencyInvalidation",
        NativeExecutionStrategy::CounterfactualImpactSearch => "CounterfactualImpactSearch",
    }
}

fn native_strategy_detail(strategy: NativeExecutionStrategy) -> &'static str {
    match strategy {
        NativeExecutionStrategy::PointInTimeLookup => {
            "use point-in-time filters over maintained atom indexes"
        }
        NativeExecutionStrategy::LeapfrogTriejoinCandidate => {
            "fully-bound conjunctive claim can later lower to a worst-case optimal join"
        }
        NativeExecutionStrategy::CausalTraversal => {
            "use causal indexes, separate from normal edges"
        }
        NativeExecutionStrategy::DependencyInvalidation => {
            "walk truth-maintenance dependency graph"
        }
        NativeExecutionStrategy::CounterfactualImpactSearch => {
            "counterfactual dependency impact, not fact"
        }
    }
}

fn native_operator_detail(operator: &RealityOperator) -> String {
    match operator {
        RealityOperator::ValidAt(instant) => format!("valid_at={}", instant.as_i64()),
        RealityOperator::KnownAt(instant) => format!("known_at={}", instant.as_i64()),
        RealityOperator::BeliefIn(beliefs) => format!("beliefs={beliefs:?}"),
        RealityOperator::RequireEvidence => "require source refs and evidence spans".to_owned(),
        RealityOperator::IncludeContradictions => "return visible conflict sets".to_owned(),
        RealityOperator::AllowPermissions(permissions) => format!("permissions={permissions:?}"),
        RealityOperator::DependencyTrace => "return dependency trace".to_owned(),
        RealityOperator::CausalCauses {
            event_id,
            max_depth,
        } => format!("causes_of={event_id}, max_depth={max_depth}"),
        RealityOperator::CausalEffects {
            event_id,
            max_depth,
        } => format!("effects_of={event_id}, max_depth={max_depth}"),
        RealityOperator::CounterfactualAtomFalse { atom_id } => {
            format!("if {atom_id} is false")
        }
        RealityOperator::SimulationOnly => "simulation output must not be labeled fact".to_owned(),
    }
}

fn sort_dependency_steps(steps: &mut Vec<DependencyStep>) {
    steps.sort_by(|left, right| {
        left.from
            .cmp(&right.from)
            .then_with(|| left.to.cmp(&right.to))
            .then_with(|| {
                left.dependency_type
                    .label()
                    .cmp(right.dependency_type.label())
            })
    });
    steps.dedup_by(|left, right| {
        left.from == right.from
            && left.to == right.to
            && left.dependency_type == right.dependency_type
            && left.explanation == right.explanation
    });
}

fn sort_and_dedup_agents(agent_ids: &mut Vec<AgentId>) {
    agent_ids.sort();
    agent_ids.dedup();
}

fn sort_atoms(atoms: &mut [RealityAtom]) {
    atoms.sort_by(|left, right| left.id.cmp(&right.id));
}

fn sort_and_dedup_atom_ids(atom_ids: &mut Vec<AtomId>) {
    atom_ids.sort();
    atom_ids.dedup();
}

fn is_belief_atom(atom: &RealityAtom) -> bool {
    matches!(atom.claim_type, ClaimType::Derived) || atom.predicate.as_str().contains("BELIEF")
}

fn is_plan_atom(atom: &RealityAtom) -> bool {
    atom.predicate.as_str().contains("PLAN")
}

fn causal_path_from_atoms(atoms: &[CausalAtom]) -> CausalPath {
    let start = atoms
        .first()
        .expect("causal path requires at least one atom")
        .cause
        .clone();
    let end = atoms
        .last()
        .expect("causal path requires at least one atom")
        .effect
        .clone();
    let confidence = atoms
        .iter()
        .fold(1.0_f32, |accumulator, atom| {
            accumulator * atom.confidence.as_f32()
        })
        .clamp(0.0, 1.0);
    let mut mechanisms = Vec::new();
    let mut evidence = Vec::new();
    let mut counterfactual_notes = Vec::new();

    for atom in atoms {
        if let Some(mechanism) = &atom.mechanism {
            push_unique_string(&mut mechanisms, mechanism.clone());
        }
        for source_id in &atom.evidence {
            push_unique_source(&mut evidence, source_id.clone());
        }
        for note in &atom.counterfactual_notes {
            push_unique_string(&mut counterfactual_notes, note.clone());
        }
    }

    CausalPath {
        start,
        end,
        atoms: atoms.to_vec(),
        confidence: Confidence::new(confidence).expect("clamped confidence is valid"),
        mechanisms,
        evidence,
        counterfactual_notes,
    }
}

fn sort_causal_atoms(atoms: &mut [CausalAtom]) {
    atoms.sort_by(|left, right| {
        left.cause
            .cmp(&right.cause)
            .then_with(|| left.effect.cmp(&right.effect))
            .then_with(|| left.mechanism.cmp(&right.mechanism))
    });
}

fn sort_causal_paths_longest_first(paths: &mut [CausalPath]) {
    paths.sort_by(|left, right| {
        right
            .atoms
            .len()
            .cmp(&left.atoms.len())
            .then_with(|| left.event_ids().cmp(&right.event_ids()))
    });
}

fn sort_causal_paths_shortest_first(paths: &mut [CausalPath]) {
    paths.sort_by(|left, right| {
        left.atoms
            .len()
            .cmp(&right.atoms.len())
            .then_with(|| left.event_ids().cmp(&right.event_ids()))
    });
}

fn push_unique_event(events: &mut Vec<EventId>, event_id: EventId) {
    if !events.contains(&event_id) {
        events.push(event_id);
    }
}

fn push_unique_source(sources: &mut Vec<SourceId>, source_id: SourceId) {
    if !sources.contains(&source_id) {
        sources.push(source_id);
    }
}

fn push_unique_string(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}
