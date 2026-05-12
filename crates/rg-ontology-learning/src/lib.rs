//! Review-gated ontology discovery for Reality Graph.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rg_core::{
    Assertion, AssertionId, Entity, EntityId, EntityType, GraphOntology, GraphValue, PredicateId,
    PropertyType,
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CandidateStatus {
    PendingReview,
    Approved,
    Rejected,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum OntologyChangeKind {
    NewPredicate,
    NewEntityType,
    SchemaInduction,
    RelationshipCardinality,
    ContradictionRule,
    TemporalPattern,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OntologyDiscoveryInput {
    pub ontology: GraphOntology,
    pub domain_hint: Option<String>,
    pub entities: Vec<Entity>,
    pub assertions: Vec<Assertion>,
}

impl OntologyDiscoveryInput {
    fn entity_by_id(&self) -> BTreeMap<EntityId, Entity> {
        self.entities
            .iter()
            .map(|entity| (entity.id.clone(), entity.clone()))
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PredicateCandidate {
    pub id: String,
    pub predicate: PredicateId,
    pub subject_type: Option<String>,
    pub object_type: Option<String>,
    pub support_count: usize,
    pub temporal: bool,
    pub evidence_assertion_ids: Vec<AssertionId>,
    pub suggested_pack: Option<String>,
    pub status: CandidateStatus,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PredicateCandidateMiner {
    pub min_support: usize,
}

impl Default for PredicateCandidateMiner {
    fn default() -> Self {
        Self { min_support: 1 }
    }
}

impl PredicateCandidateMiner {
    pub fn mine(&self, input: &OntologyDiscoveryInput) -> Vec<PredicateCandidate> {
        let entities = input.entity_by_id();
        let mut by_predicate = BTreeMap::<PredicateId, Vec<&Assertion>>::new();
        for assertion in &input.assertions {
            if input.ontology.predicate(&assertion.predicate).is_none() {
                by_predicate
                    .entry(assertion.predicate.clone())
                    .or_default()
                    .push(assertion);
            }
        }

        let mut candidates = by_predicate
            .into_iter()
            .filter_map(|(predicate, assertions)| {
                if assertions.len() < self.min_support {
                    return None;
                }
                let subject_types = assertions
                    .iter()
                    .filter_map(|assertion| entities.get(&assertion.subject))
                    .map(|entity| entity_type_name(&entity.entity_type))
                    .collect::<BTreeSet<_>>();
                let object_types = assertions
                    .iter()
                    .map(|assertion| graph_value_type(&assertion.object, &entities))
                    .collect::<BTreeSet<_>>();
                let temporal = assertions.iter().any(|assertion| {
                    assertion.valid_time.end.is_some() || assertion.valid_time.start.as_i64() != 0
                });
                let evidence_assertion_ids = assertions
                    .iter()
                    .map(|assertion| assertion.id.clone())
                    .collect::<Vec<_>>();
                Some(PredicateCandidate {
                    id: format!("predicate-candidate-{predicate}").to_ascii_lowercase(),
                    predicate,
                    subject_type: single_value(subject_types),
                    object_type: single_value(object_types),
                    support_count: assertions.len(),
                    temporal,
                    evidence_assertion_ids,
                    suggested_pack: input.domain_hint.clone(),
                    status: CandidateStatus::PendingReview,
                })
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| left.predicate.cmp(&right.predicate));
        candidates
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntityTypeCluster {
    pub id: String,
    pub suggested_type: String,
    pub entity_count: usize,
    pub entity_ids: Vec<EntityId>,
    pub common_properties: BTreeMap<String, PropertyType>,
    pub suggested_pack: Option<String>,
    pub status: CandidateStatus,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EntityTypeClusterer {
    pub min_cluster_size: usize,
}

impl Default for EntityTypeClusterer {
    fn default() -> Self {
        Self {
            min_cluster_size: 2,
        }
    }
}

impl EntityTypeClusterer {
    pub fn cluster(&self, input: &OntologyDiscoveryInput) -> Vec<EntityTypeCluster> {
        let mut by_type = BTreeMap::<String, Vec<&Entity>>::new();
        for entity in &input.entities {
            let name = entity_type_name(&entity.entity_type);
            if input.ontology.entity_type(&name).is_none() {
                by_type.entry(name).or_default().push(entity);
            }
        }

        let mut clusters = by_type
            .into_iter()
            .filter_map(|(suggested_type, entities)| {
                if entities.len() < self.min_cluster_size {
                    return None;
                }
                let common_properties = common_properties(&entities);
                let entity_ids = entities
                    .iter()
                    .map(|entity| entity.id.clone())
                    .collect::<Vec<_>>();
                Some(EntityTypeCluster {
                    id: format!("entity-type-candidate-{suggested_type}").to_ascii_lowercase(),
                    suggested_type,
                    entity_count: entities.len(),
                    entity_ids,
                    common_properties,
                    suggested_pack: input.domain_hint.clone(),
                    status: CandidateStatus::PendingReview,
                })
            })
            .collect::<Vec<_>>();
        clusters.sort_by(|left, right| left.suggested_type.cmp(&right.suggested_type));
        clusters
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConstraintCandidate {
    pub id: String,
    pub kind: OntologyChangeKind,
    pub predicate: PredicateId,
    pub max_active_objects_per_subject: Option<usize>,
    pub confidence: f32,
    pub description: String,
    pub evidence_assertion_ids: Vec<AssertionId>,
    pub status: CandidateStatus,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConstraintLearner {
    pub min_support: usize,
}

impl Default for ConstraintLearner {
    fn default() -> Self {
        Self { min_support: 2 }
    }
}

impl ConstraintLearner {
    pub fn learn(&self, input: &OntologyDiscoveryInput) -> Vec<ConstraintCandidate> {
        let mut by_predicate = BTreeMap::<PredicateId, Vec<&Assertion>>::new();
        for assertion in &input.assertions {
            if input.ontology.predicate(&assertion.predicate).is_none() {
                by_predicate
                    .entry(assertion.predicate.clone())
                    .or_default()
                    .push(assertion);
            }
        }

        let mut candidates = Vec::new();
        for (predicate, assertions) in by_predicate {
            if assertions.len() < self.min_support {
                continue;
            }
            if let Some(candidate) = cardinality_candidate(&predicate, &assertions) {
                candidates.push(candidate);
            }
            if let Some(candidate) = contradiction_candidate(&predicate, &assertions) {
                candidates.push(candidate);
            }
        }
        candidates.sort_by(|left, right| {
            left.predicate
                .cmp(&right.predicate)
                .then_with(|| left.kind.cmp(&right.kind))
        });
        candidates
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TemporalPattern {
    pub id: String,
    pub predicate: PredicateId,
    pub temporal_assertion_count: usize,
    pub bounded_interval_count: usize,
    pub open_ended_ratio: f32,
    pub average_duration: Option<f32>,
    pub description: String,
    pub status: CandidateStatus,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TemporalPatternMiner {
    pub min_support: usize,
}

impl Default for TemporalPatternMiner {
    fn default() -> Self {
        Self { min_support: 1 }
    }
}

impl TemporalPatternMiner {
    pub fn mine(&self, input: &OntologyDiscoveryInput) -> Vec<TemporalPattern> {
        let mut by_predicate = BTreeMap::<PredicateId, Vec<&Assertion>>::new();
        for assertion in &input.assertions {
            if input.ontology.predicate(&assertion.predicate).is_none() {
                by_predicate
                    .entry(assertion.predicate.clone())
                    .or_default()
                    .push(assertion);
            }
        }

        let mut patterns = by_predicate
            .into_iter()
            .filter_map(|(predicate, assertions)| {
                if assertions.len() < self.min_support {
                    return None;
                }
                let temporal_assertion_count = assertions.len();
                let bounded = assertions
                    .iter()
                    .filter(|assertion| assertion.valid_time.end.is_some())
                    .count();
                let open_ended = temporal_assertion_count - bounded;
                let durations = assertions
                    .iter()
                    .filter_map(|assertion| {
                        assertion
                            .valid_time
                            .end
                            .map(|end| end.as_i64() - assertion.valid_time.start.as_i64())
                    })
                    .collect::<Vec<_>>();
                let average_duration = (!durations.is_empty())
                    .then(|| durations.iter().sum::<i64>() as f32 / durations.len() as f32);
                let open_ended_ratio = open_ended as f32 / temporal_assertion_count as f32;
                let description = if bounded > 0 && open_ended == 0 {
                    "predicate appears temporal with bounded valid intervals".to_owned()
                } else if open_ended > 0 && bounded > 0 {
                    "predicate mixes bounded and open-ended temporal assertions".to_owned()
                } else {
                    "predicate appears temporal with open-ended valid intervals".to_owned()
                };
                Some(TemporalPattern {
                    id: format!("temporal-pattern-{predicate}").to_ascii_lowercase(),
                    predicate,
                    temporal_assertion_count,
                    bounded_interval_count: bounded,
                    open_ended_ratio,
                    average_duration,
                    description,
                    status: CandidateStatus::PendingReview,
                })
            })
            .collect::<Vec<_>>();
        patterns.sort_by(|left, right| left.predicate.cmp(&right.predicate));
        patterns
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainOntologyPack {
    pub name: String,
    pub domain_hint: Option<String>,
    pub candidate_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OntologyDriftReport {
    pub id: String,
    pub domain_pack: DomainOntologyPack,
    pub new_predicates: Vec<PredicateCandidate>,
    pub new_entity_types: Vec<EntityTypeCluster>,
    pub constraints: Vec<ConstraintCandidate>,
    pub temporal_patterns: Vec<TemporalPattern>,
    pub drift_score: f32,
    pub requires_human_review: bool,
    pub auto_promoted: bool,
    pub summary: String,
}

impl OntologyDriftReport {
    pub fn generate(pack_name: impl Into<String>, input: &OntologyDiscoveryInput) -> Self {
        let pack_name = pack_name.into();
        let new_predicates = PredicateCandidateMiner::default().mine(input);
        let new_entity_types = EntityTypeClusterer::default().cluster(input);
        let constraints = ConstraintLearner::default().learn(input);
        let temporal_patterns = TemporalPatternMiner::default().mine(input);
        let candidate_count = new_predicates.len()
            + new_entity_types.len()
            + constraints.len()
            + temporal_patterns.len();
        let observed_items = input.entities.len() + input.assertions.len();
        let drift_score = if observed_items == 0 {
            0.0
        } else {
            (candidate_count as f32 / observed_items as f32).min(1.0)
        };
        let domain_pack = DomainOntologyPack {
            name: pack_name.clone(),
            domain_hint: input.domain_hint.clone(),
            candidate_count,
        };
        Self {
            id: format!("ontology-drift-{pack_name}").to_ascii_lowercase(),
            domain_pack,
            new_predicates,
            new_entity_types,
            constraints,
            temporal_patterns,
            drift_score,
            requires_human_review: candidate_count > 0,
            auto_promoted: false,
            summary: format!(
                "{candidate_count} ontology candidates found; ontology changes require human review before promotion"
            ),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HumanReviewItem {
    pub id: String,
    pub kind: OntologyChangeKind,
    pub title: String,
    pub status: CandidateStatus,
    pub audit_trail: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HumanReviewDecision {
    Approve { reviewer: String, rationale: String },
    Reject { reviewer: String, rationale: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HumanReviewError {
    UnknownReviewItem(String),
}

impl fmt::Display for HumanReviewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownReviewItem(id) => write!(formatter, "unknown review item {id}"),
        }
    }
}

impl std::error::Error for HumanReviewError {}

#[derive(Clone, Debug, PartialEq)]
pub struct HumanReviewWorkflow {
    report: OntologyDriftReport,
    items: Vec<HumanReviewItem>,
}

impl HumanReviewWorkflow {
    pub fn from_report(report: OntologyDriftReport) -> Self {
        let mut items = Vec::new();
        items.extend(
            report
                .new_predicates
                .iter()
                .map(|candidate| HumanReviewItem {
                    id: candidate.id.clone(),
                    kind: OntologyChangeKind::NewPredicate,
                    title: format!("Add predicate {}", candidate.predicate),
                    status: CandidateStatus::PendingReview,
                    audit_trail: Vec::new(),
                }),
        );
        items.extend(
            report
                .new_entity_types
                .iter()
                .map(|cluster| HumanReviewItem {
                    id: cluster.id.clone(),
                    kind: OntologyChangeKind::NewEntityType,
                    title: format!("Add entity type {}", cluster.suggested_type),
                    status: CandidateStatus::PendingReview,
                    audit_trail: Vec::new(),
                }),
        );
        items.extend(report.constraints.iter().map(|constraint| HumanReviewItem {
            id: constraint.id.clone(),
            kind: constraint.kind,
            title: constraint.description.clone(),
            status: CandidateStatus::PendingReview,
            audit_trail: Vec::new(),
        }));
        items.extend(
            report
                .temporal_patterns
                .iter()
                .map(|pattern| HumanReviewItem {
                    id: pattern.id.clone(),
                    kind: OntologyChangeKind::TemporalPattern,
                    title: pattern.description.clone(),
                    status: CandidateStatus::PendingReview,
                    audit_trail: Vec::new(),
                }),
        );
        items.sort_by(|left, right| left.id.cmp(&right.id));
        Self { report, items }
    }

    pub fn report(&self) -> &OntologyDriftReport {
        &self.report
    }

    pub fn items(&self) -> &[HumanReviewItem] {
        &self.items
    }

    pub fn pending_items(&self) -> Vec<&HumanReviewItem> {
        self.items
            .iter()
            .filter(|item| item.status == CandidateStatus::PendingReview)
            .collect()
    }

    pub fn approved_changes(&self) -> Vec<&HumanReviewItem> {
        self.items
            .iter()
            .filter(|item| item.status == CandidateStatus::Approved)
            .collect()
    }

    pub fn can_auto_promote(&self) -> bool {
        false
    }

    pub fn record_decision(
        &mut self,
        item_id: &str,
        decision: HumanReviewDecision,
    ) -> Result<(), HumanReviewError> {
        let item = self
            .items
            .iter_mut()
            .find(|item| item.id == item_id)
            .ok_or_else(|| HumanReviewError::UnknownReviewItem(item_id.to_owned()))?;
        match decision {
            HumanReviewDecision::Approve {
                reviewer,
                rationale,
            } => {
                item.status = CandidateStatus::Approved;
                item.audit_trail
                    .push(format!("{reviewer} approved: {rationale}"));
            }
            HumanReviewDecision::Reject {
                reviewer,
                rationale,
            } => {
                item.status = CandidateStatus::Rejected;
                item.audit_trail
                    .push(format!("{reviewer} rejected: {rationale}"));
            }
        }
        Ok(())
    }
}

fn cardinality_candidate(
    predicate: &PredicateId,
    assertions: &[&Assertion],
) -> Option<ConstraintCandidate> {
    let entity_object_assertions = assertions
        .iter()
        .copied()
        .filter(|assertion| matches!(assertion.object, GraphValue::Entity(_)))
        .collect::<Vec<_>>();
    if entity_object_assertions.len() < 2 {
        return None;
    }

    let mut objects_by_subject = BTreeMap::<EntityId, BTreeSet<String>>::new();
    let mut has_repeated_subject = false;
    for assertion in &entity_object_assertions {
        let GraphValue::Entity(object_id) = &assertion.object else {
            continue;
        };
        let entry = objects_by_subject
            .entry(assertion.subject.clone())
            .or_default();
        has_repeated_subject |= !entry.is_empty() && !entry.contains(object_id.as_str());
        entry.insert(object_id.as_str().to_owned());
    }
    if !has_repeated_subject {
        return None;
    }

    let max_concurrent = max_concurrent_objects(&entity_object_assertions);
    if max_concurrent > 1 {
        return None;
    }

    let evidence_assertion_ids = entity_object_assertions
        .iter()
        .map(|assertion| assertion.id.clone())
        .collect::<Vec<_>>();
    Some(ConstraintCandidate {
        id: format!("constraint-cardinality-{predicate}").to_ascii_lowercase(),
        kind: OntologyChangeKind::RelationshipCardinality,
        predicate: predicate.clone(),
        max_active_objects_per_subject: Some(1),
        confidence: 0.85,
        description: format!(
            "{predicate} appears to have max_active_objects_per_subject = 1 based on non-overlapping valid intervals"
        ),
        evidence_assertion_ids,
        status: CandidateStatus::PendingReview,
    })
}

fn contradiction_candidate(
    predicate: &PredicateId,
    assertions: &[&Assertion],
) -> Option<ConstraintCandidate> {
    for left_index in 0..assertions.len() {
        let left = assertions[left_index];
        for right in assertions.iter().copied().skip(left_index + 1) {
            if left.subject == right.subject
                && left.object != right.object
                && scalar_value(&left.object)
                && scalar_value(&right.object)
                && left.valid_time.overlaps(&right.valid_time)
            {
                let mut evidence_assertion_ids = vec![left.id.clone(), right.id.clone()];
                evidence_assertion_ids.sort();
                return Some(ConstraintCandidate {
                    id: format!("constraint-contradiction-{predicate}").to_ascii_lowercase(),
                    kind: OntologyChangeKind::ContradictionRule,
                    predicate: predicate.clone(),
                    max_active_objects_per_subject: None,
                    confidence: 0.9,
                    description: format!(
                        "{predicate} has overlapping scalar values and should be reviewed as a contradiction rule"
                    ),
                    evidence_assertion_ids,
                    status: CandidateStatus::PendingReview,
                });
            }
        }
    }
    None
}

fn max_concurrent_objects(assertions: &[&Assertion]) -> usize {
    let mut max_count = 0;
    for assertion in assertions {
        let count = assertions
            .iter()
            .filter(|other| {
                assertion.subject == other.subject
                    && assertion.valid_time.overlaps(&other.valid_time)
                    && assertion.object != other.object
            })
            .count()
            + 1;
        max_count = max_count.max(count);
    }
    max_count
}

fn common_properties(entities: &[&Entity]) -> BTreeMap<String, PropertyType> {
    let Some(first) = entities.first() else {
        return BTreeMap::new();
    };
    let mut common = first
        .properties
        .0
        .iter()
        .map(|(key, value)| (key.as_str().to_owned(), property_type(value)))
        .collect::<BTreeMap<_, _>>();

    for entity in entities.iter().skip(1) {
        common.retain(|property, expected_type| {
            entity
                .properties
                .0
                .iter()
                .find(|(key, _)| key.as_str() == property)
                .is_some_and(|(_, value)| property_type(value) == *expected_type)
        });
    }
    common
}

fn entity_type_name(entity_type: &EntityType) -> String {
    match entity_type {
        EntityType::Person => "Person".to_owned(),
        EntityType::Organization => "Organization".to_owned(),
        EntityType::Place => "Place".to_owned(),
        EntityType::Event => "Event".to_owned(),
        EntityType::Document => "Document".to_owned(),
        EntityType::Concept => "Concept".to_owned(),
        EntityType::Custom(name) => name.clone(),
    }
}

fn graph_value_type(value: &GraphValue, entities: &BTreeMap<EntityId, Entity>) -> String {
    match value {
        GraphValue::Entity(entity_id) => entities
            .get(entity_id)
            .map(|entity| entity_type_name(&entity.entity_type))
            .unwrap_or_else(|| "Entity".to_owned()),
        GraphValue::Text(_) => "String".to_owned(),
        GraphValue::Integer(_) => "Integer".to_owned(),
        GraphValue::Decimal(_) => "Float".to_owned(),
        GraphValue::Boolean(_) => "Boolean".to_owned(),
        GraphValue::Time(_) => "Date".to_owned(),
        GraphValue::Null => "Null".to_owned(),
    }
}

fn property_type(value: &GraphValue) -> PropertyType {
    match value {
        GraphValue::Text(_) => PropertyType::String,
        GraphValue::Integer(_) => PropertyType::Integer,
        GraphValue::Decimal(_) => PropertyType::Float,
        GraphValue::Boolean(_) => PropertyType::Boolean,
        GraphValue::Time(_) => PropertyType::Date,
        GraphValue::Entity(_) | GraphValue::Null => PropertyType::String,
    }
}

fn scalar_value(value: &GraphValue) -> bool {
    !matches!(value, GraphValue::Entity(_))
}

fn single_value(values: BTreeSet<String>) -> Option<String> {
    (values.len() == 1)
        .then(|| values.into_iter().next())
        .flatten()
}
