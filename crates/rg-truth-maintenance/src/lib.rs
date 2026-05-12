//! Truth maintenance primitives for Reality Graph.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use rg_core::{Assertion, AssertionId, Confidence, SourceId, TimeInterval, TxTime, ValidTime};

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

string_newtype!(AssumptionId);
string_newtype!(DerivedAssertionId);
string_newtype!(AnswerId);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DependencyNode {
    Source(SourceId),
    Assertion(AssertionId),
    Assumption(AssumptionId),
    DerivedAssertion(DerivedAssertionId),
    Answer(AnswerId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssumptionStatus {
    Active,
    Retracted,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Assumption {
    pub id: AssumptionId,
    pub statement: String,
    pub source_ids: Vec<SourceId>,
    pub confidence: Confidence,
    pub valid_time: TimeInterval<ValidTime>,
    pub transaction_time: TxTime,
    pub status: AssumptionStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DerivedAssertionStatus {
    Supported,
    Invalidated,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DerivedAssertion {
    pub id: DerivedAssertionId,
    pub assertion: Assertion,
    pub derived_from: Vec<DependencyNode>,
    pub rule: String,
    pub explanation: String,
    pub status: DerivedAssertionStatus,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnswerRecord {
    pub id: AnswerId,
    pub question: String,
    pub answer_summary: String,
    pub depends_on: Vec<DependencyNode>,
    pub generated_at: TxTime,
    pub invalidated_by: Option<DependencyNode>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RetractionReason {
    SourceInvalidated(String),
    AssertionFalse(String),
    AssumptionRejected(String),
    CorrectionApplied(String),
    ContradictionResolved(String),
}

impl RetractionReason {
    fn explanation(&self) -> &str {
        match self {
            Self::SourceInvalidated(value)
            | Self::AssertionFalse(value)
            | Self::AssumptionRejected(value)
            | Self::CorrectionApplied(value)
            | Self::ContradictionResolved(value) => value,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DependencyEdge {
    pub from: DependencyNode,
    pub to: DependencyNode,
    pub explanation: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BeliefInvalidationStep {
    pub from: DependencyNode,
    pub to: DependencyNode,
    pub explanation: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BeliefInvalidationTrace {
    pub root: DependencyNode,
    pub reason: RetractionReason,
    pub steps: Vec<BeliefInvalidationStep>,
    pub explanation: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExplanationDependencyTree {
    pub node: DependencyNode,
    pub explanation: String,
    pub children: Vec<ExplanationDependencyTree>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetractionPropagation {
    pub root: DependencyNode,
    pub invalidated_nodes: Vec<DependencyNode>,
    pub changed_beliefs: Vec<DerivedAssertionId>,
    pub invalidated_answers: Vec<AnswerId>,
    pub trace: BeliefInvalidationTrace,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DependencyGraph {
    assumptions: BTreeMap<AssumptionId, Assumption>,
    derived_assertions: BTreeMap<DerivedAssertionId, DerivedAssertion>,
    dependencies: BTreeMap<DependencyNode, BTreeMap<DependencyNode, String>>,
    reverse_dependencies: BTreeMap<DependencyNode, BTreeMap<DependencyNode, String>>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_assumption(&mut self, assumption: Assumption) {
        let node = DependencyNode::Assumption(assumption.id.clone());
        for source_id in &assumption.source_ids {
            self.add_dependency(
                DependencyNode::Source(source_id.clone()),
                node.clone(),
                "source supports assumption",
            );
        }
        self.assumptions.insert(assumption.id.clone(), assumption);
    }

    pub fn add_derived_assertion(&mut self, derived: DerivedAssertion) {
        let node = DependencyNode::DerivedAssertion(derived.id.clone());
        for source_id in &derived.assertion.source_ids {
            self.add_dependency(
                DependencyNode::Source(source_id.clone()),
                node.clone(),
                "source supports derived assertion",
            );
        }
        for dependency in &derived.derived_from {
            self.add_dependency(
                dependency.clone(),
                node.clone(),
                format!("{} derives {}", dependency_label(dependency), derived.id),
            );
        }
        self.derived_assertions.insert(derived.id.clone(), derived);
    }

    pub fn add_dependency(
        &mut self,
        from: DependencyNode,
        to: DependencyNode,
        explanation: impl Into<String>,
    ) {
        let explanation = explanation.into();
        self.dependencies
            .entry(from.clone())
            .or_default()
            .entry(to.clone())
            .or_insert_with(|| explanation.clone());
        self.reverse_dependencies
            .entry(to)
            .or_default()
            .entry(from)
            .or_insert(explanation);
    }

    pub fn assumption(&self, id: &AssumptionId) -> Option<&Assumption> {
        self.assumptions.get(id)
    }

    pub fn derived_assertion(&self, id: &DerivedAssertionId) -> Option<&DerivedAssertion> {
        self.derived_assertions.get(id)
    }

    pub fn direct_dependents(&self, node: &DependencyNode) -> Vec<DependencyNode> {
        sorted_keys(self.dependencies.get(node))
    }

    pub fn transitive_dependents(&self, node: &DependencyNode) -> Vec<DependencyNode> {
        transitive_from(node, &self.dependencies)
    }

    pub fn explanation_dependency_tree(&self, node: &DependencyNode) -> ExplanationDependencyTree {
        self.explanation_dependency_tree_inner(node, &mut BTreeSet::new())
    }

    fn explanation_dependency_tree_inner(
        &self,
        node: &DependencyNode,
        visited: &mut BTreeSet<DependencyNode>,
    ) -> ExplanationDependencyTree {
        if !visited.insert(node.clone()) {
            return ExplanationDependencyTree {
                node: node.clone(),
                explanation: "cycle suppressed in explanation tree".to_owned(),
                children: Vec::new(),
            };
        }

        let mut prerequisites = self
            .reverse_dependencies
            .get(node)
            .map(|dependencies| dependencies.iter().collect::<Vec<_>>())
            .unwrap_or_default();
        prerequisites.sort_by(|left, right| left.0.cmp(right.0));

        let children = prerequisites
            .into_iter()
            .map(|(dependency, edge_explanation)| {
                let mut child = self.explanation_dependency_tree_inner(dependency, visited);
                child.explanation = edge_explanation.clone();
                child
            })
            .collect();
        visited.remove(node);

        ExplanationDependencyTree {
            node: node.clone(),
            explanation: node_explanation(node, self),
            children,
        }
    }

    fn invalidate_node(&mut self, node: &DependencyNode, root: &DependencyNode) {
        match node {
            DependencyNode::Assumption(id) => {
                if let Some(assumption) = self.assumptions.get_mut(id) {
                    assumption.status = AssumptionStatus::Retracted;
                }
            }
            DependencyNode::DerivedAssertion(id) => {
                if let Some(derived) = self.derived_assertions.get_mut(id) {
                    derived.status = DerivedAssertionStatus::Invalidated;
                }
            }
            DependencyNode::Answer(_) => {
                let _root = root;
            }
            DependencyNode::Source(_) | DependencyNode::Assertion(_) => {}
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TruthMaintenanceSystem {
    graph: DependencyGraph,
    answers: BTreeMap<AnswerId, AnswerRecord>,
}

impl TruthMaintenanceSystem {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_assumption(&mut self, assumption: Assumption) {
        self.graph.add_assumption(assumption);
    }

    pub fn add_derived_assertion(&mut self, derived: DerivedAssertion) {
        self.graph.add_derived_assertion(derived);
    }

    pub fn record_answer(&mut self, answer: AnswerRecord) {
        let node = DependencyNode::Answer(answer.id.clone());
        for dependency in &answer.depends_on {
            self.graph.add_dependency(
                dependency.clone(),
                node.clone(),
                format!(
                    "{} supports answer {}",
                    dependency_label(dependency),
                    answer.id
                ),
            );
        }
        self.answers.insert(answer.id.clone(), answer);
    }

    pub fn assumption(&self, id: &AssumptionId) -> Option<&Assumption> {
        self.graph.assumption(id)
    }

    pub fn derived_assertion(&self, id: &DerivedAssertionId) -> Option<&DerivedAssertion> {
        self.graph.derived_assertion(id)
    }

    pub fn answer(&self, id: &AnswerId) -> Option<&AnswerRecord> {
        self.answers.get(id)
    }

    pub fn what_depends_on_source(&self, source_id: &SourceId) -> Vec<DependencyNode> {
        self.graph
            .transitive_dependents(&DependencyNode::Source(source_id.clone()))
    }

    pub fn beliefs_changed_if_assertion_false(
        &self,
        assertion_id: &AssertionId,
    ) -> Vec<DerivedAssertionId> {
        changed_beliefs_from_nodes(
            self.graph
                .transitive_dependents(&DependencyNode::Assertion(assertion_id.clone())),
        )
    }

    pub fn answers_invalidated_by_correction(
        &self,
        corrected_node: &DependencyNode,
    ) -> Vec<AnswerId> {
        invalidated_answers_from_nodes(self.graph.transitive_dependents(corrected_node))
    }

    pub fn explain_dependency_tree(&self, node: &DependencyNode) -> ExplanationDependencyTree {
        self.graph.explanation_dependency_tree(node)
    }

    pub fn propagate_retraction(
        &mut self,
        root: DependencyNode,
        reason: RetractionReason,
    ) -> RetractionPropagation {
        let invalidated_nodes = self.graph.transitive_dependents(&root);
        for node in &invalidated_nodes {
            self.graph.invalidate_node(node, &root);
            if let DependencyNode::Answer(answer_id) = node {
                if let Some(answer) = self.answers.get_mut(answer_id) {
                    answer.invalidated_by = Some(root.clone());
                }
            }
        }

        let changed_beliefs = changed_beliefs_from_nodes(invalidated_nodes.clone());
        let invalidated_answers = invalidated_answers_from_nodes(invalidated_nodes.clone());
        let trace = invalidation_trace(&root, &reason, &self.graph);

        RetractionPropagation {
            root,
            invalidated_nodes,
            changed_beliefs,
            invalidated_answers,
            trace,
        }
    }
}

fn transitive_from(
    root: &DependencyNode,
    edges: &BTreeMap<DependencyNode, BTreeMap<DependencyNode, String>>,
) -> Vec<DependencyNode> {
    let mut seen = BTreeSet::new();
    let mut ordered = Vec::new();
    let mut queue = VecDeque::new();
    for dependent in sorted_keys(edges.get(root)) {
        queue.push_back(dependent);
    }
    while let Some(node) = queue.pop_front() {
        if !seen.insert(node.clone()) {
            continue;
        }
        ordered.push(node.clone());
        for dependent in sorted_keys(edges.get(&node)) {
            queue.push_back(dependent);
        }
    }
    ordered.sort();
    ordered
}

fn sorted_keys(edges: Option<&BTreeMap<DependencyNode, String>>) -> Vec<DependencyNode> {
    edges
        .map(|edges| edges.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default()
}

fn changed_beliefs_from_nodes(nodes: Vec<DependencyNode>) -> Vec<DerivedAssertionId> {
    let mut beliefs = nodes
        .into_iter()
        .filter_map(|node| match node {
            DependencyNode::DerivedAssertion(id) => Some(id),
            DependencyNode::Source(_)
            | DependencyNode::Assertion(_)
            | DependencyNode::Assumption(_)
            | DependencyNode::Answer(_) => None,
        })
        .collect::<Vec<_>>();
    beliefs.sort();
    beliefs.dedup();
    beliefs
}

fn invalidated_answers_from_nodes(nodes: Vec<DependencyNode>) -> Vec<AnswerId> {
    let mut answers = nodes
        .into_iter()
        .filter_map(|node| match node {
            DependencyNode::Answer(id) => Some(id),
            DependencyNode::Source(_)
            | DependencyNode::Assertion(_)
            | DependencyNode::Assumption(_)
            | DependencyNode::DerivedAssertion(_) => None,
        })
        .collect::<Vec<_>>();
    answers.sort();
    answers.dedup();
    answers
}

fn invalidation_trace(
    root: &DependencyNode,
    reason: &RetractionReason,
    graph: &DependencyGraph,
) -> BeliefInvalidationTrace {
    let mut steps = Vec::new();
    let mut visited = BTreeSet::new();
    collect_trace_steps(root, graph, &mut visited, &mut steps);
    steps.sort_by(|left, right| {
        left.from
            .cmp(&right.from)
            .then_with(|| left.to.cmp(&right.to))
            .then_with(|| left.explanation.cmp(&right.explanation))
    });

    BeliefInvalidationTrace {
        root: root.clone(),
        reason: reason.clone(),
        steps,
        explanation: format!(
            "Retraction from {} propagated because {}",
            dependency_label(root),
            reason.explanation()
        ),
    }
}

fn collect_trace_steps(
    node: &DependencyNode,
    graph: &DependencyGraph,
    visited: &mut BTreeSet<DependencyNode>,
    steps: &mut Vec<BeliefInvalidationStep>,
) {
    if !visited.insert(node.clone()) {
        return;
    }
    if let Some(dependents) = graph.dependencies.get(node) {
        for (dependent, explanation) in dependents {
            steps.push(BeliefInvalidationStep {
                from: node.clone(),
                to: dependent.clone(),
                explanation: explanation.clone(),
            });
            collect_trace_steps(dependent, graph, visited, steps);
        }
    }
}

fn node_explanation(node: &DependencyNode, graph: &DependencyGraph) -> String {
    match node {
        DependencyNode::Source(source_id) => format!("source {source_id} is evidence"),
        DependencyNode::Assertion(assertion_id) => {
            format!("assertion {assertion_id} is a claimed graph assertion")
        }
        DependencyNode::Assumption(id) => graph
            .assumptions
            .get(id)
            .map(|assumption| assumption.statement.clone())
            .unwrap_or_else(|| format!("assumption {id}")),
        DependencyNode::DerivedAssertion(id) => graph
            .derived_assertions
            .get(id)
            .map(|derived| derived.explanation.clone())
            .unwrap_or_else(|| format!("derived assertion {id}")),
        DependencyNode::Answer(answer_id) => {
            format!("answer {answer_id} was generated from dependency graph evidence")
        }
    }
}

fn dependency_label(node: &DependencyNode) -> String {
    match node {
        DependencyNode::Source(id) => format!("source {id}"),
        DependencyNode::Assertion(id) => format!("assertion {id}"),
        DependencyNode::Assumption(id) => format!("assumption {id}"),
        DependencyNode::DerivedAssertion(id) => format!("derived assertion {id}"),
        DependencyNode::Answer(id) => format!("answer {id}"),
    }
}
