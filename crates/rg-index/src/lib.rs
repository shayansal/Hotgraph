//! Indexing primitives for graph traversal and temporal queries.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::error::Error;
use std::fmt;

use rg_core::{
    Assertion, AssertionId, AssertionStatus, Confidence, ContextScope, ContradictionId, EntityId,
    GraphValue, PredicateId, TxTime, ValidTime,
};

pub type AdjacencyIndex = TemporalIndex;

#[derive(Default)]
pub struct TemporalIndex {
    assertions: HashMap<AssertionId, Assertion>,
    by_subject: HashMap<EntityId, Vec<AssertionId>>,
    by_predicate: HashMap<PredicateId, Vec<AssertionId>>,
    by_object: HashMap<ObjectIndexKey, Vec<AssertionId>>,
    by_entity: HashMap<EntityId, Vec<AssertionId>>,
    outgoing_values: HashMap<EntityId, Vec<Assertion>>,
    by_context: HashMap<ContextScope, Vec<AssertionId>>,
    by_confidence: BTreeMap<ConfidenceBucket, Vec<AssertionId>>,
    by_valid_start: BTreeMap<i64, Vec<AssertionId>>,
    by_tx_start: BTreeMap<i64, Vec<AssertionId>>,
    ontology: ContradictionOntology,
}

impl TemporalIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_ontology(ontology: ContradictionOntology) -> Self {
        Self {
            ontology,
            ..Self::default()
        }
    }

    pub fn insert_assertion(&mut self, assertion: Assertion) {
        let assertion_id = assertion.id.clone();
        push_hash_index(
            &mut self.by_subject,
            assertion.subject.clone(),
            assertion_id.clone(),
        );
        self.outgoing_values
            .entry(assertion.subject.clone())
            .or_default()
            .push(assertion.clone());
        if let Some(assertions) = self.outgoing_values.get_mut(&assertion.subject) {
            assertions.sort_by(|left, right| left.id.cmp(&right.id));
        }
        push_hash_index(
            &mut self.by_predicate,
            assertion.predicate.clone(),
            assertion_id.clone(),
        );
        push_hash_index(
            &mut self.by_object,
            ObjectIndexKey::from(&assertion.object),
            assertion_id.clone(),
        );
        push_hash_index(
            &mut self.by_entity,
            assertion.subject.clone(),
            assertion_id.clone(),
        );
        if let GraphValue::Entity(entity_id) = &assertion.object {
            push_hash_index(&mut self.by_entity, entity_id.clone(), assertion_id.clone());
        }
        push_hash_index(
            &mut self.by_context,
            assertion.context.clone(),
            assertion_id.clone(),
        );
        push_tree_index(
            &mut self.by_confidence,
            ConfidenceBucket::from(assertion.confidence),
            assertion_id.clone(),
        );
        push_tree_index(
            &mut self.by_valid_start,
            assertion.valid_time.start.as_i64(),
            assertion_id.clone(),
        );
        push_tree_index(
            &mut self.by_tx_start,
            assertion.transaction_time.start.as_i64(),
            assertion_id.clone(),
        );
        self.assertions.insert(assertion_id, assertion);
    }

    pub fn outgoing(&self, entity: &EntityId) -> &[Assertion] {
        self.outgoing_values.get(entity).map_or(&[], Vec::as_slice)
    }

    pub fn assertions_by_subject(&self, entity: &EntityId) -> Vec<&Assertion> {
        self.assertions_for_ids(self.by_subject.get(entity))
    }

    pub fn assertions_by_predicate(&self, predicate: &PredicateId) -> Vec<&Assertion> {
        self.assertions_for_ids(self.by_predicate.get(predicate))
    }

    pub fn assertions_by_object(&self, object: &GraphValue) -> Vec<&Assertion> {
        let key = ObjectIndexKey::from(object);
        self.assertions_for_ids(self.by_object.get(&key))
    }

    pub fn assertions_by_context(&self, context: &ContextScope) -> Vec<&Assertion> {
        self.assertions_for_ids(self.by_context.get(context))
    }

    pub fn assertions_by_confidence_bucket(&self, bucket: ConfidenceBucket) -> Vec<&Assertion> {
        self.assertions_for_ids(self.by_confidence.get(&bucket))
    }

    pub fn valid_at(&self, instant: ValidTime) -> Vec<&Assertion> {
        self.by_valid_start
            .range(..=instant.as_i64())
            .flat_map(|(_, ids)| ids.iter())
            .filter_map(|id| self.assertions.get(id))
            .filter(|assertion| assertion.status == AssertionStatus::Active)
            .filter(|assertion| assertion.valid_time.contains(instant))
            .collect()
    }

    pub fn known_at(&self, instant: TxTime) -> Vec<&Assertion> {
        self.by_tx_start
            .range(..=instant.as_i64())
            .flat_map(|(_, ids)| ids.iter())
            .filter_map(|id| self.assertions.get(id))
            .filter(|assertion| assertion.status == AssertionStatus::Active)
            .filter(|assertion| assertion.transaction_time.contains(instant))
            .collect()
    }

    pub fn adjacent_at(&self, entity: &EntityId, instant: ValidTime) -> Vec<&Assertion> {
        self.assertions_for_ids(self.by_entity.get(entity))
            .into_iter()
            .filter(|assertion| assertion.status == AssertionStatus::Active)
            .filter(|assertion| assertion.valid_time.contains(instant))
            .collect()
    }

    pub fn contradictions(&self) -> Vec<Contradiction> {
        let assertions = sorted_assertions(self.assertions.values());
        let mut contradictions = Vec::new();
        for left_index in 0..assertions.len() {
            for right in assertions.iter().skip(left_index + 1) {
                let left = assertions[left_index];
                if let Some(contradiction) = classify_contradiction(left, right, &self.ontology) {
                    contradictions.push(contradiction);
                }
            }
        }
        contradictions.sort_by(|left, right| left.id.cmp(&right.id));
        contradictions.dedup_by(|left, right| left.id == right.id);
        contradictions
    }

    fn assertions_for_ids(&self, ids: Option<&Vec<AssertionId>>) -> Vec<&Assertion> {
        ids.into_iter()
            .flatten()
            .filter_map(|id| self.assertions.get(id))
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Contradiction {
    pub id: ContradictionId,
    pub assertion_a: AssertionId,
    pub assertion_b: AssertionId,
    pub contradiction_type: ContradictionType,
    pub severity: Severity,
    pub explanation: String,
}

impl Contradiction {
    fn new(
        left: &Assertion,
        right: &Assertion,
        contradiction_type: ContradictionType,
        severity: Severity,
        explanation: impl Into<String>,
    ) -> Self {
        let (assertion_a, assertion_b) = if left.id <= right.id {
            (left.id.clone(), right.id.clone())
        } else {
            (right.id.clone(), left.id.clone())
        };
        let id = contradiction_id(&assertion_a, &assertion_b, contradiction_type);

        Self {
            id,
            assertion_a,
            assertion_b,
            contradiction_type,
            severity,
            explanation: explanation.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ContradictionType {
    ExactPredicateConflict,
    MutuallyExclusivePredicates,
    IncompatibleScalarValues,
    LowerConfidenceReplacement,
}

impl ContradictionType {
    fn slug(self) -> &'static str {
        match self {
            Self::ExactPredicateConflict => "exact-predicate-conflict",
            Self::MutuallyExclusivePredicates => "mutually-exclusive-predicates",
            Self::IncompatibleScalarValues => "incompatible-scalar-values",
            Self::LowerConfidenceReplacement => "lower-confidence-replacement",
        }
    }
}

impl fmt::Display for ContradictionType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ExactPredicateConflict => "exact_predicate_conflict",
            Self::MutuallyExclusivePredicates => "mutually_exclusive_predicates",
            Self::IncompatibleScalarValues => "incompatible_scalar_values",
            Self::LowerConfidenceReplacement => "lower_confidence_replacement",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

impl fmt::Display for Severity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        })
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContradictionOntology {
    mutually_exclusive: BTreeSet<PredicatePair>,
}

impl ContradictionOntology {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_mutually_exclusive(&mut self, left: PredicateId, right: PredicateId) {
        self.mutually_exclusive
            .insert(PredicatePair::new(left, right));
    }

    pub fn are_mutually_exclusive(&self, left: &PredicateId, right: &PredicateId) -> bool {
        self.mutually_exclusive
            .contains(&PredicatePair::new(left.clone(), right.clone()))
    }

    pub fn from_config_str(config: &str) -> Result<Self, ContradictionOntologyError> {
        let mut ontology = Self::new();
        for (line_index, raw_line) in config.lines().enumerate() {
            let trimmed = raw_line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let declaration = trimmed.split('#').next().unwrap_or("").trim();
            let parts = declaration
                .split('|')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>();
            if parts.len() != 2 {
                return Err(ContradictionOntologyError::MalformedLine {
                    line: line_index + 1,
                    text: raw_line.to_owned(),
                });
            }

            ontology.add_mutually_exclusive(PredicateId::new(parts[0]), PredicateId::new(parts[1]));
        }

        Ok(ontology)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContradictionOntologyError {
    MalformedLine { line: usize, text: String },
}

impl fmt::Display for ContradictionOntologyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedLine { line, text } => {
                write!(formatter, "malformed ontology line {line}: {text}")
            }
        }
    }
}

impl Error for ContradictionOntologyError {}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct PredicatePair(PredicateId, PredicateId);

impl PredicatePair {
    fn new(left: PredicateId, right: PredicateId) -> Self {
        if left <= right {
            Self(left, right)
        } else {
            Self(right, left)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Ord, PartialOrd)]
pub struct ConfidenceBucket(u8);

impl ConfidenceBucket {
    pub fn new(bucket: u8) -> Self {
        Self(bucket.min(10))
    }

    pub fn as_u8(self) -> u8 {
        self.0
    }
}

impl From<Confidence> for ConfidenceBucket {
    fn from(confidence: Confidence) -> Self {
        Self::new((confidence.as_f32() * 10.0).floor() as u8)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum ObjectIndexKey {
    Entity(EntityId),
    Text(String),
    Integer(i64),
    DecimalBits(u64),
    Boolean(bool),
    Time(ValidTime),
    Null,
}

impl From<&GraphValue> for ObjectIndexKey {
    fn from(value: &GraphValue) -> Self {
        match value {
            GraphValue::Entity(id) => Self::Entity(id.clone()),
            GraphValue::Text(value) => Self::Text(value.clone()),
            GraphValue::Integer(value) => Self::Integer(*value),
            GraphValue::Decimal(value) => Self::DecimalBits(value.to_bits()),
            GraphValue::Boolean(value) => Self::Boolean(*value),
            GraphValue::Time(value) => Self::Time(*value),
            GraphValue::Null => Self::Null,
        }
    }
}

fn push_hash_index<K>(index: &mut HashMap<K, Vec<AssertionId>>, key: K, assertion_id: AssertionId)
where
    K: Eq + std::hash::Hash,
{
    let ids = index.entry(key).or_default();
    ids.push(assertion_id);
    ids.sort();
    ids.dedup();
}

fn push_tree_index<K>(index: &mut BTreeMap<K, Vec<AssertionId>>, key: K, assertion_id: AssertionId)
where
    K: Ord,
{
    let ids = index.entry(key).or_default();
    ids.push(assertion_id);
    ids.sort();
    ids.dedup();
}

fn sorted_assertions<'a>(assertions: impl Iterator<Item = &'a Assertion>) -> Vec<&'a Assertion> {
    let mut assertions = assertions.collect::<Vec<_>>();
    assertions.sort_by(|left, right| left.id.cmp(&right.id));
    assertions
}

fn classify_contradiction(
    left: &Assertion,
    right: &Assertion,
    ontology: &ContradictionOntology,
) -> Option<Contradiction> {
    if left.subject != right.subject
        || left.context != right.context
        || !left.valid_time.overlaps(&right.valid_time)
    {
        return None;
    }

    if let Some(contradiction) = lower_confidence_replacement(left, right) {
        return Some(contradiction);
    }

    if left.status != AssertionStatus::Active || right.status != AssertionStatus::Active {
        return None;
    }

    if ontology.are_mutually_exclusive(&left.predicate, &right.predicate) {
        return Some(Contradiction::new(
            left,
            right,
            ContradictionType::MutuallyExclusivePredicates,
            Severity::Critical,
            format!(
                "Predicates {} and {} are configured as mutually exclusive over overlapping valid time.",
                left.predicate, right.predicate
            ),
        ));
    }

    if left.predicate == right.predicate && left.object != right.object {
        if is_scalar_value(&left.object) && is_scalar_value(&right.object) {
            return Some(Contradiction::new(
                left,
                right,
                ContradictionType::IncompatibleScalarValues,
                Severity::Medium,
                format!(
                    "Predicate {} has incompatible scalar values over overlapping valid time.",
                    left.predicate
                ),
            ));
        }

        return Some(Contradiction::new(
            left,
            right,
            ContradictionType::ExactPredicateConflict,
            Severity::High,
            format!(
                "Predicate {} points to incompatible graph values over overlapping valid time.",
                left.predicate
            ),
        ));
    }

    None
}

fn lower_confidence_replacement(left: &Assertion, right: &Assertion) -> Option<Contradiction> {
    if left.predicate != right.predicate || left.object != right.object {
        return None;
    }

    let (replacement, replaced) = match (
        left.status == AssertionStatus::Active,
        right.status == AssertionStatus::Active,
    ) {
        (true, false) if left.confidence > right.confidence => (left, right),
        (false, true) if right.confidence > left.confidence => (right, left),
        _ => return None,
    };

    Some(Contradiction::new(
        replacement,
        replaced,
        ContradictionType::LowerConfidenceReplacement,
        Severity::Low,
        format!(
            "Higher-confidence assertion {} replaces lower-confidence assertion {} for predicate {}.",
            replacement.id, replaced.id, replacement.predicate
        ),
    ))
}

fn is_scalar_value(value: &GraphValue) -> bool {
    matches!(
        value,
        GraphValue::Text(_)
            | GraphValue::Integer(_)
            | GraphValue::Decimal(_)
            | GraphValue::Boolean(_)
            | GraphValue::Time(_)
    )
}

fn contradiction_id(
    assertion_a: &AssertionId,
    assertion_b: &AssertionId,
    contradiction_type: ContradictionType,
) -> ContradictionId {
    ContradictionId::new(format!(
        "contradiction-{}-{}-{}",
        assertion_a,
        assertion_b,
        contradiction_type.slug()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rg_core::{
        AssertionId, AssertionStatus, Confidence, ContextScope, GraphValue, PredicateId, SourceId,
        TimeInterval, TxTime, ValidTime,
    };

    fn assertion(id: &str, object: GraphValue) -> Assertion {
        Assertion {
            id: AssertionId::new(id),
            subject: EntityId::new("person-a"),
            predicate: PredicateId::new("works_at"),
            object,
            valid_time: TimeInterval::new(ValidTime::new(10), None).expect("valid interval"),
            transaction_time: TimeInterval::new(TxTime::new(1), None)
                .expect("valid transaction interval"),
            confidence: Confidence::new(0.9).expect("valid confidence"),
            source_ids: vec![SourceId::new("source-1")],
            context: ContextScope::Global,
            status: AssertionStatus::Active,
        }
    }

    fn with_valid_time(mut assertion: Assertion, start: i64, end: Option<i64>) -> Assertion {
        assertion.valid_time = TimeInterval::new(ValidTime::new(start), end.map(ValidTime::new))
            .expect("valid interval");
        assertion
    }

    fn with_transaction_start(mut assertion: Assertion, start: i64) -> Assertion {
        assertion.transaction_time =
            TimeInterval::new(TxTime::new(start), None).expect("valid interval");
        assertion
    }

    fn with_confidence(mut assertion: Assertion, confidence: f32) -> Assertion {
        assertion.confidence = Confidence::new(confidence).expect("valid confidence");
        assertion
    }

    fn with_predicate(mut assertion: Assertion, predicate: &str) -> Assertion {
        assertion.predicate = PredicateId::new(predicate);
        assertion
    }

    fn with_context(mut assertion: Assertion, context: ContextScope) -> Assertion {
        assertion.context = context;
        assertion
    }

    fn with_subject(mut assertion: Assertion, subject: &str) -> Assertion {
        assertion.subject = EntityId::new(subject);
        assertion
    }

    fn with_status(mut assertion: Assertion, status: AssertionStatus) -> Assertion {
        assertion.status = status;
        assertion
    }

    fn only_contradiction(index: &TemporalIndex) -> Contradiction {
        let mut contradictions = index.contradictions();
        assert_eq!(
            contradictions.len(),
            1,
            "expected exactly one contradiction: {contradictions:?}"
        );
        contradictions.remove(0)
    }

    #[test]
    fn inserted_assertions_are_indexed_by_subject() {
        let subject = EntityId::new("person-a");
        let assertion = assertion(
            "assertion-1",
            GraphValue::Entity(EntityId::new("company-b")),
        );
        let mut index = AdjacencyIndex::new();

        index.insert_assertion(assertion.clone());

        assert_eq!(index.outgoing(&subject), &[assertion]);
        assert!(index.outgoing(&EntityId::new("missing")).is_empty());
    }

    #[test]
    fn queries_assertions_valid_at_time() {
        let current = with_valid_time(
            assertion("current", GraphValue::Entity(EntityId::new("company-b"))),
            10,
            Some(30),
        );
        let expired = with_valid_time(
            assertion("expired", GraphValue::Entity(EntityId::new("company-c"))),
            1,
            Some(9),
        );
        let mut index = TemporalIndex::new();

        index.insert_assertion(current.clone());
        index.insert_assertion(expired);

        assert_eq!(index.valid_at(ValidTime::new(15)), vec![&current]);
        assert!(index.valid_at(ValidTime::new(30)).is_empty());
    }

    #[test]
    fn queries_assertions_known_at_transaction_time() {
        let known = with_transaction_start(
            assertion("known", GraphValue::Entity(EntityId::new("company-b"))),
            50,
        );
        let future = with_transaction_start(
            assertion("future", GraphValue::Entity(EntityId::new("company-c"))),
            70,
        );
        let mut index = TemporalIndex::new();

        index.insert_assertion(known.clone());
        index.insert_assertion(future);

        assert_eq!(index.known_at(TxTime::new(50)), vec![&known]);
        assert!(index.known_at(TxTime::new(49)).is_empty());
    }

    #[test]
    fn queries_adjacent_edges_at_valid_time() {
        let edge = with_valid_time(
            assertion("edge", GraphValue::Entity(EntityId::new("company-b"))),
            10,
            Some(20),
        );
        let expired = with_valid_time(
            assertion("expired", GraphValue::Entity(EntityId::new("company-c"))),
            1,
            Some(5),
        );
        let mut index = TemporalIndex::new();

        index.insert_assertion(edge.clone());
        index.insert_assertion(expired);

        assert_eq!(
            index.adjacent_at(&EntityId::new("company-b"), ValidTime::new(15)),
            vec![&edge]
        );
        assert!(index
            .adjacent_at(&EntityId::new("company-b"), ValidTime::new(20))
            .is_empty());
    }

    #[test]
    fn ontology_config_parses_mutually_exclusive_predicates() {
        let ontology = ContradictionOntology::from_config_str(include_str!(
            "../../../schemas/ontology/mutually-exclusive-predicates.txt"
        ))
        .expect("ontology config parses");

        assert!(ontology.are_mutually_exclusive(
            &PredicateId::new("status_acquired"),
            &PredicateId::new("status_independent")
        ));
        assert!(ontology.are_mutually_exclusive(
            &PredicateId::new("status_independent"),
            &PredicateId::new("status_acquired")
        ));
        assert!(!ontology.are_mutually_exclusive(
            &PredicateId::new("status_acquired"),
            &PredicateId::new("located_in")
        ));
    }

    #[test]
    fn programmatic_ontology_marks_predicates_as_mutually_exclusive() {
        let mut ontology = ContradictionOntology::new();

        ontology.add_mutually_exclusive(PredicateId::new("active"), PredicateId::new("inactive"));

        assert!(ontology
            .are_mutually_exclusive(&PredicateId::new("active"), &PredicateId::new("inactive")));
    }

    #[test]
    fn detects_conflicting_employment_assertions() {
        let first = with_valid_time(
            with_predicate(
                assertion(
                    "employment-a",
                    GraphValue::Entity(EntityId::new("company-a")),
                ),
                "ceo_of",
            ),
            10,
            Some(20),
        );
        let second = with_transaction_start(
            with_valid_time(
                with_predicate(
                    assertion(
                        "employment-b",
                        GraphValue::Entity(EntityId::new("company-b")),
                    ),
                    "ceo_of",
                ),
                15,
                Some(25),
            ),
            2,
        );
        let mut index = TemporalIndex::new();

        index.insert_assertion(second.clone());
        index.insert_assertion(first.clone());
        let contradiction = only_contradiction(&index);

        assert_eq!(contradiction.assertion_a, first.id);
        assert_eq!(contradiction.assertion_b, second.id);
        assert_eq!(
            contradiction.contradiction_type,
            ContradictionType::ExactPredicateConflict
        );
        assert_eq!(contradiction.severity, Severity::High);
        assert_eq!(
            contradiction.id.as_str(),
            "contradiction-employment-a-employment-b-exact-predicate-conflict"
        );
        assert!(contradiction.explanation.contains("ceo_of"));
    }

    #[test]
    fn detects_conflicting_ownership_assertions() {
        let first = with_subject(
            with_valid_time(
                with_predicate(
                    assertion("ownership-a", GraphValue::Entity(EntityId::new("owner-a"))),
                    "owned_by",
                ),
                10,
                Some(20),
            ),
            "company-a",
        );
        let second = with_subject(
            with_transaction_start(
                with_valid_time(
                    with_predicate(
                        assertion("ownership-b", GraphValue::Entity(EntityId::new("owner-b"))),
                        "owned_by",
                    ),
                    15,
                    Some(25),
                ),
                2,
            ),
            "company-a",
        );
        let mut index = TemporalIndex::new();

        index.insert_assertion(first.clone());
        index.insert_assertion(second.clone());
        let contradiction = only_contradiction(&index);

        assert_eq!(contradiction.assertion_a, first.id);
        assert_eq!(contradiction.assertion_b, second.id);
        assert_eq!(
            contradiction.contradiction_type,
            ContradictionType::ExactPredicateConflict
        );
    }

    #[test]
    fn detects_conflicting_location_assertions_without_cross_context_noise() {
        let first = with_context(
            with_confidence(
                with_valid_time(
                    with_predicate(
                        assertion("first", GraphValue::Entity(EntityId::new("city-a"))),
                        "located_in",
                    ),
                    10,
                    Some(20),
                ),
                0.7,
            ),
            ContextScope::Named("world".to_owned()),
        );
        let second = with_context(
            with_confidence(
                with_transaction_start(
                    with_valid_time(
                        with_predicate(
                            assertion("second", GraphValue::Entity(EntityId::new("city-b"))),
                            "located_in",
                        ),
                        15,
                        Some(25),
                    ),
                    2,
                ),
                0.8,
            ),
            ContextScope::Named("world".to_owned()),
        );
        let different_context = with_context(
            with_confidence(
                with_transaction_start(
                    with_valid_time(
                        with_predicate(
                            assertion(
                                "different-context",
                                GraphValue::Entity(EntityId::new("city-c")),
                            ),
                            "located_in",
                        ),
                        15,
                        Some(25),
                    ),
                    2,
                ),
                0.8,
            ),
            ContextScope::Named("simulation".to_owned()),
        );
        let mut index = TemporalIndex::new();

        index.insert_assertion(first.clone());
        index.insert_assertion(second.clone());
        index.insert_assertion(different_context);
        let contradiction = only_contradiction(&index);

        assert_eq!(contradiction.assertion_a, first.id);
        assert_eq!(contradiction.assertion_b, second.id);
        assert_eq!(
            contradiction.contradiction_type,
            ContradictionType::ExactPredicateConflict
        );
    }

    #[test]
    fn detects_incompatible_numeric_values() {
        let first = with_subject(
            with_valid_time(
                with_predicate(
                    assertion("employees-a", GraphValue::Integer(100)),
                    "employee_count",
                ),
                10,
                Some(20),
            ),
            "company-a",
        );
        let second = with_subject(
            with_transaction_start(
                with_valid_time(
                    with_predicate(
                        assertion("employees-b", GraphValue::Integer(125)),
                        "employee_count",
                    ),
                    15,
                    Some(25),
                ),
                2,
            ),
            "company-a",
        );
        let mut index = TemporalIndex::new();

        index.insert_assertion(first.clone());
        index.insert_assertion(second.clone());
        let contradiction = only_contradiction(&index);

        assert_eq!(contradiction.assertion_a, first.id);
        assert_eq!(contradiction.assertion_b, second.id);
        assert_eq!(
            contradiction.contradiction_type,
            ContradictionType::IncompatibleScalarValues
        );
        assert_eq!(contradiction.severity, Severity::Medium);
    }

    #[test]
    fn detects_mutually_exclusive_status_assertions_from_ontology() {
        let ontology = ContradictionOntology::from_config_str(include_str!(
            "../../../schemas/ontology/mutually-exclusive-predicates.txt"
        ))
        .expect("ontology config parses");
        let acquired = with_subject(
            with_valid_time(
                with_predicate(
                    assertion("status-a", GraphValue::Text("acquired".to_owned())),
                    "status_acquired",
                ),
                10,
                Some(20),
            ),
            "company-a",
        );
        let independent = with_subject(
            with_transaction_start(
                with_valid_time(
                    with_predicate(
                        assertion("status-b", GraphValue::Text("independent".to_owned())),
                        "status_independent",
                    ),
                    15,
                    Some(25),
                ),
                2,
            ),
            "company-a",
        );
        let mut index = TemporalIndex::with_ontology(ontology);

        index.insert_assertion(acquired.clone());
        index.insert_assertion(independent.clone());
        let contradiction = only_contradiction(&index);

        assert_eq!(contradiction.assertion_a, acquired.id);
        assert_eq!(contradiction.assertion_b, independent.id);
        assert_eq!(
            contradiction.contradiction_type,
            ContradictionType::MutuallyExclusivePredicates
        );
        assert_eq!(contradiction.severity, Severity::Critical);
    }

    #[test]
    fn detects_lower_confidence_replacement_after_retraction() {
        let lower_confidence = with_status(
            with_confidence(
                with_valid_time(
                    with_predicate(
                        assertion(
                            "relationship-low",
                            GraphValue::Entity(EntityId::new("company-a")),
                        ),
                        "worked_at",
                    ),
                    10,
                    Some(20),
                ),
                0.4,
            ),
            AssertionStatus::Retracted,
        );
        let higher_confidence = with_confidence(
            with_transaction_start(
                with_valid_time(
                    with_predicate(
                        assertion(
                            "relationship-high",
                            GraphValue::Entity(EntityId::new("company-a")),
                        ),
                        "worked_at",
                    ),
                    10,
                    Some(20),
                ),
                2,
            ),
            0.9,
        );
        let mut index = TemporalIndex::new();

        index.insert_assertion(lower_confidence.clone());
        index.insert_assertion(higher_confidence.clone());
        let contradiction = only_contradiction(&index);

        assert_eq!(contradiction.assertion_a, higher_confidence.id);
        assert_eq!(contradiction.assertion_b, lower_confidence.id);
        assert_eq!(
            contradiction.contradiction_type,
            ContradictionType::LowerConfidenceReplacement
        );
        assert_eq!(contradiction.severity, Severity::Low);
    }
}
