//! Internal query model and execution for Reality Graph.

use std::collections::BTreeSet;

use rg_core::{
    Assertion, AssertionId, Confidence, ContextScope, EntityId, GraphValue, PredicateId, SourceId,
    TxTime, ValidTime,
};
use rg_index::AdjacencyIndex;
use rg_storage::InMemoryStorage;

pub type Timestamp = i64;

#[derive(Clone, Debug, PartialEq)]
pub struct GraphQuery {
    pub subject: Option<EntityPattern>,
    pub predicate: Option<PredicatePattern>,
    pub object: Option<ObjectPattern>,
    pub valid_at: Option<Timestamp>,
    pub known_at: Option<Timestamp>,
    pub context: Option<ContextScope>,
    pub min_confidence: Option<f32>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntityPattern {
    Id(EntityId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PredicatePattern {
    Id(PredicateId),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ObjectPattern {
    Entity(EntityId),
    Value(GraphValue),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PathQuery {
    pub start: EntityId,
    pub end: Option<EntityId>,
    pub predicates: Vec<PredicateId>,
    pub valid_at: Option<Timestamp>,
    pub max_depth: usize,
    pub min_confidence: Option<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueryResult {
    pub assertion_id: AssertionId,
    pub subject: EntityId,
    pub predicate: PredicateId,
    pub object: GraphValue,
    pub valid_from: ValidTime,
    pub valid_to: Option<ValidTime>,
    pub tx_from: TxTime,
    pub tx_to: Option<TxTime>,
    pub confidence: Confidence,
    pub source_ids: Vec<SourceId>,
    pub context: ContextScope,
}

impl QueryResult {
    pub fn from_assertion(assertion: &Assertion) -> Self {
        Self {
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
}

#[derive(Clone, Debug, PartialEq)]
pub struct PathResult {
    pub start: EntityId,
    pub end: EntityId,
    pub hops: Vec<QueryResult>,
}

#[derive(Default)]
pub struct QueryEngine {
    adjacency: AdjacencyIndex,
    storage: Option<InMemoryStorage>,
}

impl QueryEngine {
    pub fn new(adjacency: AdjacencyIndex) -> Self {
        Self {
            adjacency,
            storage: None,
        }
    }

    pub fn from_storage(storage: InMemoryStorage) -> Self {
        let mut adjacency = AdjacencyIndex::new();
        for assertion in storage.graph_state().assertions.values() {
            adjacency.insert_assertion(assertion.clone());
        }
        Self {
            adjacency,
            storage: Some(storage),
        }
    }

    pub fn outgoing(&self, entity: &EntityId) -> &[Assertion] {
        self.adjacency.outgoing(entity)
    }

    pub fn execute_graph(&self, query: GraphQuery) -> Vec<QueryResult> {
        let Some(storage) = &self.storage else {
            return Vec::new();
        };
        let mut assertions = graph_candidates(storage, &query);
        assertions.retain(|assertion| matches_graph_query(assertion, &query));
        assertions.sort_by(|left, right| left.id.cmp(&right.id));
        if let Some(limit) = query.limit {
            assertions.truncate(limit);
        }
        assertions
            .into_iter()
            .map(QueryResult::from_assertion)
            .collect()
    }

    pub fn execute_path(&self, query: PathQuery) -> Vec<PathResult> {
        let Some(storage) = &self.storage else {
            return Vec::new();
        };
        if query.max_depth == 0 {
            return Vec::new();
        }

        let mut results = Vec::new();
        let mut visited = BTreeSet::new();
        visited.insert(query.start.clone());
        let mut hops = Vec::new();
        search_paths(
            storage,
            &query,
            query.start.clone(),
            &mut visited,
            &mut hops,
            &mut results,
        );
        results.sort_by(|left, right| {
            left.hops
                .iter()
                .map(|hop| hop.assertion_id.as_str())
                .cmp(right.hops.iter().map(|hop| hop.assertion_id.as_str()))
        });
        results
    }
}

fn graph_candidates<'a>(storage: &'a InMemoryStorage, query: &GraphQuery) -> Vec<&'a Assertion> {
    if let Some(EntityPattern::Id(subject)) = &query.subject {
        storage.assertions_by_subject(subject)
    } else if let Some(PredicatePattern::Id(predicate)) = &query.predicate {
        storage.assertions_by_predicate(predicate)
    } else if let Some(object) = &query.object {
        storage.assertions_by_object(&object.as_graph_value())
    } else {
        storage.graph_state().assertions.values().collect()
    }
}

fn matches_graph_query(assertion: &Assertion, query: &GraphQuery) -> bool {
    query
        .subject
        .as_ref()
        .map_or(true, |pattern| pattern.matches(&assertion.subject))
        && query
            .predicate
            .as_ref()
            .map_or(true, |pattern| pattern.matches(&assertion.predicate))
        && query
            .object
            .as_ref()
            .map_or(true, |pattern| pattern.matches(&assertion.object))
        && query.valid_at.map_or(true, |instant| {
            assertion.valid_time.contains(ValidTime::new(instant))
        })
        && query.known_at.map_or(true, |instant| {
            assertion.transaction_time.contains(TxTime::new(instant))
        })
        && query
            .context
            .as_ref()
            .map_or(true, |context| &assertion.context == context)
        && query
            .min_confidence
            .map_or(true, |minimum| assertion.confidence.as_f32() >= minimum)
}

impl EntityPattern {
    fn matches(&self, entity: &EntityId) -> bool {
        match self {
            Self::Id(id) => id == entity,
        }
    }
}

impl PredicatePattern {
    fn matches(&self, predicate: &PredicateId) -> bool {
        match self {
            Self::Id(id) => id == predicate,
        }
    }
}

impl ObjectPattern {
    fn matches(&self, object: &GraphValue) -> bool {
        match self {
            Self::Entity(id) => object == &GraphValue::Entity(id.clone()),
            Self::Value(value) => value == object,
        }
    }

    fn as_graph_value(&self) -> GraphValue {
        match self {
            Self::Entity(id) => GraphValue::Entity(id.clone()),
            Self::Value(value) => value.clone(),
        }
    }
}

fn search_paths(
    storage: &InMemoryStorage,
    query: &PathQuery,
    current: EntityId,
    visited: &mut BTreeSet<EntityId>,
    hops: &mut Vec<QueryResult>,
    results: &mut Vec<PathResult>,
) {
    if hops.len() == query.max_depth {
        return;
    }
    if !query.predicates.is_empty() && hops.len() >= query.predicates.len() {
        return;
    }

    let mut candidates = storage.assertions_by_subject(&current);
    candidates.sort_by(|left, right| left.id.cmp(&right.id));
    for assertion in candidates {
        if !matches_path_edge(assertion, query, hops.len()) {
            continue;
        }
        let GraphValue::Entity(next) = &assertion.object else {
            continue;
        };
        if visited.contains(next) {
            continue;
        }

        hops.push(QueryResult::from_assertion(assertion));
        if query.end.as_ref().map_or(true, |end| end == next) {
            results.push(PathResult {
                start: query.start.clone(),
                end: next.clone(),
                hops: hops.clone(),
            });
        }
        visited.insert(next.clone());
        search_paths(storage, query, next.clone(), visited, hops, results);
        visited.remove(next);
        hops.pop();
    }
}

fn matches_path_edge(assertion: &Assertion, query: &PathQuery, depth: usize) -> bool {
    query
        .predicates
        .get(depth)
        .map_or(true, |predicate| predicate == &assertion.predicate)
        && query.valid_at.map_or(true, |instant| {
            assertion.valid_time.contains(ValidTime::new(instant))
        })
        && query
            .min_confidence
            .map_or(true, |minimum| assertion.confidence.as_f32() >= minimum)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rg_core::{
        AssertionId, AssertionStatus, Confidence, ContextScope, EntityType, GraphValue,
        PredicateId, PropertyMap, SourceId, TimeInterval, TxTime, ValidTime,
    };
    use rg_events::{
        AddAssertion, AddSource, ContentHash, CreateEntity, EventLog, GraphCommand, SourceType,
    };
    use rg_storage::InMemoryStorage;

    #[test]
    fn query_engine_reads_outgoing_assertions() {
        let subject = EntityId::new("person-a");
        let assertion = Assertion {
            id: AssertionId::new("assertion-1"),
            subject: subject.clone(),
            predicate: PredicateId::new("works_at"),
            object: GraphValue::Entity(EntityId::new("company-b")),
            valid_time: TimeInterval::new(ValidTime::new(10), None).expect("valid interval"),
            transaction_time: TimeInterval::new(TxTime::new(20), None).expect("valid interval"),
            confidence: Confidence::new(0.9).expect("valid confidence"),
            source_ids: vec![SourceId::new("source-1")],
            context: ContextScope::Global,
            status: AssertionStatus::Active,
        };
        let mut adjacency = AdjacencyIndex::new();
        adjacency.insert_assertion(assertion.clone());
        let query = QueryEngine::new(adjacency);

        assert_eq!(query.outgoing(&subject), &[assertion]);
    }

    fn test_storage() -> InMemoryStorage {
        let mut log = EventLog::new(TxTime::new(0));
        log.execute(GraphCommand::AddSource(AddSource {
            id: SourceId::new("source-employment"),
            source_type: SourceType::Document,
            uri: Some("file://employment.md".to_owned()),
            content_hash: ContentHash::new("sha256:employment"),
            trust_score: Some(0.95),
        }))
        .expect("source added");
        for (id, entity_type, name) in [
            ("person-a", EntityType::Person, "Person A"),
            ("company-b", EntityType::Organization, "Company B"),
            ("city-c", EntityType::Place, "City C"),
        ] {
            log.execute(GraphCommand::CreateEntity(CreateEntity {
                id: EntityId::new(id),
                entity_type,
                canonical_name: Some(name.to_owned()),
                properties: PropertyMap::default(),
            }))
            .expect("entity added");
        }
        log.execute(GraphCommand::AddAssertion(AddAssertion {
            id: AssertionId::new("assertion-worked-at"),
            subject: EntityId::new("person-a"),
            predicate: PredicateId::new("worked_at"),
            object: GraphValue::Entity(EntityId::new("company-b")),
            valid_time: TimeInterval::new(ValidTime::new(2021), Some(ValidTime::new(2025)))
                .expect("valid interval"),
            confidence: Confidence::new(0.92).expect("valid confidence"),
            source_ids: vec![SourceId::new("source-employment")],
            context: ContextScope::Named("world".to_owned()),
        }))
        .expect("assertion added");
        log.execute(GraphCommand::AddAssertion(AddAssertion {
            id: AssertionId::new("assertion-located-in"),
            subject: EntityId::new("company-b"),
            predicate: PredicateId::new("located_in"),
            object: GraphValue::Entity(EntityId::new("city-c")),
            valid_time: TimeInterval::new(ValidTime::new(2020), None).expect("valid interval"),
            confidence: Confidence::new(0.88).expect("valid confidence"),
            source_ids: vec![SourceId::new("source-employment")],
            context: ContextScope::Named("world".to_owned()),
        }))
        .expect("assertion added");
        log.execute(GraphCommand::AddAssertion(AddAssertion {
            id: AssertionId::new("assertion-low-confidence"),
            subject: EntityId::new("person-a"),
            predicate: PredicateId::new("worked_at"),
            object: GraphValue::Entity(EntityId::new("city-c")),
            valid_time: TimeInterval::new(ValidTime::new(2021), Some(ValidTime::new(2025)))
                .expect("valid interval"),
            confidence: Confidence::new(0.4).expect("valid confidence"),
            source_ids: vec![SourceId::new("source-employment")],
            context: ContextScope::Named("world".to_owned()),
        }))
        .expect("assertion added");

        InMemoryStorage::replay(log.events()).expect("storage replay")
    }

    #[test]
    fn graph_query_filters_and_returns_provenance() {
        let engine = QueryEngine::from_storage(test_storage());
        let results = engine.execute_graph(GraphQuery {
            subject: Some(EntityPattern::Id(EntityId::new("person-a"))),
            predicate: Some(PredicatePattern::Id(PredicateId::new("worked_at"))),
            object: Some(ObjectPattern::Entity(EntityId::new("company-b"))),
            valid_at: Some(2024),
            known_at: Some(5),
            context: Some(ContextScope::Named("world".to_owned())),
            min_confidence: Some(0.8),
            limit: Some(10),
        });

        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].assertion_id,
            AssertionId::new("assertion-worked-at")
        );
        assert_eq!(results[0].subject, EntityId::new("person-a"));
        assert_eq!(results[0].predicate, PredicateId::new("worked_at"));
        assert_eq!(
            results[0].object,
            GraphValue::Entity(EntityId::new("company-b"))
        );
        assert_eq!(results[0].valid_from, ValidTime::new(2021));
        assert_eq!(results[0].valid_to, Some(ValidTime::new(2025)));
        assert_eq!(results[0].confidence.as_f32(), 0.92);
        assert_eq!(
            results[0].source_ids,
            vec![SourceId::new("source-employment")]
        );
        assert_eq!(results[0].context, ContextScope::Named("world".to_owned()));
    }

    #[test]
    fn graph_query_limit_is_deterministic() {
        let engine = QueryEngine::from_storage(test_storage());
        let results = engine.execute_graph(GraphQuery {
            subject: Some(EntityPattern::Id(EntityId::new("person-a"))),
            predicate: None,
            object: None,
            valid_at: Some(2024),
            known_at: None,
            context: None,
            min_confidence: None,
            limit: Some(1),
        });

        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].assertion_id,
            AssertionId::new("assertion-low-confidence")
        );
    }

    #[test]
    fn path_query_returns_provenance_for_each_hop() {
        let engine = QueryEngine::from_storage(test_storage());
        let paths = engine.execute_path(PathQuery {
            start: EntityId::new("person-a"),
            end: Some(EntityId::new("city-c")),
            predicates: vec![
                PredicateId::new("worked_at"),
                PredicateId::new("located_in"),
            ],
            valid_at: Some(2024),
            max_depth: 2,
            min_confidence: Some(0.8),
        });

        assert_eq!(paths.len(), 1);
        assert_eq!(
            paths[0]
                .hops
                .iter()
                .map(|hop| hop.assertion_id.as_str())
                .collect::<Vec<_>>(),
            vec!["assertion-worked-at", "assertion-located-in"]
        );
        assert_eq!(
            paths[0].hops[0].source_ids,
            vec![SourceId::new("source-employment")]
        );
    }
}
