//! Self-healing maintenance jobs for Reality Graph.

use std::collections::{BTreeMap, BTreeSet};

use rg_core::{
    Assertion, AssertionId, AssertionStatus, Entity, EntityId, GraphValue, SourceId, TxTime,
};
use rg_events::GraphState;
use rg_index::TemporalIndex;
use rg_storage::InMemoryStorage;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MaintenanceJob {
    DetectDuplicateEntities,
    SuggestEntityMerges,
    DetectStaleAssertions,
    DetectContradictions,
    RefreshSummaries,
    RecomputeCommunities,
    RecalibrateSourceTrust,
    CompactEventLog,
    RebuildIndexes,
}

impl MaintenanceJob {
    pub fn all() -> Vec<Self> {
        vec![
            Self::DetectDuplicateEntities,
            Self::SuggestEntityMerges,
            Self::DetectStaleAssertions,
            Self::DetectContradictions,
            Self::RefreshSummaries,
            Self::RecomputeCommunities,
            Self::RecalibrateSourceTrust,
            Self::CompactEventLog,
            Self::RebuildIndexes,
        ]
    }

    fn slug(self) -> &'static str {
        match self {
            Self::DetectDuplicateEntities => "detect-duplicate-entities",
            Self::SuggestEntityMerges => "suggest-entity-merges",
            Self::DetectStaleAssertions => "detect-stale-assertions",
            Self::DetectContradictions => "detect-contradictions",
            Self::RefreshSummaries => "refresh-summaries",
            Self::RecomputeCommunities => "recompute-communities",
            Self::RecalibrateSourceTrust => "recalibrate-source-trust",
            Self::CompactEventLog => "compact-event-log",
            Self::RebuildIndexes => "rebuild-indexes",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MaintenanceCursor {
    pub after_tx: Option<TxTime>,
}

impl MaintenanceCursor {
    pub fn from_tx(after_tx: TxTime) -> Self {
        Self {
            after_tx: Some(after_tx),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaintenancePolicy {
    pub run_at: TxTime,
    pub stale_tx_lag: i64,
    pub low_trust_threshold: f32,
    pub auto_apply_destructive: bool,
}

impl MaintenancePolicy {
    pub fn review_only(run_at: TxTime) -> Self {
        Self {
            run_at,
            stale_tx_lag: 10_000,
            low_trust_threshold: 0.35,
            auto_apply_destructive: false,
        }
    }

    pub fn with_stale_tx_lag(mut self, stale_tx_lag: i64) -> Self {
        self.stale_tx_lag = stale_tx_lag.max(0);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewStatus {
    Pending,
    Approved,
    Rejected,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MaintenanceActionKind {
    SuggestEntityMerge,
    MarkAssertionStale,
    ClusterContradictions,
    RefreshSummary,
    RecomputeCommunity,
    RecalibrateSourceTrust,
    CompactEventLog,
    RebuildIndexes,
    ReportBrokenRelationship,
}

impl MaintenanceActionKind {
    fn slug(self) -> &'static str {
        match self {
            Self::SuggestEntityMerge => "suggest-entity-merge",
            Self::MarkAssertionStale => "mark-assertion-stale",
            Self::ClusterContradictions => "cluster-contradictions",
            Self::RefreshSummary => "refresh-summary",
            Self::RecomputeCommunity => "recompute-community",
            Self::RecalibrateSourceTrust => "recalibrate-source-trust",
            Self::CompactEventLog => "compact-event-log",
            Self::RebuildIndexes => "rebuild-indexes",
            Self::ReportBrokenRelationship => "report-broken-relationship",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MaintenanceTarget {
    Entity(EntityId),
    EntityPair {
        left: EntityId,
        right: EntityId,
    },
    Assertion(AssertionId),
    Source(SourceId),
    Summary(String),
    Community(String),
    EventLog,
    Indexes,
    ContradictionCluster {
        assertion_ids: Vec<AssertionId>,
    },
    BrokenRelationship {
        assertion_id: AssertionId,
        missing_entity: EntityId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaintenanceAction {
    pub id: String,
    pub kind: MaintenanceActionKind,
    pub target: MaintenanceTarget,
    pub requires_review: bool,
    pub destructive_if_applied: bool,
    pub auto_applied: bool,
    pub explanation: String,
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditEntry {
    pub at: TxTime,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphHealthSnapshot {
    pub recorded_at: TxTime,
    pub entity_count: usize,
    pub assertion_count: usize,
    pub source_count: usize,
    pub duplicate_entity_candidates: usize,
    pub stale_assertion_count: usize,
    pub contradiction_count: usize,
    pub broken_relationship_count: usize,
    pub low_trust_source_count: usize,
    pub health_score: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MaintenanceReport {
    pub id: String,
    pub job: MaintenanceJob,
    pub run_at: TxTime,
    pub cursor: MaintenanceCursor,
    pub next_cursor: MaintenanceCursor,
    pub incremental: bool,
    pub review_status: ReviewStatus,
    pub actions: Vec<MaintenanceAction>,
    pub graph_health: GraphHealthSnapshot,
    pub audit_log: Vec<AuditEntry>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MaintenanceEngine {
    policy: MaintenancePolicy,
    health_history: Vec<GraphHealthSnapshot>,
}

impl MaintenanceEngine {
    pub fn new(policy: MaintenancePolicy) -> Self {
        Self {
            policy,
            health_history: Vec::new(),
        }
    }

    pub fn run_job(
        &mut self,
        job: MaintenanceJob,
        state: &GraphState,
        cursor: MaintenanceCursor,
    ) -> MaintenanceReport {
        match job {
            MaintenanceJob::DetectDuplicateEntities => {
                self.detect_duplicate_entities(state, cursor)
            }
            MaintenanceJob::SuggestEntityMerges => self.suggest_entity_merges(state, cursor),
            MaintenanceJob::DetectStaleAssertions => self.detect_stale_assertions(state, cursor),
            MaintenanceJob::DetectContradictions => self.detect_contradictions(state, cursor),
            MaintenanceJob::RefreshSummaries => self.refresh_summaries(state, cursor),
            MaintenanceJob::RecomputeCommunities => self.recompute_communities(state, cursor),
            MaintenanceJob::RecalibrateSourceTrust => self.recalibrate_source_trust(state, cursor),
            MaintenanceJob::CompactEventLog => self.compact_event_log(state, cursor),
            MaintenanceJob::RebuildIndexes => self.rebuild_indexes(state, cursor),
        }
    }

    pub fn detect_duplicate_entities(
        &mut self,
        state: &GraphState,
        cursor: MaintenanceCursor,
    ) -> MaintenanceReport {
        let pairs = duplicate_entity_pairs(state, cursor);
        let actions = pairs
            .iter()
            .map(|(left, right)| {
                action(
                    MaintenanceActionKind::SuggestEntityMerge,
                    MaintenanceTarget::EntityPair {
                        left: left.id.clone(),
                        right: right.id.clone(),
                    },
                    true,
                    true,
                    self.policy.auto_apply_destructive,
                    format!(
                        "Potential duplicate entities share normalized name and type. No destructive merge was applied; review {} and {} before issuing EntityMerged.",
                        left.id, right.id
                    ),
                    vec![
                        format!("left_name={}", display_name(left)),
                        format!("right_name={}", display_name(right)),
                        format!("entity_type={:?}", left.entity_type),
                    ],
                )
            })
            .collect::<Vec<_>>();
        self.report(
            MaintenanceJob::DetectDuplicateEntities,
            state,
            cursor,
            actions,
            vec!["duplicate entity scan requires operator review before merge".to_owned()],
        )
    }

    pub fn suggest_entity_merges(
        &mut self,
        state: &GraphState,
        cursor: MaintenanceCursor,
    ) -> MaintenanceReport {
        let mut report = self.detect_duplicate_entities(state, cursor);
        report.job = MaintenanceJob::SuggestEntityMerges;
        report.id = report_id(report.job, self.policy.run_at);
        report.audit_log.push(AuditEntry {
            at: self.policy.run_at,
            message:
                "entity merge suggestions are proposals only; no EntityMerged event was appended"
                    .to_owned(),
        });
        self.replace_last_history(report.graph_health.clone());
        report
    }

    pub fn detect_stale_assertions(
        &mut self,
        state: &GraphState,
        cursor: MaintenanceCursor,
    ) -> MaintenanceReport {
        let stale = stale_assertions(state, cursor, self.policy);
        let actions = stale
            .iter()
            .map(|assertion| {
                action(
                    MaintenanceActionKind::MarkAssertionStale,
                    MaintenanceTarget::Assertion(assertion.id.clone()),
                    true,
                    false,
                    false,
                    format!(
                        "Assertion {} has not changed for at least {} transaction ticks; review before retraction or confidence update.",
                        assertion.id, self.policy.stale_tx_lag
                    ),
                    vec![
                        format!("tx_start={}", assertion.transaction_time.start.as_i64()),
                        format!("run_at={}", self.policy.run_at.as_i64()),
                    ],
                )
            })
            .collect::<Vec<_>>();
        self.report(
            MaintenanceJob::DetectStaleAssertions,
            state,
            cursor,
            actions,
            vec![format!(
                "stale assertion scan used incremental cursor {:?}",
                cursor.after_tx.map(TxTime::as_i64)
            )],
        )
    }

    pub fn detect_contradictions(
        &mut self,
        state: &GraphState,
        cursor: MaintenanceCursor,
    ) -> MaintenanceReport {
        let contradictions = contradictions(state, cursor);
        let actions = contradictions
            .iter()
            .map(|contradiction| {
                let assertion_ids = vec![
                    contradiction.assertion_a.clone(),
                    contradiction.assertion_b.clone(),
                ];
                action(
                    MaintenanceActionKind::ClusterContradictions,
                    MaintenanceTarget::ContradictionCluster {
                        assertion_ids: assertion_ids.clone(),
                    },
                    true,
                    false,
                    false,
                    format!(
                        "Contradictory assertions clustered for belief review: {} and {}.",
                        contradiction.assertion_a, contradiction.assertion_b
                    ),
                    vec![
                        contradiction.contradiction_type.to_string(),
                        contradiction.severity.to_string(),
                        contradiction.explanation.clone(),
                    ],
                )
            })
            .collect::<Vec<_>>();
        self.report(
            MaintenanceJob::DetectContradictions,
            state,
            cursor,
            actions,
            vec!["contradiction clustering preserves competing claims for review".to_owned()],
        )
    }

    pub fn refresh_summaries(
        &mut self,
        state: &GraphState,
        cursor: MaintenanceCursor,
    ) -> MaintenanceReport {
        let actions = changed_assertions(state, cursor)
            .into_iter()
            .map(|assertion| {
                action(
                    MaintenanceActionKind::RefreshSummary,
                    MaintenanceTarget::Summary(format!("summary-for-{}", assertion.subject)),
                    true,
                    false,
                    false,
                    format!(
                        "Assertion {} changed; invalidate and refresh only summaries touching {}.",
                        assertion.id, assertion.subject
                    ),
                    vec![assertion.id.to_string()],
                )
            })
            .collect::<Vec<_>>();
        self.report(
            MaintenanceJob::RefreshSummaries,
            state,
            cursor,
            actions,
            vec!["summary refresh job emits invalidation proposals instead of rewriting summaries inline".to_owned()],
        )
    }

    pub fn recompute_communities(
        &mut self,
        state: &GraphState,
        cursor: MaintenanceCursor,
    ) -> MaintenanceReport {
        let mut touched_entities = BTreeSet::new();
        for assertion in changed_assertions(state, cursor) {
            touched_entities.insert(assertion.subject.clone());
            if let GraphValue::Entity(entity_id) = &assertion.object {
                touched_entities.insert(entity_id.clone());
            }
        }
        let actions = touched_entities
            .into_iter()
            .map(|entity_id| {
                action(
                    MaintenanceActionKind::RecomputeCommunity,
                    MaintenanceTarget::Community(format!("community-for-{entity_id}")),
                    true,
                    false,
                    false,
                    format!(
                        "Recompute only the community containing {entity_id}; unchanged communities remain valid."
                    ),
                    vec![entity_id.to_string()],
                )
            })
            .collect::<Vec<_>>();
        self.report(
            MaintenanceJob::RecomputeCommunities,
            state,
            cursor,
            actions,
            vec!["community recomputation is incremental by touched entity set".to_owned()],
        )
    }

    pub fn recalibrate_source_trust(
        &mut self,
        state: &GraphState,
        cursor: MaintenanceCursor,
    ) -> MaintenanceReport {
        let low_trust_sources = state
            .sources
            .values()
            .filter(|source| source.observed_at > cursor.after_tx.unwrap_or(TxTime::new(i64::MIN)))
            .filter(|source| {
                source
                    .trust_score
                    .is_some_and(|score| score < self.policy.low_trust_threshold)
            })
            .collect::<Vec<_>>();
        let actions = low_trust_sources
            .iter()
            .map(|source| {
                action(
                    MaintenanceActionKind::RecalibrateSourceTrust,
                    MaintenanceTarget::Source(source.id.clone()),
                    true,
                    false,
                    false,
                    format!(
                        "Source {} trust score is below {:.2}; review calibration against contradiction and freshness signals.",
                        source.id, self.policy.low_trust_threshold
                    ),
                    vec![format!(
                        "trust_score={:.2}",
                        source.trust_score.unwrap_or_default()
                    )],
                )
            })
            .collect::<Vec<_>>();
        self.report(
            MaintenanceJob::RecalibrateSourceTrust,
            state,
            cursor,
            actions,
            vec!["source trust recalibration is proposed for operator or policy review".to_owned()],
        )
    }

    pub fn compact_event_log(
        &mut self,
        state: &GraphState,
        cursor: MaintenanceCursor,
    ) -> MaintenanceReport {
        let action = action(
            MaintenanceActionKind::CompactEventLog,
            MaintenanceTarget::EventLog,
            true,
            true,
            self.policy.auto_apply_destructive,
            "Event log compaction should create an immutable snapshot and retain replay safety; no compaction was applied automatically.".to_owned(),
            vec![
                format!("entity_count={}", state.entities.len()),
                format!("assertion_count={}", state.assertions.len()),
            ],
        );
        self.report(
            MaintenanceJob::CompactEventLog,
            state,
            cursor,
            vec![action],
            vec![
                "event log compaction remains review gated because it changes storage layout"
                    .to_owned(),
            ],
        )
    }

    pub fn rebuild_indexes(
        &mut self,
        state: &GraphState,
        cursor: MaintenanceCursor,
    ) -> MaintenanceReport {
        let broken_edges = broken_relationships(state);
        let mut actions = broken_edges
            .into_iter()
            .map(|(assertion_id, missing_entity)| {
                action(
                    MaintenanceActionKind::ReportBrokenRelationship,
                    MaintenanceTarget::BrokenRelationship {
                        assertion_id: assertion_id.clone(),
                        missing_entity: missing_entity.clone(),
                    },
                    true,
                    false,
                    false,
                    format!(
                        "Relationship assertion {assertion_id} points at missing entity {missing_entity}; repair source data before rebuilding serving indexes."
                    ),
                    vec![assertion_id.to_string(), missing_entity.to_string()],
                )
            })
            .collect::<Vec<_>>();
        actions.push(action(
            MaintenanceActionKind::RebuildIndexes,
            MaintenanceTarget::Indexes,
            false,
            false,
            false,
            "Rebuild hot serving indexes from current graph state and compare deterministic counts."
                .to_owned(),
            vec![format!("indexed_assertions={}", rebuilt_index_size(state))],
        ));
        self.report(
            MaintenanceJob::RebuildIndexes,
            state,
            cursor,
            actions,
            vec!["index rebuild is repairable from graph state and append log replay".to_owned()],
        )
    }

    pub fn health_history(&self) -> &[GraphHealthSnapshot] {
        &self.health_history
    }

    fn report(
        &mut self,
        job: MaintenanceJob,
        state: &GraphState,
        cursor: MaintenanceCursor,
        mut actions: Vec<MaintenanceAction>,
        audit_messages: Vec<String>,
    ) -> MaintenanceReport {
        suppress_forbidden_auto_apply(&mut actions);
        sort_actions(&mut actions);
        let graph_health = graph_health(state, cursor, self.policy, &actions);
        let mut audit_log = vec![AuditEntry {
            at: self.policy.run_at,
            message: format!(
                "maintenance job {} ran with incremental cursor {:?}",
                job.slug(),
                cursor.after_tx.map(TxTime::as_i64)
            ),
        }];
        audit_log.extend(audit_messages.into_iter().map(|message| AuditEntry {
            at: self.policy.run_at,
            message,
        }));

        self.health_history.push(graph_health.clone());
        MaintenanceReport {
            id: report_id(job, self.policy.run_at),
            job,
            run_at: self.policy.run_at,
            cursor,
            next_cursor: MaintenanceCursor::from_tx(self.policy.run_at),
            incremental: true,
            review_status: ReviewStatus::Pending,
            actions,
            graph_health,
            audit_log,
        }
    }

    fn replace_last_history(&mut self, snapshot: GraphHealthSnapshot) {
        if let Some(last) = self.health_history.last_mut() {
            *last = snapshot;
        }
    }
}

fn duplicate_entity_pairs(
    state: &GraphState,
    cursor: MaintenanceCursor,
) -> Vec<(&Entity, &Entity)> {
    let mut buckets: BTreeMap<(String, String), Vec<&Entity>> = BTreeMap::new();
    for entity in state.entities.values() {
        if !changed_since(entity.created_tx, cursor) {
            continue;
        }
        let Some(name) = &entity.canonical_name else {
            continue;
        };
        buckets
            .entry((format!("{:?}", entity.entity_type), normalize_name(name)))
            .or_default()
            .push(entity);
    }

    let mut pairs = Vec::new();
    for entities in buckets.values_mut() {
        entities.sort_by(|left, right| left.id.cmp(&right.id));
        for left_index in 0..entities.len() {
            for right in entities.iter().skip(left_index + 1) {
                pairs.push((entities[left_index], *right));
            }
        }
    }
    pairs.sort_by(|left, right| {
        left.0
            .id
            .cmp(&right.0.id)
            .then_with(|| left.1.id.cmp(&right.1.id))
    });
    pairs
}

fn stale_assertions(
    state: &GraphState,
    cursor: MaintenanceCursor,
    policy: MaintenancePolicy,
) -> Vec<&Assertion> {
    let mut assertions = state
        .assertions
        .values()
        .filter(|assertion| assertion.status == AssertionStatus::Active)
        .filter(|assertion| changed_since(assertion.transaction_time.start, cursor))
        .filter(|assertion| {
            policy.run_at.as_i64() - assertion.transaction_time.start.as_i64()
                >= policy.stale_tx_lag
        })
        .collect::<Vec<_>>();
    assertions.sort_by(|left, right| left.id.cmp(&right.id));
    assertions
}

fn contradictions(state: &GraphState, cursor: MaintenanceCursor) -> Vec<rg_index::Contradiction> {
    let changed = changed_assertion_ids(state, cursor);
    let mut index = TemporalIndex::new();
    for assertion in state.assertions.values() {
        index.insert_assertion(assertion.clone());
    }
    index
        .contradictions()
        .into_iter()
        .filter(|contradiction| {
            cursor.after_tx.is_none()
                || changed.contains(&contradiction.assertion_a)
                || changed.contains(&contradiction.assertion_b)
        })
        .collect()
}

fn changed_assertions(state: &GraphState, cursor: MaintenanceCursor) -> Vec<&Assertion> {
    let mut assertions = state
        .assertions
        .values()
        .filter(|assertion| changed_since(assertion.transaction_time.start, cursor))
        .collect::<Vec<_>>();
    assertions.sort_by(|left, right| left.id.cmp(&right.id));
    assertions
}

fn changed_assertion_ids(state: &GraphState, cursor: MaintenanceCursor) -> BTreeSet<AssertionId> {
    changed_assertions(state, cursor)
        .into_iter()
        .map(|assertion| assertion.id.clone())
        .collect()
}

fn broken_relationships(state: &GraphState) -> Vec<(AssertionId, EntityId)> {
    let mut broken = Vec::new();
    for assertion in state.assertions.values() {
        if let GraphValue::Entity(entity_id) = &assertion.object {
            if !state.entities.contains_key(entity_id) {
                broken.push((assertion.id.clone(), entity_id.clone()));
            }
        }
    }
    broken.sort();
    broken
}

fn rebuilt_index_size(state: &GraphState) -> usize {
    let events = Vec::new();
    let _storage = InMemoryStorage::replay(&events).expect("empty replay is valid");
    let mut index = TemporalIndex::new();
    for assertion in state.assertions.values() {
        index.insert_assertion(assertion.clone());
    }
    state.assertions.len()
}

fn graph_health(
    state: &GraphState,
    cursor: MaintenanceCursor,
    policy: MaintenancePolicy,
    actions: &[MaintenanceAction],
) -> GraphHealthSnapshot {
    let duplicate_entity_candidates = duplicate_entity_pairs(state, cursor).len();
    let stale_assertion_count = stale_assertions(state, cursor, policy).len();
    let contradiction_count = contradictions(state, cursor).len();
    let broken_relationship_count = broken_relationships(state).len();
    let low_trust_source_count = state
        .sources
        .values()
        .filter(|source| {
            source
                .trust_score
                .is_some_and(|score| score < policy.low_trust_threshold)
        })
        .count();
    let penalty = duplicate_entity_candidates
        + stale_assertion_count
        + contradiction_count
        + broken_relationship_count
        + low_trust_source_count
        + actions
            .iter()
            .filter(|action| action.requires_review)
            .count();
    let health_score = (1.0 - penalty as f32 * 0.05).clamp(0.0, 1.0);
    GraphHealthSnapshot {
        recorded_at: policy.run_at,
        entity_count: state.entities.len(),
        assertion_count: state.assertions.len(),
        source_count: state.sources.len(),
        duplicate_entity_candidates,
        stale_assertion_count,
        contradiction_count,
        broken_relationship_count,
        low_trust_source_count,
        health_score,
    }
}

fn action(
    kind: MaintenanceActionKind,
    target: MaintenanceTarget,
    requires_review: bool,
    destructive_if_applied: bool,
    auto_applied: bool,
    explanation: String,
    evidence: Vec<String>,
) -> MaintenanceAction {
    MaintenanceAction {
        id: action_id(kind, &target),
        kind,
        target,
        requires_review,
        destructive_if_applied,
        auto_applied,
        explanation,
        evidence,
    }
}

fn suppress_forbidden_auto_apply(actions: &mut [MaintenanceAction]) {
    for action in actions {
        if action.destructive_if_applied && action.auto_applied {
            action.auto_applied = false;
            action.requires_review = true;
            action
                .explanation
                .push_str(" Policy forbids destructive automatic repair; action is review-only.");
        }
    }
}

fn sort_actions(actions: &mut [MaintenanceAction]) {
    actions.sort_by(|left, right| left.id.cmp(&right.id));
}

fn changed_since(transaction_time: TxTime, cursor: MaintenanceCursor) -> bool {
    match cursor.after_tx {
        Some(after_tx) => transaction_time > after_tx,
        None => true,
    }
}

fn normalize_name(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn display_name(entity: &Entity) -> String {
    entity
        .canonical_name
        .clone()
        .unwrap_or_else(|| entity.id.to_string())
}

fn report_id(job: MaintenanceJob, run_at: TxTime) -> String {
    format!("maintenance-{}-{}", job.slug(), run_at.as_i64())
}

fn action_id(kind: MaintenanceActionKind, target: &MaintenanceTarget) -> String {
    format!("maintenance-action-{}-{}", kind.slug(), target_slug(target))
}

fn target_slug(target: &MaintenanceTarget) -> String {
    match target {
        MaintenanceTarget::Entity(entity_id) => format!("entity-{entity_id}"),
        MaintenanceTarget::EntityPair { left, right } => format!("entity-pair-{left}-{right}"),
        MaintenanceTarget::Assertion(assertion_id) => format!("assertion-{assertion_id}"),
        MaintenanceTarget::Source(source_id) => format!("source-{source_id}"),
        MaintenanceTarget::Summary(summary_id) => format!("summary-{summary_id}"),
        MaintenanceTarget::Community(community_id) => format!("community-{community_id}"),
        MaintenanceTarget::EventLog => "event-log".to_owned(),
        MaintenanceTarget::Indexes => "indexes".to_owned(),
        MaintenanceTarget::ContradictionCluster { assertion_ids } => format!(
            "contradiction-cluster-{}",
            assertion_ids
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("-")
        ),
        MaintenanceTarget::BrokenRelationship {
            assertion_id,
            missing_entity,
        } => format!("broken-relationship-{assertion_id}-{missing_entity}"),
    }
}
