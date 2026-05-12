//! Context compression and token economics for AI-facing Reality Graph packs.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};

use rg_ai::EvidencePack;
use rg_core::{Assertion, AssertionId, ContradictionId, EntityId, GraphValue, SourceId};
use rg_index::{Contradiction, Severity};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextBudget {
    pub max_tokens: usize,
    pub reserved_output_tokens: usize,
}

impl ContextBudget {
    pub fn new(max_tokens: usize, reserved_output_tokens: usize) -> Self {
        Self {
            max_tokens,
            reserved_output_tokens,
        }
    }

    pub fn available_context_tokens(&self) -> usize {
        self.max_tokens.saturating_sub(self.reserved_output_tokens)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CompressionLevel {
    RawSourceSnippets,
    AssertionList,
    EntityTimeline,
    RelationshipSummary,
    CommunitySummary,
    TaskSpecificDistilledContext,
}

impl CompressionLevel {
    pub fn level_number(self) -> u8 {
        match self {
            Self::RawSourceSnippets => 0,
            Self::AssertionList => 1,
            Self::EntityTimeline => 2,
            Self::RelationshipSummary => 3,
            Self::CommunitySummary => 4,
            Self::TaskSpecificDistilledContext => 5,
        }
    }

    fn all_up_to(max_level: Self) -> Vec<Self> {
        [
            Self::RawSourceSnippets,
            Self::AssertionList,
            Self::EntityTimeline,
            Self::RelationshipSummary,
            Self::CommunitySummary,
            Self::TaskSpecificDistilledContext,
        ]
        .into_iter()
        .filter(|level| level.level_number() <= max_level.level_number())
        .collect()
    }
}

impl fmt::Display for CompressionLevel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RawSourceSnippets => "level_0_raw_source_snippets",
            Self::AssertionList => "level_1_assertion_list",
            Self::EntityTimeline => "level_2_entity_timeline",
            Self::RelationshipSummary => "level_3_relationship_summary",
            Self::CommunitySummary => "level_4_community_summary",
            Self::TaskSpecificDistilledContext => "level_5_task_specific_distilled_context",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CompressionSignal {
    Recency,
    Confidence,
    SourceTrust,
    GraphCentrality,
    TaskRelevance,
    ContradictionImportance,
}

impl CompressionSignal {
    pub fn all() -> Vec<Self> {
        vec![
            Self::Recency,
            Self::Confidence,
            Self::SourceTrust,
            Self::GraphCentrality,
            Self::TaskRelevance,
            Self::ContradictionImportance,
        ]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EvidencePreservationPolicy {
    pub preserve_citations: bool,
    pub preserve_uncertainty: bool,
    pub preserve_temporal_constraints: bool,
    pub preserve_contradictions: bool,
    pub include_why_this_matters: bool,
    pub min_confidence: Option<f32>,
    pub min_source_trust: Option<f32>,
}

impl EvidencePreservationPolicy {
    pub fn strict() -> Self {
        Self {
            preserve_citations: true,
            preserve_uncertainty: true,
            preserve_temporal_constraints: true,
            preserve_contradictions: true,
            include_why_this_matters: true,
            min_confidence: None,
            min_source_trust: None,
        }
    }
}

impl Default for EvidencePreservationPolicy {
    fn default() -> Self {
        Self::strict()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompressionPlan {
    pub budget: ContextBudget,
    pub allowed_levels: Vec<CompressionLevel>,
    pub policy: EvidencePreservationPolicy,
    pub rank_by: Vec<CompressionSignal>,
}

impl CompressionPlan {
    pub fn new(budget: ContextBudget, policy: EvidencePreservationPolicy) -> Self {
        Self {
            budget,
            allowed_levels: CompressionLevel::all_up_to(
                CompressionLevel::TaskSpecificDistilledContext,
            ),
            policy,
            rank_by: default_signals(),
        }
    }

    pub fn default_for_tokens(max_tokens: usize) -> Self {
        Self::new(
            ContextBudget::new(max_tokens, 0),
            EvidencePreservationPolicy::strict(),
        )
    }

    pub fn with_max_level(mut self, max_level: CompressionLevel) -> Self {
        self.allowed_levels = CompressionLevel::all_up_to(max_level);
        self
    }

    pub fn rank_by(mut self, signals: Vec<CompressionSignal>) -> Self {
        self.rank_by = signals;
        self
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CompressionUnitId(String);

impl CompressionUnitId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CompressionUnitId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompressionUnit {
    pub id: CompressionUnitId,
    pub level: CompressionLevel,
    pub text: String,
    pub estimated_tokens: usize,
    pub source_ids: Vec<SourceId>,
    pub assertion_ids: Vec<AssertionId>,
    pub contradiction_ids: Vec<ContradictionId>,
    pub entity_ids: Vec<EntityId>,
    pub score: f64,
    pub why_this_matters: String,
    pub uncertainty: Option<String>,
    pub temporal_constraints: Option<String>,
    pub applied_signals: Vec<CompressionSignal>,
    required: bool,
}

struct CompressionUnitDraft {
    id: String,
    level: CompressionLevel,
    text: String,
    source_ids: Vec<SourceId>,
    assertion_ids: Vec<AssertionId>,
    contradiction_ids: Vec<ContradictionId>,
    entity_ids: Vec<EntityId>,
    why_this_matters: String,
    uncertainty: Option<String>,
    temporal_constraints: Option<String>,
    required: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CitationEntry {
    pub unit_id: CompressionUnitId,
    pub source_ids: Vec<SourceId>,
    pub assertion_ids: Vec<AssertionId>,
    pub contradiction_ids: Vec<ContradictionId>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CitationMap {
    entries: BTreeMap<CompressionUnitId, CitationEntry>,
}

impl CitationMap {
    pub fn insert(&mut self, unit: &CompressionUnit) {
        self.entries.insert(
            unit.id.clone(),
            CitationEntry {
                unit_id: unit.id.clone(),
                source_ids: unit.source_ids.clone(),
                assertion_ids: unit.assertion_ids.clone(),
                contradiction_ids: unit.contradiction_ids.clone(),
            },
        );
    }

    pub fn entries(&self) -> impl Iterator<Item = &CitationEntry> {
        self.entries.values()
    }

    pub fn source_ids(&self) -> BTreeSet<SourceId> {
        self.entries
            .values()
            .flat_map(|entry| entry.source_ids.iter().cloned())
            .collect()
    }

    pub fn assertion_ids(&self) -> BTreeSet<AssertionId> {
        self.entries
            .values()
            .flat_map(|entry| entry.assertion_ids.iter().cloned())
            .collect()
    }

    pub fn contradiction_ids(&self) -> BTreeSet<String> {
        self.entries
            .values()
            .flat_map(|entry| entry.contradiction_ids.iter().map(ToString::to_string))
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompressedContext {
    pub text: String,
    pub units: Vec<CompressionUnit>,
    pub omitted_units: Vec<CompressionUnit>,
    pub citation_map: CitationMap,
    pub estimated_tokens: usize,
    pub warnings: Vec<String>,
}

pub struct ContextCompressor {
    plan: CompressionPlan,
}

impl ContextCompressor {
    pub fn new(plan: CompressionPlan) -> Self {
        Self { plan }
    }

    pub fn compress(&self, pack: &EvidencePack, task: &str) -> CompressedContext {
        let mut units = build_units(pack, &self.plan);
        score_units(&mut units, pack, task, &self.plan.rank_by);
        units.sort_by(|left, right| {
            right
                .required
                .cmp(&left.required)
                .then_with(|| right.score.total_cmp(&left.score))
                .then_with(|| right.level.level_number().cmp(&left.level.level_number()))
                .then_with(|| left.id.cmp(&right.id))
        });

        let budget = self.plan.budget.available_context_tokens();
        let mut selected = Vec::new();
        let mut omitted = Vec::new();
        let mut remaining = budget;
        for mut unit in units {
            if unit.estimated_tokens <= remaining {
                remaining -= unit.estimated_tokens;
                selected.push(unit);
            } else if unit.required && remaining > 0 {
                unit.text = truncate_to_tokens(&unit.text, remaining);
                unit.estimated_tokens = estimate_tokens(&unit.text);
                remaining = remaining.saturating_sub(unit.estimated_tokens);
                selected.push(unit);
            } else {
                omitted.push(unit);
            }
        }

        selected.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| right.required.cmp(&left.required))
                .then_with(|| left.level.level_number().cmp(&right.level.level_number()))
                .then_with(|| left.id.cmp(&right.id))
        });

        let mut citation_map = CitationMap::default();
        for unit in &selected {
            citation_map.insert(unit);
        }
        let text = render_units(&selected, &self.plan.policy);
        let estimated_tokens = selected.iter().map(|unit| unit.estimated_tokens).sum();
        let warnings = compression_warnings(pack, &selected, &omitted, &self.plan.policy);
        CompressedContext {
            text,
            units: selected,
            omitted_units: omitted,
            citation_map,
            estimated_tokens,
            warnings,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GoldAnswerSet {
    pub required_source_ids: BTreeSet<SourceId>,
    pub required_assertion_ids: BTreeSet<AssertionId>,
    pub required_contradiction_ids: BTreeSet<String>,
    pub baseline_answer_accuracy: f64,
}

pub struct QualityEvaluator;

impl QualityEvaluator {
    pub fn evaluate(context: &CompressedContext, gold: &GoldAnswerSet) -> CompressionQualityReport {
        let source_recall = recall(
            &gold.required_source_ids,
            &context.citation_map.source_ids(),
        );
        let assertion_recall = recall(
            &gold.required_assertion_ids,
            &context.citation_map.assertion_ids(),
        );
        let contradiction_recall = recall(
            &gold.required_contradiction_ids,
            &context.citation_map.contradiction_ids(),
        );
        let citation_recall = if gold.required_assertion_ids.is_empty() {
            source_recall
        } else {
            (source_recall + assertion_recall) / 2.0
        };
        let preservation = (citation_recall + contradiction_recall) / 2.0;
        CompressionQualityReport {
            citation_recall,
            source_recall,
            assertion_recall,
            contradiction_recall,
            estimated_answer_accuracy: round_two_f64(gold.baseline_answer_accuracy * preservation),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompressionQualityReport {
    pub citation_recall: f64,
    pub source_recall: f64,
    pub assertion_recall: f64,
    pub contradiction_recall: f64,
    pub estimated_answer_accuracy: f64,
}

impl CompressionQualityReport {
    pub fn passes_accuracy_floor(&self, floor: f64) -> bool {
        self.estimated_answer_accuracy >= floor
    }
}

fn build_units(pack: &EvidencePack, plan: &CompressionPlan) -> Vec<CompressionUnit> {
    let mut units = Vec::new();
    if plan
        .allowed_levels
        .contains(&CompressionLevel::RawSourceSnippets)
    {
        for source in &pack.sources {
            units.push(CompressionUnit::from_draft(CompressionUnitDraft {
                id: format!("source-{}", source.source_id),
                level: CompressionLevel::RawSourceSnippets,
                text: format!(
                    "raw_source source_ids={} trust={} snippet={}",
                    source.source_id,
                    source
                        .trust_score
                        .map(|score| format!("{score:.2}"))
                        .unwrap_or_else(|| "unknown".to_owned()),
                    source.snippet
                ),
                source_ids: vec![source.source_id.clone()],
                assertion_ids: Vec::new(),
                contradiction_ids: Vec::new(),
                entity_ids: Vec::new(),
                why_this_matters: "Raw evidence preserves exact source wording.".to_owned(),
                uncertainty: Some(
                    "Source excerpts may be partial and require graph assertions for scope."
                        .to_owned(),
                ),
                temporal_constraints: None,
                required: false,
            }));
        }
    }

    if plan
        .allowed_levels
        .contains(&CompressionLevel::AssertionList)
    {
        for assertion in &pack.assertions {
            if plan
                .policy
                .min_confidence
                .is_some_and(|minimum| assertion.confidence.as_f32() < minimum)
            {
                continue;
            }
            units.push(assertion_unit(assertion));
        }
    }

    if plan
        .allowed_levels
        .contains(&CompressionLevel::EntityTimeline)
    {
        units.extend(entity_timeline_units(pack));
    }
    if plan
        .allowed_levels
        .contains(&CompressionLevel::RelationshipSummary)
    {
        units.extend(relationship_summary_units(pack));
    }
    if plan
        .allowed_levels
        .contains(&CompressionLevel::CommunitySummary)
    {
        units.push(community_summary_unit(pack));
    }
    if plan
        .allowed_levels
        .contains(&CompressionLevel::TaskSpecificDistilledContext)
    {
        units.push(task_specific_unit(pack));
    }

    if plan.policy.preserve_contradictions {
        for contradiction in &pack.contradictions {
            units.push(contradiction_unit(contradiction, pack));
        }
    }
    units
}

impl CompressionUnit {
    fn from_draft(draft: CompressionUnitDraft) -> Self {
        let estimated_tokens = estimate_tokens(&draft.text);
        Self {
            id: CompressionUnitId::new(draft.id),
            level: draft.level,
            text: draft.text,
            estimated_tokens,
            source_ids: sorted_dedup(draft.source_ids),
            assertion_ids: sorted_dedup(draft.assertion_ids),
            contradiction_ids: sorted_dedup(draft.contradiction_ids),
            entity_ids: sorted_dedup(draft.entity_ids),
            score: 0.0,
            why_this_matters: draft.why_this_matters,
            uncertainty: draft.uncertainty,
            temporal_constraints: draft.temporal_constraints,
            applied_signals: Vec::new(),
            required: draft.required,
        }
    }
}

fn assertion_unit(assertion: &Assertion) -> CompressionUnit {
    CompressionUnit::from_draft(CompressionUnitDraft {
        id: format!("assertion-{}", assertion.id),
        level: CompressionLevel::AssertionList,
        text: format!(
            "assertion id={} subject={} predicate={} object={} valid={} tx={} confidence={:.2} source_ids={}",
            assertion.id,
            assertion.subject,
            assertion.predicate,
            graph_value_text(&assertion.object),
            valid_interval_text(assertion),
            tx_interval_text(assertion),
            assertion.confidence.as_f32(),
            join_ids(&assertion.source_ids)
        ),
        source_ids: assertion.source_ids.clone(),
        assertion_ids: vec![assertion.id.clone()],
        contradiction_ids: Vec::new(),
        entity_ids: assertion_entities(assertion),
        why_this_matters:
            "Assertion-level context preserves provenance, confidence, and bitemporal scope."
                .to_owned(),
        uncertainty: Some(format!(
            "Confidence {:.2}; assertion status {:?}.",
            assertion.confidence.as_f32(),
            assertion.status
        )),
        temporal_constraints: Some(format!(
            "valid_time={} transaction_time={}",
            valid_interval_text(assertion),
            tx_interval_text(assertion)
        )),
        required: !assertion.source_ids.is_empty(),
    })
}

fn entity_timeline_units(pack: &EvidencePack) -> Vec<CompressionUnit> {
    let mut by_entity: BTreeMap<EntityId, Vec<&Assertion>> = BTreeMap::new();
    for assertion in &pack.assertions {
        by_entity
            .entry(assertion.subject.clone())
            .or_default()
            .push(assertion);
        if let GraphValue::Entity(entity_id) = &assertion.object {
            by_entity
                .entry(entity_id.clone())
                .or_default()
                .push(assertion);
        }
    }
    by_entity
        .into_iter()
        .map(|(entity_id, mut assertions)| {
            assertions.sort_by(|left, right| {
                left.valid_time
                    .start
                    .cmp(&right.valid_time.start)
                    .then_with(|| left.id.cmp(&right.id))
            });
            let mut text = format!("entity_timeline entity_id={entity_id}");
            let mut source_ids = Vec::new();
            let mut assertion_ids = Vec::new();
            for assertion in assertions {
                let _ = write!(
                    text,
                    " | {} {} {} valid={} confidence={:.2} sources={}",
                    assertion.subject,
                    assertion.predicate,
                    graph_value_text(&assertion.object),
                    valid_interval_text(assertion),
                    assertion.confidence.as_f32(),
                    join_ids(&assertion.source_ids)
                );
                source_ids.extend(assertion.source_ids.clone());
                assertion_ids.push(assertion.id.clone());
            }
            CompressionUnit::from_draft(CompressionUnitDraft {
                id: format!("timeline-{entity_id}"),
                level: CompressionLevel::EntityTimeline,
                text,
                source_ids,
                assertion_ids,
                contradiction_ids: Vec::new(),
                entity_ids: vec![entity_id],
                why_this_matters: "Timeline context keeps temporal ordering visible to the model."
                    .to_owned(),
                uncertainty: Some(
                    "Timeline summaries may omit source wording but keep citations.".to_owned(),
                ),
                temporal_constraints: Some("valid intervals preserved per assertion".to_owned()),
                required: false,
            })
        })
        .collect()
}

fn relationship_summary_units(pack: &EvidencePack) -> Vec<CompressionUnit> {
    let mut by_predicate: BTreeMap<String, Vec<&Assertion>> = BTreeMap::new();
    for assertion in &pack.assertions {
        by_predicate
            .entry(assertion.predicate.to_string())
            .or_default()
            .push(assertion);
    }
    by_predicate
        .into_iter()
        .map(|(predicate, assertions)| {
            let mut text = format!(
                "relationship_summary predicate={predicate} assertion_count={}",
                assertions.len()
            );
            let mut source_ids = Vec::new();
            let mut assertion_ids = Vec::new();
            let mut entity_ids = Vec::new();
            for assertion in assertions {
                let _ = write!(
                    text,
                    " | {} -> {} confidence={:.2} valid={}",
                    assertion.subject,
                    graph_value_text(&assertion.object),
                    assertion.confidence.as_f32(),
                    valid_interval_text(assertion)
                );
                source_ids.extend(assertion.source_ids.clone());
                assertion_ids.push(assertion.id.clone());
                entity_ids.extend(assertion_entities(assertion));
            }
            CompressionUnit::from_draft(CompressionUnitDraft {
                id: format!("relationship-{predicate}"),
                level: CompressionLevel::RelationshipSummary,
                text,
                source_ids,
                assertion_ids,
                contradiction_ids: Vec::new(),
                entity_ids,
                why_this_matters:
                    "Relationship summaries reveal repeated or conflicting graph structure."
                        .to_owned(),
                uncertainty: Some(
                    "Relationship summary is derived from assertions, not a new fact.".to_owned(),
                ),
                temporal_constraints: Some("valid intervals preserved in compact form".to_owned()),
                required: false,
            })
        })
        .collect()
}

fn community_summary_unit(pack: &EvidencePack) -> CompressionUnit {
    let source_ids = pack
        .assertions
        .iter()
        .flat_map(|assertion| assertion.source_ids.iter().cloned())
        .collect::<Vec<_>>();
    let assertion_ids = pack
        .assertions
        .iter()
        .map(|assertion| assertion.id.clone())
        .collect::<Vec<_>>();
    let entity_ids = pack
        .assertions
        .iter()
        .flat_map(assertion_entities)
        .collect::<Vec<_>>();
    CompressionUnit::from_draft(CompressionUnitDraft {
        id: "community-summary".to_owned(),
        level: CompressionLevel::CommunitySummary,
        text: format!(
            "community_summary entities={} assertions={} sources={} contradictions={}",
            entity_ids.len(),
            pack.assertions.len(),
            source_ids.len(),
            pack.contradictions.len()
        ),
        source_ids,
        assertion_ids,
        contradiction_ids: pack
            .contradictions
            .iter()
            .map(|contradiction| contradiction.id.clone())
            .collect(),
        entity_ids,
        why_this_matters:
            "Community summary provides compact global orientation before detailed evidence."
                .to_owned(),
        uncertainty: Some(
            "Community summaries are derived and should be checked against citations.".to_owned(),
        ),
        temporal_constraints: Some("summary spans the evidence pack temporal windows".to_owned()),
        required: false,
    })
}

fn task_specific_unit(pack: &EvidencePack) -> CompressionUnit {
    let mut best_assertions = pack.assertions.clone();
    best_assertions.sort_by(|left, right| {
        right
            .confidence
            .as_f32()
            .total_cmp(&left.confidence.as_f32())
            .then_with(|| {
                right
                    .transaction_time
                    .start
                    .cmp(&left.transaction_time.start)
            })
            .then_with(|| left.id.cmp(&right.id))
    });
    let selected = best_assertions.iter().take(4).collect::<Vec<_>>();
    let mut text = format!("distilled_context query={}", pack.query);
    for assertion in &selected {
        let _ = write!(
            text,
            " | {} {} {} confidence={:.2} valid={} sources={}",
            assertion.subject,
            assertion.predicate,
            graph_value_text(&assertion.object),
            assertion.confidence.as_f32(),
            valid_interval_text(assertion),
            join_ids(&assertion.source_ids)
        );
    }
    if !pack.contradictions.is_empty() {
        let _ = write!(
            text,
            " | contradiction_count={} must_report_both_sides=true",
            pack.contradictions.len()
        );
    }
    CompressionUnit::from_draft(CompressionUnitDraft {
        id: "task-distilled-context".to_owned(),
        level: CompressionLevel::TaskSpecificDistilledContext,
        text,
        source_ids: selected
            .iter()
            .flat_map(|assertion| assertion.source_ids.iter().cloned())
            .collect(),
        assertion_ids: selected
            .iter()
            .map(|assertion| assertion.id.clone())
            .collect(),
        contradiction_ids: pack
            .contradictions
            .iter()
            .map(|contradiction| contradiction.id.clone())
            .collect(),
        entity_ids: selected
            .iter()
            .flat_map(|assertion| assertion_entities(assertion))
            .collect(),
        why_this_matters:
            "Task-specific context keeps the most answer-bearing facts under a tight budget."
                .to_owned(),
        uncertainty: Some("Distilled context is lossy; use citations for audit.".to_owned()),
        temporal_constraints: Some("temporal windows preserved for selected assertions".to_owned()),
        required: true,
    })
}

fn contradiction_unit(contradiction: &Contradiction, pack: &EvidencePack) -> CompressionUnit {
    let related = pack
        .assertions
        .iter()
        .filter(|assertion| {
            assertion.id == contradiction.assertion_a || assertion.id == contradiction.assertion_b
        })
        .collect::<Vec<_>>();
    let mut source_ids = Vec::new();
    let mut entity_ids = Vec::new();
    let mut text = format!(
        "contradiction id={} type={:?} severity={:?} assertion_a={} assertion_b={} explanation={}",
        contradiction.id,
        contradiction.contradiction_type,
        contradiction.severity,
        contradiction.assertion_a,
        contradiction.assertion_b,
        contradiction.explanation
    );
    for assertion in related {
        let _ = write!(
            text,
            " | side assertion={} object={} confidence={:.2} valid={} sources={}",
            assertion.id,
            graph_value_text(&assertion.object),
            assertion.confidence.as_f32(),
            valid_interval_text(assertion),
            join_ids(&assertion.source_ids)
        );
        source_ids.extend(assertion.source_ids.clone());
        entity_ids.extend(assertion_entities(assertion));
    }
    CompressionUnit::from_draft(CompressionUnitDraft {
        id: format!("contradiction-{}", contradiction.id),
        level: CompressionLevel::TaskSpecificDistilledContext,
        text,
        source_ids,
        assertion_ids: vec![
            contradiction.assertion_a.clone(),
            contradiction.assertion_b.clone(),
        ],
        contradiction_ids: vec![contradiction.id.clone()],
        entity_ids,
        why_this_matters:
            "Contradiction must be surfaced so the model does not collapse competing claims."
                .to_owned(),
        uncertainty: Some(
            "Competing assertions may both be evidence-backed; do not present one side as settled."
                .to_owned(),
        ),
        temporal_constraints: Some("overlapping valid-time conflict preserved".to_owned()),
        required: true,
    })
}

fn score_units(
    units: &mut [CompressionUnit],
    pack: &EvidencePack,
    task: &str,
    signals: &[CompressionSignal],
) {
    let source_trust = pack
        .sources
        .iter()
        .map(|source| (source.source_id.clone(), source.trust_score.unwrap_or(0.5)))
        .collect::<BTreeMap<_, _>>();
    let contradiction_ids = pack
        .contradictions
        .iter()
        .map(|contradiction| contradiction.id.clone())
        .collect::<BTreeSet<_>>();
    for unit in units {
        unit.score = 0.0;
        unit.applied_signals.clear();
        for signal in signals {
            unit.applied_signals.push(*signal);
            unit.score += match signal {
                CompressionSignal::Recency => recency_score(unit, pack),
                CompressionSignal::Confidence => confidence_score(unit, pack),
                CompressionSignal::SourceTrust => source_trust_score(unit, &source_trust),
                CompressionSignal::GraphCentrality => graph_centrality_score(unit),
                CompressionSignal::TaskRelevance => task_relevance_score(unit, task),
                CompressionSignal::ContradictionImportance => {
                    contradiction_importance_score(unit, &contradiction_ids, pack)
                }
            };
        }
        if unit.required {
            unit.score += 3.0;
        }
        unit.score += f64::from(unit.level.level_number()) * 0.05;
    }
}

fn recency_score(unit: &CompressionUnit, pack: &EvidencePack) -> f64 {
    let latest = pack
        .assertions
        .iter()
        .map(|assertion| assertion.transaction_time.start.as_i64())
        .max()
        .unwrap_or(0);
    let unit_latest = unit
        .assertion_ids
        .iter()
        .filter_map(|id| pack.assertions.iter().find(|assertion| &assertion.id == id))
        .map(|assertion| assertion.transaction_time.start.as_i64())
        .max()
        .unwrap_or(latest);
    if latest <= 0 {
        0.0
    } else {
        (unit_latest as f64 / latest as f64).clamp(0.0, 1.0)
    }
}

fn confidence_score(unit: &CompressionUnit, pack: &EvidencePack) -> f64 {
    let confidences = unit
        .assertion_ids
        .iter()
        .filter_map(|id| pack.assertions.iter().find(|assertion| &assertion.id == id))
        .map(|assertion| assertion.confidence.as_f32() as f64)
        .collect::<Vec<_>>();
    if confidences.is_empty() {
        0.5
    } else {
        confidences.iter().sum::<f64>() / confidences.len() as f64
    }
}

fn source_trust_score(unit: &CompressionUnit, source_trust: &BTreeMap<SourceId, f32>) -> f64 {
    if unit.source_ids.is_empty() {
        return 0.0;
    }
    unit.source_ids
        .iter()
        .map(|id| f64::from(source_trust.get(id).copied().unwrap_or(0.5)))
        .sum::<f64>()
        / unit.source_ids.len() as f64
}

fn graph_centrality_score(unit: &CompressionUnit) -> f64 {
    let links = unit.source_ids.len()
        + unit.assertion_ids.len()
        + unit.contradiction_ids.len()
        + unit.entity_ids.len();
    (links as f64 / 8.0).min(1.0)
}

fn task_relevance_score(unit: &CompressionUnit, task: &str) -> f64 {
    let task_tokens = tokens(task).collect::<BTreeSet<_>>();
    if task_tokens.is_empty() {
        return 0.0;
    }
    let unit_tokens = tokens(&unit.text).collect::<BTreeSet<_>>();
    task_tokens.intersection(&unit_tokens).count() as f64 / task_tokens.len() as f64
}

fn contradiction_importance_score(
    unit: &CompressionUnit,
    contradiction_ids: &BTreeSet<ContradictionId>,
    pack: &EvidencePack,
) -> f64 {
    if unit
        .contradiction_ids
        .iter()
        .any(|id| contradiction_ids.contains(id))
    {
        return unit
            .contradiction_ids
            .iter()
            .filter_map(|id| {
                pack.contradictions
                    .iter()
                    .find(|contradiction| &contradiction.id == id)
            })
            .map(|contradiction| match contradiction.severity {
                Severity::Critical => 1.0,
                Severity::High => 0.9,
                Severity::Medium => 0.65,
                Severity::Low => 0.35,
            })
            .fold(0.0, f64::max);
    }
    0.0
}

fn render_units(units: &[CompressionUnit], policy: &EvidencePreservationPolicy) -> String {
    let mut output = String::new();
    for unit in units {
        let _ = writeln!(
            output,
            "[{}:{}] {}",
            unit.level.level_number(),
            unit.id,
            unit.text
        );
        if policy.include_why_this_matters {
            let _ = writeln!(output, "why_this_matters={}", unit.why_this_matters);
        }
        if policy.preserve_uncertainty {
            if let Some(uncertainty) = &unit.uncertainty {
                let _ = writeln!(output, "uncertainty={uncertainty}");
            }
        }
        if policy.preserve_temporal_constraints {
            if let Some(temporal) = &unit.temporal_constraints {
                let _ = writeln!(output, "temporal={temporal}");
            }
        }
        if policy.preserve_citations {
            let _ = writeln!(
                output,
                "citations sources={} assertions={} contradictions={}",
                join_ids(&unit.source_ids),
                join_ids(&unit.assertion_ids),
                join_ids(&unit.contradiction_ids)
            );
        }
    }
    output
}

fn compression_warnings(
    pack: &EvidencePack,
    selected: &[CompressionUnit],
    omitted: &[CompressionUnit],
    policy: &EvidencePreservationPolicy,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if policy.preserve_contradictions {
        let selected_contradictions = selected
            .iter()
            .flat_map(|unit| unit.contradiction_ids.iter())
            .collect::<BTreeSet<_>>();
        for contradiction in &pack.contradictions {
            if !selected_contradictions.contains(&contradiction.id) {
                warnings.push(format!(
                    "contradiction retained in source pack but omitted from compressed context: {}",
                    contradiction.id
                ));
            }
        }
    }
    if !omitted.is_empty() {
        warnings.push(format!("omitted_units={}", omitted.len()));
    }
    warnings
}

fn estimate_tokens(text: &str) -> usize {
    text.split_whitespace().count().max(1)
}

fn truncate_to_tokens(text: &str, tokens: usize) -> String {
    text.split_whitespace()
        .take(tokens)
        .collect::<Vec<_>>()
        .join(" ")
}

fn assertion_entities(assertion: &Assertion) -> Vec<EntityId> {
    let mut ids = vec![assertion.subject.clone()];
    if let GraphValue::Entity(id) = &assertion.object {
        ids.push(id.clone());
    }
    sorted_dedup(ids)
}

fn graph_value_text(value: &GraphValue) -> String {
    match value {
        GraphValue::Entity(id) => id.to_string(),
        GraphValue::Text(value) => value.clone(),
        GraphValue::Integer(value) => value.to_string(),
        GraphValue::Decimal(value) => value.to_string(),
        GraphValue::Boolean(value) => value.to_string(),
        GraphValue::Time(value) => value.as_i64().to_string(),
        GraphValue::Null => "null".to_owned(),
    }
}

fn valid_interval_text(assertion: &Assertion) -> String {
    format!(
        "{}..{}",
        assertion.valid_time.start.as_i64(),
        assertion
            .valid_time
            .end
            .map(|time| time.as_i64().to_string())
            .unwrap_or_else(|| "open".to_owned())
    )
}

fn tx_interval_text(assertion: &Assertion) -> String {
    format!(
        "{}..{}",
        assertion.transaction_time.start.as_i64(),
        assertion
            .transaction_time
            .end
            .map(|time| time.as_i64().to_string())
            .unwrap_or_else(|| "open".to_owned())
    )
}

fn join_ids<T: ToString>(ids: &[T]) -> String {
    ids.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn sorted_dedup<T: Ord>(mut values: Vec<T>) -> Vec<T> {
    values.sort();
    values.dedup();
    values
}

fn tokens(value: &str) -> impl Iterator<Item = String> + '_ {
    value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| token.len() > 2)
        .map(|token| token.to_ascii_lowercase())
}

fn recall<T: Ord>(required: &BTreeSet<T>, actual: &BTreeSet<T>) -> f64 {
    if required.is_empty() {
        return 1.0;
    }
    required.intersection(actual).count() as f64 / required.len() as f64
}

fn default_signals() -> Vec<CompressionSignal> {
    vec![
        CompressionSignal::ContradictionImportance,
        CompressionSignal::TaskRelevance,
        CompressionSignal::Confidence,
        CompressionSignal::SourceTrust,
        CompressionSignal::GraphCentrality,
        CompressionSignal::Recency,
    ]
}

fn round_two_f64(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}
