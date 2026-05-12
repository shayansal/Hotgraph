//! Simulation helpers and synthetic graph events.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;

use rg_core::{
    Assertion, AssertionId, AssertionStatus, CausalLinkId, Confidence, EventId, GraphValue,
    PredicateId, SourceId, TxTime, ValidTime,
};
use rg_events::{
    CreateEntity, EntityId, EntityType, EventLog, GraphCommand, GraphEvent, GraphState, PropertyMap,
};

#[derive(Clone, Debug, PartialEq)]
pub struct CausalLink {
    pub id: CausalLinkId,
    pub cause_event: EventId,
    pub effect_event: EventId,
    pub confidence: Confidence,
    pub mechanism: Option<String>,
    pub lag: TimeLag,
    pub counterfactual_note: Option<String>,
    pub source_ids: Vec<SourceId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimeLag(String);

impl TimeLag {
    pub fn new(value: impl Into<String>) -> Result<Self, CausalModelError> {
        let value = value.into();
        if value.trim().is_empty() || !value.starts_with('P') {
            return Err(CausalModelError::InvalidLag);
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CausalModelError {
    InvalidLag,
}

impl fmt::Display for CausalModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLag => formatter.write_str("causal lag must be an ISO-8601 period"),
        }
    }
}

impl Error for CausalModelError {}

#[derive(Clone, Debug, PartialEq)]
pub struct CausalPath {
    pub start: EventId,
    pub end: EventId,
    pub links: Vec<CausalLink>,
    pub confidence: Confidence,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CausalPathQuery {
    pub start: EventId,
    pub end: Option<EventId>,
    pub max_depth: usize,
    pub min_confidence: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CausalGraph {
    outgoing: BTreeMap<EventId, Vec<CausalLink>>,
}

impl CausalGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_link(&mut self, link: CausalLink) {
        let links = self.outgoing.entry(link.cause_event.clone()).or_default();
        links.push(link);
        links.sort_by(|left, right| left.id.cmp(&right.id));
        links.dedup_by(|left, right| left.id == right.id);
    }

    pub fn causal_paths(&self, query: CausalPathQuery) -> Vec<CausalPath> {
        if query.max_depth == 0 {
            return Vec::new();
        }

        let mut results = Vec::new();
        let mut path = Vec::new();
        let mut visited = BTreeSet::from([query.start.clone()]);
        self.walk_paths(&query.start, &query, &mut visited, &mut path, &mut results);
        results.sort_by(|left, right| {
            left.links
                .len()
                .cmp(&right.links.len())
                .then_with(|| causal_path_key(left).cmp(&causal_path_key(right)))
        });
        results
    }

    fn walk_paths(
        &self,
        current: &EventId,
        query: &CausalPathQuery,
        visited: &mut BTreeSet<EventId>,
        path: &mut Vec<CausalLink>,
        results: &mut Vec<CausalPath>,
    ) {
        let Some(links) = self.outgoing.get(current) else {
            return;
        };

        for link in links {
            if path.len() >= query.max_depth {
                return;
            }
            if query
                .min_confidence
                .is_some_and(|minimum| link.confidence.as_f32() < minimum)
            {
                continue;
            }
            if visited.contains(&link.effect_event) {
                continue;
            }

            path.push(link.clone());
            visited.insert(link.effect_event.clone());

            let reached_requested_end = query
                .end
                .as_ref()
                .is_some_and(|end| end == &link.effect_event);
            if reached_requested_end || query.end.is_none() {
                results.push(CausalPath {
                    start: query.start.clone(),
                    end: link.effect_event.clone(),
                    links: path.clone(),
                    confidence: path_confidence(path),
                });
            }

            if !reached_requested_end {
                self.walk_paths(&link.effect_event, query, visited, path, results);
            }

            visited.remove(&link.effect_event);
            path.pop();
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CounterfactualIntervention {
    AddAssertion(Assertion),
    RemoveAssertion(AssertionId),
    ModifyAssertion {
        assertion_id: AssertionId,
        replacement: Assertion,
    },
    AddEvent(EventId),
    RemoveEvent(EventId),
    ModifyEvent {
        event_id: EventId,
        replacement: EventId,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct CounterfactualRule {
    pub predicate: PredicateId,
    pub confidence_delta: f32,
    pub explanation: String,
}

impl CounterfactualRule {
    pub fn predicate_delta(
        predicate: impl Into<String>,
        confidence_delta: f32,
        explanation: impl Into<String>,
    ) -> Self {
        Self {
            predicate: PredicateId::new(predicate),
            confidence_delta,
            explanation: explanation.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CounterfactualRequest {
    pub valid_at: ValidTime,
    pub intervention: CounterfactualIntervention,
    pub max_depth: usize,
    pub rules: Vec<CounterfactualRule>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AffectedPath {
    pub start: EntityId,
    pub end: EntityId,
    pub assertions: Vec<AssertionId>,
    pub depth: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConfidenceDelta {
    pub assertion_id: AssertionId,
    pub before: Option<Confidence>,
    pub after: Option<Confidence>,
    pub delta: f32,
    pub explanation: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CounterfactualResult {
    pub changed_entities: Vec<EntityId>,
    pub changed_assertions: Vec<AssertionId>,
    pub changed_events: Vec<EventId>,
    pub impacted_entities: Vec<EntityId>,
    pub impacted_assertions: Vec<AssertionId>,
    pub affected_paths: Vec<AffectedPath>,
    pub confidence_deltas: Vec<ConfidenceDelta>,
    pub explanation_trace: Vec<String>,
}

pub struct CounterfactualEngine;

impl CounterfactualEngine {
    pub fn evaluate(snapshot: &GraphState, request: CounterfactualRequest) -> CounterfactualResult {
        let mut explanation_trace = Vec::new();
        let mutation = apply_intervention(snapshot, &request, &mut explanation_trace);
        let affected_paths = affected_paths(
            &mutation.assertions,
            &mutation.changed_entities,
            request.valid_at,
            request.max_depth,
        );
        let impacted_entities = impacted_entities(&mutation.changed_entities, &affected_paths);
        let impacted_assertions = impacted_assertions(&affected_paths);
        let confidence_deltas = confidence_deltas(
            &mutation,
            &impacted_assertions,
            &request.rules,
            &mut explanation_trace,
        );

        for path in &affected_paths {
            explanation_trace.push(format!(
                "depth {}: {} -> {} via {}",
                path.depth,
                path.start,
                path.end,
                path.assertions
                    .iter()
                    .map(AssertionId::as_str)
                    .collect::<Vec<_>>()
                    .join(" > ")
            ));
        }

        CounterfactualResult {
            changed_entities: mutation.changed_entities,
            changed_assertions: mutation.changed_assertions,
            changed_events: mutation.changed_events,
            impacted_entities,
            impacted_assertions,
            affected_paths,
            confidence_deltas,
            explanation_trace,
        }
    }
}

pub fn seed_events() -> Vec<GraphEvent> {
    let mut log = EventLog::new(TxTime::new(0));
    log.execute(GraphCommand::CreateEntity(CreateEntity {
        id: EntityId::new("observer"),
        entity_type: EntityType::Concept,
        canonical_name: Some("observer".to_owned()),
        properties: PropertyMap::default(),
    }))
    .expect("observer event is valid");
    log.execute(GraphCommand::CreateEntity(CreateEntity {
        id: EntityId::new("world"),
        entity_type: EntityType::Concept,
        canonical_name: Some("world".to_owned()),
        properties: PropertyMap::default(),
    }))
    .expect("world event is valid");
    log.events().to_vec()
}

#[derive(Clone, Debug)]
struct CounterfactualMutation {
    assertions: Vec<Assertion>,
    changed_entities: Vec<EntityId>,
    changed_assertions: Vec<AssertionId>,
    changed_events: Vec<EventId>,
    direct_deltas: Vec<ConfidenceDelta>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TraversalEdge {
    to: EntityId,
    assertion_id: AssertionId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PathFrame {
    start: EntityId,
    current: EntityId,
    assertions: Vec<AssertionId>,
    visited_entities: BTreeSet<EntityId>,
}

fn apply_intervention(
    snapshot: &GraphState,
    request: &CounterfactualRequest,
    explanation_trace: &mut Vec<String>,
) -> CounterfactualMutation {
    let mut assertions = active_assertions(snapshot, request.valid_at);
    let mut changed_entities = BTreeSet::new();
    let mut changed_assertions = BTreeSet::new();
    let mut changed_events = BTreeSet::new();
    let mut direct_deltas = Vec::new();

    match &request.intervention {
        CounterfactualIntervention::AddAssertion(assertion) => {
            collect_assertion_entities(assertion, &mut changed_entities);
            changed_assertions.insert(assertion.id.clone());
            direct_deltas.push(ConfidenceDelta {
                assertion_id: assertion.id.clone(),
                before: None,
                after: Some(assertion.confidence),
                delta: assertion.confidence.as_f32(),
                explanation: "intervention adds assertion".to_owned(),
            });
            explanation_trace.push(format!(
                "add assertion {} at valid time {}",
                assertion.id,
                request.valid_at.as_i64()
            ));
            if assertion_is_active_at(assertion, request.valid_at) {
                assertions.push(assertion.clone());
            }
        }
        CounterfactualIntervention::RemoveAssertion(assertion_id) => {
            assertions.retain(|assertion| &assertion.id != assertion_id);
            changed_assertions.insert(assertion_id.clone());
            if let Some(assertion) = snapshot.assertions.get(assertion_id) {
                collect_assertion_entities(assertion, &mut changed_entities);
                direct_deltas.push(ConfidenceDelta {
                    assertion_id: assertion.id.clone(),
                    before: Some(assertion.confidence),
                    after: None,
                    delta: -assertion.confidence.as_f32(),
                    explanation: "intervention removes assertion".to_owned(),
                });
            }
            explanation_trace.push(format!(
                "remove assertion {} at valid time {}",
                assertion_id,
                request.valid_at.as_i64()
            ));
        }
        CounterfactualIntervention::ModifyAssertion {
            assertion_id,
            replacement,
        } => {
            assertions.retain(|assertion| &assertion.id != assertion_id);
            changed_assertions.insert(assertion_id.clone());
            if let Some(assertion) = snapshot.assertions.get(assertion_id) {
                collect_assertion_entities(assertion, &mut changed_entities);
                direct_deltas.push(ConfidenceDelta {
                    assertion_id: assertion.id.clone(),
                    before: Some(assertion.confidence),
                    after: Some(replacement.confidence),
                    delta: replacement.confidence.as_f32() - assertion.confidence.as_f32(),
                    explanation: "intervention modifies assertion".to_owned(),
                });
            }
            collect_assertion_entities(replacement, &mut changed_entities);
            if assertion_is_active_at(replacement, request.valid_at) {
                assertions.push(replacement.clone());
            }
            explanation_trace.push(format!(
                "modify assertion {} at valid time {}",
                assertion_id,
                request.valid_at.as_i64()
            ));
        }
        CounterfactualIntervention::AddEvent(event_id) => {
            changed_events.insert(event_id.clone());
            explanation_trace.push(format!("add event {event_id}"));
        }
        CounterfactualIntervention::RemoveEvent(event_id) => {
            changed_events.insert(event_id.clone());
            explanation_trace.push(format!("remove event {event_id}"));
        }
        CounterfactualIntervention::ModifyEvent {
            event_id,
            replacement,
        } => {
            changed_events.insert(event_id.clone());
            changed_events.insert(replacement.clone());
            explanation_trace.push(format!("modify event {event_id} -> {replacement}"));
        }
    }

    assertions.sort_by(|left, right| left.id.cmp(&right.id));
    assertions.dedup_by(|left, right| left.id == right.id);
    direct_deltas.sort_by(|left, right| left.assertion_id.cmp(&right.assertion_id));

    CounterfactualMutation {
        assertions,
        changed_entities: changed_entities.into_iter().collect(),
        changed_assertions: changed_assertions.into_iter().collect(),
        changed_events: changed_events.into_iter().collect(),
        direct_deltas,
    }
}

fn active_assertions(snapshot: &GraphState, valid_at: ValidTime) -> Vec<Assertion> {
    let mut assertions = snapshot
        .assertions
        .values()
        .filter(|assertion| assertion_is_active_at(assertion, valid_at))
        .cloned()
        .collect::<Vec<_>>();
    assertions.sort_by(|left, right| left.id.cmp(&right.id));
    assertions
}

fn assertion_is_active_at(assertion: &Assertion, valid_at: ValidTime) -> bool {
    assertion.status == AssertionStatus::Active && assertion.valid_time.contains(valid_at)
}

fn collect_assertion_entities(assertion: &Assertion, entities: &mut BTreeSet<EntityId>) {
    entities.insert(assertion.subject.clone());
    if let GraphValue::Entity(entity_id) = &assertion.object {
        entities.insert(entity_id.clone());
    }
}

fn affected_paths(
    assertions: &[Assertion],
    starts: &[EntityId],
    valid_at: ValidTime,
    max_depth: usize,
) -> Vec<AffectedPath> {
    if max_depth == 0 {
        return Vec::new();
    }

    let adjacency = assertion_adjacency(assertions, valid_at);
    let mut results = Vec::new();
    let mut queue = VecDeque::new();
    for start in starts {
        queue.push_back(PathFrame {
            start: start.clone(),
            current: start.clone(),
            assertions: Vec::new(),
            visited_entities: BTreeSet::from([start.clone()]),
        });
    }

    while let Some(frame) = queue.pop_front() {
        if frame.assertions.len() >= max_depth {
            continue;
        }
        let Some(edges) = adjacency.get(&frame.current) else {
            continue;
        };

        for edge in edges {
            if frame.visited_entities.contains(&edge.to) {
                continue;
            }
            if frame.assertions.contains(&edge.assertion_id) {
                continue;
            }

            let mut assertions = frame.assertions.clone();
            assertions.push(edge.assertion_id.clone());
            let mut visited_entities = frame.visited_entities.clone();
            visited_entities.insert(edge.to.clone());

            results.push(AffectedPath {
                start: frame.start.clone(),
                end: edge.to.clone(),
                depth: assertions.len(),
                assertions: assertions.clone(),
            });

            queue.push_back(PathFrame {
                start: frame.start.clone(),
                current: edge.to.clone(),
                assertions,
                visited_entities,
            });
        }
    }

    results.sort_by(|left, right| {
        left.depth
            .cmp(&right.depth)
            .then_with(|| left.assertions.cmp(&right.assertions))
            .then_with(|| left.start.cmp(&right.start))
            .then_with(|| left.end.cmp(&right.end))
    });
    results.dedup();
    results
}

fn assertion_adjacency(
    assertions: &[Assertion],
    valid_at: ValidTime,
) -> BTreeMap<EntityId, Vec<TraversalEdge>> {
    let mut adjacency: BTreeMap<EntityId, Vec<TraversalEdge>> = BTreeMap::new();
    for assertion in assertions {
        if !assertion_is_active_at(assertion, valid_at) {
            continue;
        }
        let GraphValue::Entity(entity_id) = &assertion.object else {
            continue;
        };

        adjacency
            .entry(assertion.subject.clone())
            .or_default()
            .push(TraversalEdge {
                to: entity_id.clone(),
                assertion_id: assertion.id.clone(),
            });
        adjacency
            .entry(entity_id.clone())
            .or_default()
            .push(TraversalEdge {
                to: assertion.subject.clone(),
                assertion_id: assertion.id.clone(),
            });
    }

    for edges in adjacency.values_mut() {
        edges.sort_by(|left, right| {
            left.assertion_id
                .cmp(&right.assertion_id)
                .then_with(|| left.to.cmp(&right.to))
        });
        edges.dedup();
    }

    adjacency
}

fn impacted_entities(changed_entities: &[EntityId], paths: &[AffectedPath]) -> Vec<EntityId> {
    let mut impacted = changed_entities.iter().cloned().collect::<BTreeSet<_>>();
    for path in paths {
        impacted.insert(path.start.clone());
        impacted.insert(path.end.clone());
    }
    impacted.into_iter().collect()
}

fn impacted_assertions(paths: &[AffectedPath]) -> Vec<AssertionId> {
    let mut impacted = BTreeSet::new();
    for path in paths {
        impacted.extend(path.assertions.iter().cloned());
    }
    impacted.into_iter().collect()
}

fn confidence_deltas(
    mutation: &CounterfactualMutation,
    impacted_assertions: &[AssertionId],
    rules: &[CounterfactualRule],
    explanation_trace: &mut Vec<String>,
) -> Vec<ConfidenceDelta> {
    let mut deltas = mutation.direct_deltas.clone();
    let changed_assertions = mutation
        .changed_assertions
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();

    for assertion_id in impacted_assertions {
        if changed_assertions.contains(assertion_id) {
            continue;
        }
        let Some(assertion) = mutation
            .assertions
            .iter()
            .find(|assertion| &assertion.id == assertion_id)
        else {
            continue;
        };
        for rule in rules {
            if rule.predicate != assertion.predicate {
                continue;
            }
            let before = assertion.confidence;
            let after = clamp_confidence(before.as_f32() + rule.confidence_delta);
            deltas.push(ConfidenceDelta {
                assertion_id: assertion.id.clone(),
                before: Some(before),
                after: Some(after),
                delta: after.as_f32() - before.as_f32(),
                explanation: rule.explanation.clone(),
            });
            explanation_trace.push(format!(
                "rule {} changed {} by {:.2}: {}",
                rule.predicate, assertion.id, rule.confidence_delta, rule.explanation
            ));
        }
    }

    deltas.sort_by(|left, right| {
        left.assertion_id
            .cmp(&right.assertion_id)
            .then_with(|| left.explanation.cmp(&right.explanation))
    });
    deltas
}

fn clamp_confidence(value: f32) -> Confidence {
    Confidence::new(value.clamp(0.0, 1.0)).expect("clamped confidence is valid")
}

fn path_confidence(links: &[CausalLink]) -> Confidence {
    let confidence = links
        .iter()
        .map(|link| link.confidence.as_f32())
        .fold(1.0_f32, f32::min);
    Confidence::new(confidence).expect("minimum of valid confidences is valid")
}

fn causal_path_key(path: &CausalPath) -> Vec<String> {
    path.links
        .iter()
        .map(|link| link.id.as_str().to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rg_core::{
        Assertion, AssertionId, AssertionStatus, CausalLinkId, Confidence, ContextScope, Entity,
        EventId, GraphValue, PredicateId, SourceId, TimeInterval, ValidTime,
    };
    use rg_events::GraphState;

    #[test]
    fn seed_events_are_deterministic() {
        let events = seed_events();

        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0].event_id().as_str(),
            "evt-000000000000000001-entity-created"
        );
        assert_eq!(
            events[1].event_id().as_str(),
            "evt-000000000000000002-entity-created"
        );
    }

    fn link(id: &str, cause: &str, effect: &str, confidence: f32, lag: &str) -> CausalLink {
        CausalLink {
            id: CausalLinkId::new(id),
            cause_event: EventId::new(cause),
            effect_event: EventId::new(effect),
            confidence: Confidence::new(confidence).expect("valid confidence"),
            mechanism: Some(format!("{cause} changes likelihood of {effect}")),
            lag: TimeLag::new(lag).expect("valid lag"),
            counterfactual_note: Some(format!("Without {cause}, {effect} is less likely.")),
            source_ids: vec![SourceId::new("source-1")],
        }
    }

    #[test]
    fn causal_link_carries_mechanism_lag_sources_and_counterfactual_note() {
        let causal_link = link(
            "causal-1",
            "sanction_announced",
            "oil_price_increase",
            0.71,
            "P3D",
        );

        assert_eq!(causal_link.cause_event.as_str(), "sanction_announced");
        assert_eq!(causal_link.effect_event.as_str(), "oil_price_increase");
        assert_eq!(
            causal_link.mechanism.as_deref(),
            Some("sanction_announced changes likelihood of oil_price_increase")
        );
        assert_eq!(causal_link.lag.as_str(), "P3D");
        assert_eq!(causal_link.source_ids, vec![SourceId::new("source-1")]);
        assert_eq!(
            causal_link.counterfactual_note.as_deref(),
            Some("Without sanction_announced, oil_price_increase is less likely.")
        );
    }

    #[test]
    fn time_lag_rejects_empty_or_non_period_values() {
        assert_eq!(TimeLag::new(""), Err(CausalModelError::InvalidLag));
        assert_eq!(TimeLag::new("3D"), Err(CausalModelError::InvalidLag));
    }

    #[test]
    fn causal_path_query_returns_deterministic_multi_hop_chain() {
        let mut graph = CausalGraph::new();
        graph.insert_link(link(
            "causal-1",
            "sanction_announced",
            "supply_restriction_expectation",
            0.8,
            "P1D",
        ));
        graph.insert_link(link(
            "causal-2",
            "supply_restriction_expectation",
            "oil_price_increase",
            0.71,
            "P3D",
        ));
        graph.insert_link(link(
            "causal-3",
            "oil_price_increase",
            "inflation_pressure",
            0.62,
            "P14D",
        ));

        let paths = graph.causal_paths(CausalPathQuery {
            start: EventId::new("sanction_announced"),
            end: Some(EventId::new("inflation_pressure")),
            max_depth: 3,
            min_confidence: None,
        });

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].start.as_str(), "sanction_announced");
        assert_eq!(paths[0].end.as_str(), "inflation_pressure");
        assert_eq!(
            paths[0]
                .links
                .iter()
                .map(|link| link.id.as_str())
                .collect::<Vec<_>>(),
            vec!["causal-1", "causal-2", "causal-3"]
        );
        assert_eq!(
            paths[0].confidence,
            Confidence::new(0.62).expect("valid confidence")
        );
    }

    #[test]
    fn causal_path_query_respects_depth_and_confidence_filters() {
        let mut graph = CausalGraph::new();
        graph.insert_link(link("causal-1", "a", "b", 0.9, "P1D"));
        graph.insert_link(link("causal-2", "b", "c", 0.7, "P1D"));
        graph.insert_link(link("causal-3", "c", "d", 0.5, "P1D"));

        assert!(graph
            .causal_paths(CausalPathQuery {
                start: EventId::new("a"),
                end: Some(EventId::new("d")),
                max_depth: 2,
                min_confidence: None,
            })
            .is_empty());
        assert!(graph
            .causal_paths(CausalPathQuery {
                start: EventId::new("a"),
                end: Some(EventId::new("d")),
                max_depth: 3,
                min_confidence: Some(0.6),
            })
            .is_empty());
    }

    #[test]
    fn causal_path_query_avoids_cycles() {
        let mut graph = CausalGraph::new();
        graph.insert_link(link("causal-1", "a", "b", 0.9, "P1D"));
        graph.insert_link(link("causal-2", "b", "a", 0.9, "P1D"));
        graph.insert_link(link("causal-3", "b", "c", 0.9, "P1D"));

        let paths = graph.causal_paths(CausalPathQuery {
            start: EventId::new("a"),
            end: Some(EventId::new("c")),
            max_depth: 4,
            min_confidence: None,
        });

        assert_eq!(paths.len(), 1);
        assert_eq!(
            paths[0]
                .links
                .iter()
                .map(|link| link.id.as_str())
                .collect::<Vec<_>>(),
            vec!["causal-1", "causal-3"]
        );
    }

    fn entity(id: &str) -> Entity {
        Entity {
            id: EntityId::new(id),
            entity_type: EntityType::Organization,
            canonical_name: Some(id.to_owned()),
            properties: PropertyMap::default(),
            created_tx: TxTime::new(1),
        }
    }

    fn assertion(
        id: &str,
        subject: &str,
        predicate: &str,
        object: &str,
        confidence: f32,
    ) -> Assertion {
        Assertion {
            id: AssertionId::new(id),
            subject: EntityId::new(subject),
            predicate: PredicateId::new(predicate),
            object: GraphValue::Entity(EntityId::new(object)),
            valid_time: TimeInterval::new(ValidTime::new(1), None).expect("valid interval"),
            transaction_time: TimeInterval::new(TxTime::new(1), None)
                .expect("valid transaction interval"),
            confidence: Confidence::new(confidence).expect("valid confidence"),
            source_ids: vec![SourceId::new("source-1")],
            context: ContextScope::Global,
            status: AssertionStatus::Active,
        }
    }

    fn fixture_state(entities: &[&str], assertions: Vec<Assertion>) -> GraphState {
        let mut state = GraphState::new();
        for entity_id in entities {
            state
                .entities
                .insert(EntityId::new(*entity_id), entity(entity_id));
        }
        for assertion in assertions {
            state.assertions.insert(assertion.id.clone(), assertion);
        }
        state
    }

    fn supply_chain_fixture() -> GraphState {
        fixture_state(
            &[
                "company-a",
                "company-b",
                "contract-1",
                "revenue-exposure",
                "geopolitical-risk",
            ],
            vec![
                assertion("supply-a-b", "company-a", "SUPPLIES", "company-b", 0.9),
                assertion(
                    "contract-b-1",
                    "company-b",
                    "DEPENDENT_CONTRACT",
                    "contract-1",
                    0.8,
                ),
                assertion(
                    "revenue-1",
                    "contract-1",
                    "REVENUE_EXPOSURE",
                    "revenue-exposure",
                    0.7,
                ),
                assertion(
                    "risk-1",
                    "company-b",
                    "GEOPOLITICAL_RISK",
                    "geopolitical-risk",
                    0.6,
                ),
            ],
        )
    }

    fn ownership_fixture() -> GraphState {
        fixture_state(
            &["investor-a", "holding-co", "subsidiary", "asset-1"],
            vec![
                assertion("owns-1", "investor-a", "OWNS", "holding-co", 0.95),
                assertion("owns-2", "holding-co", "OWNS", "subsidiary", 0.9),
                assertion("owns-3", "subsidiary", "OWNS", "asset-1", 0.85),
            ],
        )
    }

    #[test]
    fn removing_supply_edge_finds_dependent_paths_and_rule_deltas() {
        let result = CounterfactualEngine::evaluate(
            &supply_chain_fixture(),
            CounterfactualRequest {
                valid_at: ValidTime::new(1),
                intervention: CounterfactualIntervention::RemoveAssertion(AssertionId::new(
                    "supply-a-b",
                )),
                max_depth: 3,
                rules: vec![
                    CounterfactualRule::predicate_delta(
                        "DEPENDENT_CONTRACT",
                        -0.20,
                        "dependent contract exposure",
                    ),
                    CounterfactualRule::predicate_delta(
                        "REVENUE_EXPOSURE",
                        -0.15,
                        "revenue exposure moves with disrupted supply",
                    ),
                    CounterfactualRule::predicate_delta(
                        "GEOPOLITICAL_RISK",
                        0.10,
                        "risk increases after supplier removal",
                    ),
                ],
            },
        );

        assert_eq!(
            result.changed_entities,
            entity_ids(&["company-a", "company-b"])
        );
        assert_eq!(
            result.changed_assertions,
            vec![AssertionId::new("supply-a-b")]
        );
        assert_eq!(
            result.impacted_entities,
            entity_ids(&[
                "company-a",
                "company-b",
                "contract-1",
                "geopolitical-risk",
                "revenue-exposure",
            ])
        );
        assert_eq!(
            result.impacted_assertions,
            assertion_ids(&["contract-b-1", "revenue-1", "risk-1"])
        );
        assert_eq!(
            result
                .affected_paths
                .iter()
                .map(|path| path.assertions.clone())
                .collect::<Vec<_>>(),
            vec![
                assertion_ids(&["contract-b-1"]),
                assertion_ids(&["risk-1"]),
                assertion_ids(&["contract-b-1", "revenue-1"]),
            ]
        );
        assert_eq!(
            result
                .confidence_deltas
                .iter()
                .map(|delta| (delta.assertion_id.as_str(), round_delta(delta.delta)))
                .collect::<Vec<_>>(),
            vec![
                ("contract-b-1", -0.2),
                ("revenue-1", -0.15),
                ("risk-1", 0.1),
                ("supply-a-b", -0.9),
            ]
        );
        assert_eq!(
            result.explanation_trace[0],
            "remove assertion supply-a-b at valid time 1"
        );
        assert!(result
            .explanation_trace
            .iter()
            .any(|entry| entry.contains("dependent contract exposure")));
    }

    #[test]
    fn ownership_fixture_respects_horizon_when_removing_control_edge() {
        let result = CounterfactualEngine::evaluate(
            &ownership_fixture(),
            CounterfactualRequest {
                valid_at: ValidTime::new(1),
                intervention: CounterfactualIntervention::RemoveAssertion(AssertionId::new(
                    "owns-1",
                )),
                max_depth: 1,
                rules: vec![CounterfactualRule::predicate_delta(
                    "OWNS",
                    -0.25,
                    "ownership chain control weakens",
                )],
            },
        );

        assert_eq!(
            result.changed_entities,
            entity_ids(&["holding-co", "investor-a"])
        );
        assert_eq!(
            result.impacted_entities,
            entity_ids(&["holding-co", "investor-a", "subsidiary"])
        );
        assert_eq!(result.impacted_assertions, assertion_ids(&["owns-2"]));
        assert_eq!(
            result
                .affected_paths
                .iter()
                .map(|path| (&path.start, &path.end, path.depth))
                .collect::<Vec<_>>(),
            vec![(
                &EntityId::new("holding-co"),
                &EntityId::new("subsidiary"),
                1
            )]
        );
        assert_eq!(
            result
                .confidence_deltas
                .iter()
                .map(|delta| (delta.assertion_id.as_str(), round_delta(delta.delta)))
                .collect::<Vec<_>>(),
            vec![("owns-1", -0.95), ("owns-2", -0.25)]
        );
    }

    fn entity_ids(ids: &[&str]) -> Vec<EntityId> {
        ids.iter().map(|id| EntityId::new(*id)).collect()
    }

    fn assertion_ids(ids: &[&str]) -> Vec<AssertionId> {
        ids.iter().map(|id| AssertionId::new(*id)).collect()
    }

    fn round_delta(value: f32) -> f32 {
        (value * 100.0).round() / 100.0
    }
}
