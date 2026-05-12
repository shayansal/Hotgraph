//! Causal and counterfactual reasoning for Reality Graph.

use std::collections::{BTreeMap, BTreeSet};

use rg_core::{
    Assertion, AssertionId, AssertionStatus, CausalLinkId, Confidence, ContextScope, EntityId,
    EventId, GraphValue, SourceId, ValidTime,
};
use rg_events::GraphState;

#[derive(Clone, Debug, PartialEq)]
pub struct CausalEvent {
    pub id: EventId,
    pub description: String,
    pub occurred_at: Option<ValidTime>,
    pub related_entities: Vec<EntityId>,
    pub related_assertions: Vec<AssertionId>,
    pub source_ids: Vec<SourceId>,
    pub context: ContextScope,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CausalLink {
    pub id: CausalLinkId,
    pub cause_event: EventId,
    pub effect_event: EventId,
    pub relation: CausalRelation,
    pub mechanism: Mechanism,
    pub confidence: Confidence,
    pub source_ids: Vec<SourceId>,
    pub context: ContextScope,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CausalRelation {
    Caused,
    Influenced,
    Enabled,
    Prevented,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mechanism {
    pub label: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CausalPathQuery {
    pub start: EventId,
    pub end: Option<EventId>,
    pub max_depth: usize,
    pub min_confidence: Option<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CausalPath {
    pub start: EventId,
    pub end: EventId,
    pub links: Vec<CausalLink>,
    pub confidence: Confidence,
    pub explanation: String,
}

impl CausalPath {
    pub fn event_ids(&self) -> Vec<EventId> {
        let mut events = vec![self.start.clone()];
        events.extend(self.links.iter().map(|link| link.effect_event.clone()));
        events
    }

    pub fn link_ids(&self) -> Vec<CausalLinkId> {
        self.links.iter().map(|link| link.id.clone()).collect()
    }

    pub fn normal_assertion_ids(&self) -> Vec<AssertionId> {
        Vec::new()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CausalGraph {
    events: BTreeMap<EventId, CausalEvent>,
    outgoing: BTreeMap<EventId, Vec<CausalLink>>,
    incoming: BTreeMap<EventId, Vec<CausalLink>>,
}

impl CausalGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_event(&mut self, event: CausalEvent) {
        self.events.insert(event.id.clone(), event);
    }

    pub fn insert_link(&mut self, link: CausalLink) {
        push_link(
            self.outgoing.entry(link.cause_event.clone()).or_default(),
            link.clone(),
        );
        push_link(
            self.incoming.entry(link.effect_event.clone()).or_default(),
            link,
        );
    }

    pub fn event(&self, event_id: &EventId) -> Option<&CausalEvent> {
        self.events.get(event_id)
    }

    pub fn downstream_paths(&self, query: CausalPathQuery) -> Vec<CausalPath> {
        if query.max_depth == 0 {
            return Vec::new();
        }

        let mut paths = Vec::new();
        let mut path = Vec::new();
        let mut visited = BTreeSet::from([query.start.clone()]);
        self.walk_downstream(&query.start, &query, &mut visited, &mut path, &mut paths);
        sort_paths(&mut paths);
        paths
    }

    pub fn upstream_causes(&self, event: EventId, max_depth: usize) -> Vec<CausalPath> {
        if max_depth == 0 {
            return Vec::new();
        }

        let mut paths = Vec::new();
        let mut path = Vec::new();
        let mut visited = BTreeSet::from([event.clone()]);
        self.walk_upstream(
            &event,
            &event,
            max_depth,
            &mut visited,
            &mut path,
            &mut paths,
        );
        sort_paths(&mut paths);
        paths
    }

    fn walk_downstream(
        &self,
        current: &EventId,
        query: &CausalPathQuery,
        visited: &mut BTreeSet<EventId>,
        path: &mut Vec<CausalLink>,
        paths: &mut Vec<CausalPath>,
    ) {
        if path.len() >= query.max_depth {
            return;
        }
        let Some(links) = self.outgoing.get(current) else {
            return;
        };

        for link in links {
            if visited.contains(&link.effect_event) {
                continue;
            }
            if query
                .min_confidence
                .is_some_and(|minimum| link.confidence.as_f32() < minimum)
            {
                continue;
            }

            path.push(link.clone());
            visited.insert(link.effect_event.clone());
            let reached_end = query
                .end
                .as_ref()
                .is_some_and(|end| end == &link.effect_event);
            if reached_end || query.end.is_none() {
                paths.push(build_path(
                    query.start.clone(),
                    link.effect_event.clone(),
                    path,
                ));
            }
            if !reached_end {
                self.walk_downstream(&link.effect_event, query, visited, path, paths);
            }
            visited.remove(&link.effect_event);
            path.pop();
        }
    }

    fn walk_upstream(
        &self,
        current: &EventId,
        target: &EventId,
        max_depth: usize,
        visited: &mut BTreeSet<EventId>,
        path: &mut Vec<CausalLink>,
        paths: &mut Vec<CausalPath>,
    ) {
        if path.len() >= max_depth {
            return;
        }
        let Some(links) = self.incoming.get(current) else {
            return;
        };

        for link in links {
            if visited.contains(&link.cause_event) {
                continue;
            }

            path.insert(0, link.clone());
            visited.insert(link.cause_event.clone());
            paths.push(build_path(link.cause_event.clone(), target.clone(), path));
            self.walk_upstream(&link.cause_event, target, max_depth, visited, path, paths);
            visited.remove(&link.cause_event);
            path.remove(0);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Intervention {
    RemoveEvent(EventId),
    RemoveAssertion(AssertionId),
    AddEvent(CausalEvent),
    DisableCausalLink(CausalLinkId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct CounterfactualScenario {
    pub intervention: Intervention,
    pub valid_at: ValidTime,
    pub max_depth: usize,
    pub assumptions: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DependencyCone {
    pub root_event: Option<EventId>,
    pub downstream_events: Vec<EventId>,
    pub upstream_events: Vec<EventId>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ImpactedCausalPath {
    Causal {
        event_ids: Vec<EventId>,
        link_ids: Vec<CausalLinkId>,
        confidence: Confidence,
        explanation: String,
    },
    NormalRelationshipBlastRadius {
        assertion_ids: Vec<AssertionId>,
        entity_ids: Vec<EntityId>,
        explanation: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImpactTrace {
    pub affected_entities: Vec<EntityId>,
    pub affected_assertions: Vec<AssertionId>,
    pub affected_events: Vec<EventId>,
    pub impact_paths: Vec<ImpactedCausalPath>,
    pub dependency_cone: DependencyCone,
    pub propagated_confidence: Confidence,
    pub assumptions: Vec<String>,
    pub uncertainty: String,
    pub explanation_trace: Vec<String>,
    pub simulation_not_fact: bool,
}

pub struct CounterfactualEngine<'a> {
    graph: &'a CausalGraph,
    state: &'a GraphState,
}

impl<'a> CounterfactualEngine<'a> {
    pub fn new(graph: &'a CausalGraph, state: &'a GraphState) -> Self {
        Self { graph, state }
    }

    pub fn simulate(&self, scenario: CounterfactualScenario) -> ImpactTrace {
        match &scenario.intervention {
            Intervention::RemoveEvent(event_id) => self.remove_event(event_id, &scenario),
            Intervention::RemoveAssertion(assertion_id) => {
                self.remove_assertion(assertion_id, &scenario)
            }
            Intervention::AddEvent(event) => self.add_event(event, &scenario),
            Intervention::DisableCausalLink(link_id) => self.disable_link(link_id, &scenario),
        }
    }

    fn remove_event(&self, event_id: &EventId, scenario: &CounterfactualScenario) -> ImpactTrace {
        let paths = self.graph.downstream_paths(CausalPathQuery {
            start: event_id.clone(),
            end: None,
            max_depth: scenario.max_depth,
            min_confidence: None,
        });
        let upstream = self
            .graph
            .upstream_causes(event_id.clone(), scenario.max_depth)
            .into_iter()
            .map(|path| path.start)
            .collect::<BTreeSet<_>>();

        let mut affected_events = BTreeSet::from([event_id.clone()]);
        for path in &paths {
            affected_events.insert(path.end.clone());
        }

        let (affected_entities, affected_assertions) =
            collect_causal_impacts(self.graph, &affected_events);
        let impact_paths = paths
            .iter()
            .map(|path| ImpactedCausalPath::Causal {
                event_ids: path.event_ids(),
                link_ids: path.link_ids(),
                confidence: path.confidence,
                explanation: path.explanation.clone(),
            })
            .collect::<Vec<_>>();
        let propagated_confidence = paths
            .last()
            .map(|path| path.confidence)
            .unwrap_or_else(one_confidence);

        let mut trace = base_trace(&scenario.assumptions);
        trace.push(format!(
            "intervention removes event {}; downstream effects are simulated through causal links only",
            event_id
        ));
        for path in &paths {
            trace.push(path.explanation.clone());
        }

        ImpactTrace {
            affected_entities,
            affected_assertions,
            affected_events: affected_events.into_iter().collect(),
            impact_paths,
            dependency_cone: DependencyCone {
                root_event: Some(event_id.clone()),
                downstream_events: downstream_events(event_id, &paths),
                upstream_events: upstream.into_iter().collect(),
            },
            propagated_confidence,
            assumptions: scenario.assumptions.clone(),
            uncertainty:
                "counterfactual simulation with propagated causal uncertainty; not observed fact"
                    .to_owned(),
            explanation_trace: trace,
            simulation_not_fact: true,
        }
    }

    fn remove_assertion(
        &self,
        assertion_id: &AssertionId,
        scenario: &CounterfactualScenario,
    ) -> ImpactTrace {
        let mut affected_entities = BTreeSet::new();
        let mut affected_assertions = BTreeSet::new();
        if let Some(assertion) = self.state.assertions.get(assertion_id) {
            affected_assertions.insert(assertion.id.clone());
            collect_assertion_entities(assertion, &mut affected_entities);
        }

        let path = ImpactedCausalPath::NormalRelationshipBlastRadius {
            assertion_ids: affected_assertions.iter().cloned().collect(),
            entity_ids: affected_entities.iter().cloned().collect(),
            explanation: format!(
                "normal relationship blast radius for {}; this is not a causal path",
                assertion_id
            ),
        };
        let mut trace = base_trace(&scenario.assumptions);
        trace.push(format!(
            "intervention removes normal assertion {}; blast radius is graph-neighborhood impact, not causal proof",
            assertion_id
        ));

        ImpactTrace {
            affected_entities: affected_entities.into_iter().collect(),
            affected_assertions: affected_assertions.into_iter().collect(),
            affected_events: Vec::new(),
            impact_paths: vec![path],
            dependency_cone: DependencyCone {
                root_event: None,
                downstream_events: Vec::new(),
                upstream_events: Vec::new(),
            },
            propagated_confidence: one_confidence(),
            assumptions: scenario.assumptions.clone(),
            uncertainty: "relationship-removal simulation; results are hypothetical and not facts"
                .to_owned(),
            explanation_trace: trace,
            simulation_not_fact: true,
        }
    }

    fn add_event(&self, event: &CausalEvent, scenario: &CounterfactualScenario) -> ImpactTrace {
        let mut affected_entities = sorted(event.related_entities.iter().cloned());
        affected_entities.dedup();
        let mut affected_assertions = sorted(event.related_assertions.iter().cloned());
        affected_assertions.dedup();
        let mut trace = base_trace(&scenario.assumptions);
        trace.push(format!(
            "intervention adds hypothetical event {}; no source-of-truth event is appended",
            event.id
        ));

        ImpactTrace {
            affected_entities,
            affected_assertions,
            affected_events: vec![event.id.clone()],
            impact_paths: Vec::new(),
            dependency_cone: DependencyCone {
                root_event: Some(event.id.clone()),
                downstream_events: Vec::new(),
                upstream_events: Vec::new(),
            },
            propagated_confidence: one_confidence(),
            assumptions: scenario.assumptions.clone(),
            uncertainty: "hypothetical event insertion simulation, not fact".to_owned(),
            explanation_trace: trace,
            simulation_not_fact: true,
        }
    }

    fn disable_link(
        &self,
        link_id: &CausalLinkId,
        scenario: &CounterfactualScenario,
    ) -> ImpactTrace {
        let mut trace = base_trace(&scenario.assumptions);
        trace.push(format!(
            "intervention disables causal link {}; downstream impact depends on alternate paths",
            link_id
        ));

        ImpactTrace {
            affected_entities: Vec::new(),
            affected_assertions: Vec::new(),
            affected_events: Vec::new(),
            impact_paths: Vec::new(),
            dependency_cone: DependencyCone {
                root_event: None,
                downstream_events: Vec::new(),
                upstream_events: Vec::new(),
            },
            propagated_confidence: one_confidence(),
            assumptions: scenario.assumptions.clone(),
            uncertainty: "causal-link intervention simulation, not fact".to_owned(),
            explanation_trace: trace,
            simulation_not_fact: true,
        }
    }
}

fn push_link(links: &mut Vec<CausalLink>, link: CausalLink) {
    links.push(link);
    links.sort_by(|left, right| left.id.cmp(&right.id));
    links.dedup_by(|left, right| left.id == right.id);
}

fn build_path(start: EventId, end: EventId, links: &[CausalLink]) -> CausalPath {
    let confidence = propagated_confidence(links);
    let explanation = format!(
        "causal path, not a normal relationship path: {} with confidence {:.2}",
        links
            .iter()
            .map(|link| format!(
                "{} --{:?}/{}--> {}",
                link.cause_event, link.relation, link.mechanism.label, link.effect_event
            ))
            .collect::<Vec<_>>()
            .join(" | "),
        confidence.as_f32()
    );
    CausalPath {
        start,
        end,
        links: links.to_vec(),
        confidence,
        explanation,
    }
}

fn propagated_confidence(links: &[CausalLink]) -> Confidence {
    let confidence = links
        .iter()
        .map(|link| link.confidence.as_f32())
        .product::<f32>();
    Confidence::new(round_two(confidence)).expect("product of confidence scores is valid")
}

fn sort_paths(paths: &mut [CausalPath]) {
    paths.sort_by(|left, right| {
        left.links
            .len()
            .cmp(&right.links.len())
            .then_with(|| left.link_ids().cmp(&right.link_ids()))
            .then_with(|| left.start.cmp(&right.start))
            .then_with(|| left.end.cmp(&right.end))
    });
}

fn collect_causal_impacts(
    graph: &CausalGraph,
    affected_events: &BTreeSet<EventId>,
) -> (Vec<EntityId>, Vec<AssertionId>) {
    let mut entities = BTreeSet::new();
    let mut assertions = BTreeSet::new();
    for event_id in affected_events {
        let Some(event) = graph.event(event_id) else {
            continue;
        };
        entities.extend(event.related_entities.iter().cloned());
        assertions.extend(event.related_assertions.iter().cloned());
    }
    (
        entities.into_iter().collect(),
        assertions.into_iter().collect(),
    )
}

fn downstream_events(root: &EventId, paths: &[CausalPath]) -> Vec<EventId> {
    let mut events = Vec::new();
    let mut seen = BTreeSet::new();
    for path in paths {
        for event_id in path.event_ids() {
            if &event_id != root && seen.insert(event_id.clone()) {
                events.push(event_id);
            }
        }
    }
    events
}

fn collect_assertion_entities(assertion: &Assertion, entities: &mut BTreeSet<EntityId>) {
    if assertion.status != AssertionStatus::Active {
        return;
    }
    entities.insert(assertion.subject.clone());
    if let GraphValue::Entity(entity_id) = &assertion.object {
        entities.insert(entity_id.clone());
    }
}

fn base_trace(assumptions: &[String]) -> Vec<String> {
    let mut trace = vec![
        "Counterfactual output is simulation, not fact; it does not assert that reality changed."
            .to_owned(),
    ];
    if !assumptions.is_empty() {
        trace.push(format!("assumptions: {}", assumptions.join("; ")));
    }
    trace
}

fn one_confidence() -> Confidence {
    Confidence::new(1.0).expect("one is valid confidence")
}

fn round_two(value: f32) -> f32 {
    (value * 100.0).round() / 100.0
}

fn sorted<T: Ord>(values: impl Iterator<Item = T>) -> Vec<T> {
    let mut values = values.collect::<Vec<_>>();
    values.sort();
    values
}
