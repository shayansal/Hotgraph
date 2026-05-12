//! Reality Graph Query Language (RGQL).

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;

use rg_ai::{EvidencePack, EvidencePackGenerator, EvidencePackRequest};
use rg_causal::{
    CausalGraph, CausalPath, CounterfactualEngine, CounterfactualScenario, ImpactTrace,
    Intervention,
};
use rg_core::{
    Assertion, AssertionId, Entity, EntityId, EventId, GraphValue, PredicateId, TxTime, ValidTime,
};
use rg_index::{Contradiction, TemporalIndex};
use rg_query::{
    EntityPattern, GraphQuery, ObjectPattern, PathQuery, PredicatePattern, QueryEngine, QueryResult,
};
use rg_retrieval_compiler::{
    QueryIntent, RetrievalBudget, RetrievalOperator, RetrievalPlan, RetrievalTrace,
    RetrievalTraceStep,
};
use rg_storage::InMemoryStorage;

#[derive(Clone, Debug, PartialEq)]
pub enum RgqlStatement {
    Find {
        entity: EntitySelector,
        predicate: Option<PredicateId>,
        object: Option<EntitySelector>,
        valid_at: Option<i64>,
        known_at: Option<i64>,
        with_evidence: bool,
        min_confidence: Option<f32>,
        contradictions: bool,
        limit: Option<usize>,
    },
    Path {
        from: EntitySelector,
        to: Option<EntitySelector>,
        via: Vec<PredicateId>,
        valid_at: Option<i64>,
        valid_during: Option<(i64, i64)>,
        min_confidence: Option<f32>,
        max_depth: usize,
        returns: Vec<String>,
    },
    Causes {
        event: EventId,
        within: Option<DurationLiteral>,
        min_confidence: Option<f32>,
        max_depth: usize,
        returns: Vec<String>,
    },
    Contradictions {
        entity: Option<EntitySelector>,
        valid_at: Option<i64>,
        known_at: Option<i64>,
        returns: Vec<String>,
    },
    Counterfactual {
        intervention: CounterfactualIntervention,
        valid_at: Option<i64>,
        max_depth: usize,
        returns: Vec<String>,
    },
}

impl RgqlStatement {
    pub fn plan(&self) -> RgqlPlan {
        let intent = self.intent();
        let operators = self.operators();
        RgqlPlan {
            statement: self.clone(),
            retrieval_plan: RetrievalPlan {
                intent,
                operators,
                budget: RetrievalBudget::default(),
            },
        }
    }

    pub fn explain(&self) -> QueryExplain {
        let plan = self.plan();
        let trace = RetrievalTrace {
            steps: plan
                .retrieval_plan
                .operators
                .iter()
                .map(|operator| RetrievalTraceStep {
                    operator: *operator,
                    reason: self.operator_reason(*operator),
                })
                .collect(),
        };
        QueryExplain {
            estimated_cost: self.estimate_cost(),
            plan,
            trace,
        }
    }

    pub fn estimate_cost(&self) -> QueryCostEstimate {
        let operators = self.operators();
        let temporal_filters = usize::from(self.has_temporal_clause());
        let graph_expansions = operators
            .iter()
            .filter(|operator| {
                matches!(
                    operator,
                    RetrievalOperator::GraphExpansion
                        | RetrievalOperator::PathSearch
                        | RetrievalOperator::CausalExpansion
                )
            })
            .count();
        let base_rows = match self {
            Self::Find { limit, .. } => limit.unwrap_or(16),
            Self::Path { max_depth, .. } => max_depth.saturating_mul(4).max(1),
            Self::Causes { max_depth, .. } => max_depth.saturating_mul(3).max(1),
            Self::Contradictions { .. } => 8,
            Self::Counterfactual { max_depth, .. } => max_depth.saturating_mul(5).max(1),
        };
        QueryCostEstimate {
            estimated_rows: base_rows,
            estimated_cost_units: round_two(
                1.0 + operators.len() as f32 * 0.35 + graph_expansions as f32 * 0.65,
            ),
            temporal_filters,
            graph_expansions,
        }
    }

    fn intent(&self) -> QueryIntent {
        match self {
            Self::Find {
                contradictions: true,
                ..
            }
            | Self::Contradictions { .. } => QueryIntent::ContradictoryEvidence,
            Self::Find {
                valid_at: Some(_), ..
            }
            | Self::Find {
                known_at: Some(_), ..
            } => QueryIntent::Historical,
            Self::Find {
                predicate: Some(_), ..
            } => QueryIntent::Relationship,
            Self::Find { .. } => QueryIntent::SimpleFact,
            Self::Path { .. } => QueryIntent::MultiHop,
            Self::Causes { .. } | Self::Counterfactual { .. } => QueryIntent::Causal,
        }
    }

    fn operators(&self) -> Vec<RetrievalOperator> {
        let mut operators = match self {
            Self::Find {
                with_evidence,
                contradictions,
                ..
            } => {
                let mut operators = vec![RetrievalOperator::KeywordSearch];
                if self.has_temporal_clause() {
                    operators.push(RetrievalOperator::TemporalFilter);
                }
                operators.push(RetrievalOperator::GraphExpansion);
                if *contradictions {
                    operators.push(RetrievalOperator::ContradictionCheck);
                }
                operators.push(RetrievalOperator::Rerank);
                operators.push(RetrievalOperator::Compress);
                if *with_evidence {
                    operators.push(RetrievalOperator::Cite);
                }
                operators
            }
            Self::Path { .. } => {
                let mut operators = vec![RetrievalOperator::PathSearch];
                if self.has_temporal_clause() {
                    operators.push(RetrievalOperator::TemporalFilter);
                }
                operators.extend([
                    RetrievalOperator::GraphExpansion,
                    RetrievalOperator::Rerank,
                    RetrievalOperator::Compress,
                    RetrievalOperator::Cite,
                ]);
                operators
            }
            Self::Causes { .. } => vec![
                RetrievalOperator::CausalExpansion,
                RetrievalOperator::TemporalFilter,
                RetrievalOperator::Rerank,
                RetrievalOperator::Compress,
                RetrievalOperator::Cite,
            ],
            Self::Contradictions { .. } => vec![
                RetrievalOperator::TemporalFilter,
                RetrievalOperator::GraphExpansion,
                RetrievalOperator::ContradictionCheck,
                RetrievalOperator::Compress,
                RetrievalOperator::Cite,
            ],
            Self::Counterfactual { .. } => vec![
                RetrievalOperator::CausalExpansion,
                RetrievalOperator::GraphExpansion,
                RetrievalOperator::Rerank,
                RetrievalOperator::Cite,
            ],
        };
        operators.dedup();
        operators
    }

    fn has_temporal_clause(&self) -> bool {
        match self {
            Self::Find {
                valid_at, known_at, ..
            } => valid_at.is_some() || known_at.is_some(),
            Self::Path {
                valid_at,
                valid_during,
                ..
            } => valid_at.is_some() || valid_during.is_some(),
            Self::Causes { within, .. } => within.is_some(),
            Self::Contradictions {
                valid_at, known_at, ..
            } => valid_at.is_some() || known_at.is_some(),
            Self::Counterfactual { valid_at, .. } => valid_at.is_some(),
        }
    }

    fn operator_reason(&self, operator: RetrievalOperator) -> String {
        match operator {
            RetrievalOperator::TemporalFilter => match self {
                Self::Find {
                    valid_at: Some(_),
                    known_at: Some(_),
                    ..
                } => "selected because RGQL specified VALID_AT and KNOWN_AT".to_owned(),
                Self::Find {
                    valid_at: Some(_), ..
                }
                | Self::Path {
                    valid_at: Some(_), ..
                }
                | Self::Contradictions {
                    valid_at: Some(_), ..
                }
                | Self::Counterfactual {
                    valid_at: Some(_), ..
                } => "selected because RGQL specified VALID_AT".to_owned(),
                Self::Path {
                    valid_during: Some(_),
                    ..
                } => "selected because RGQL specified VALID_DURING".to_owned(),
                Self::Causes {
                    within: Some(duration),
                    ..
                } => format!("selected because RGQL specified WITHIN {}", duration.raw),
                _ => "selected for temporal constraints".to_owned(),
            },
            RetrievalOperator::GraphExpansion => {
                "selected to resolve typed entities and assertion relationships".to_owned()
            }
            RetrievalOperator::PathSearch => {
                "selected because RGQL PATH requires multi-hop traversal".to_owned()
            }
            RetrievalOperator::ContradictionCheck => {
                "selected because RGQL requested CONTRADICTIONS".to_owned()
            }
            RetrievalOperator::CausalExpansion => {
                "selected because RGQL requested CAUSES or COUNTERFACTUAL reasoning".to_owned()
            }
            RetrievalOperator::Cite => {
                "selected because RGQL requested evidence/source-backed output".to_owned()
            }
            RetrievalOperator::KeywordSearch => {
                "selected to anchor typed names, predicates, and literal query terms".to_owned()
            }
            RetrievalOperator::VectorSearch => {
                "selected for semantic candidate retrieval".to_owned()
            }
            RetrievalOperator::CommunitySearch => {
                "selected for broad temporal community context".to_owned()
            }
            RetrievalOperator::Rerank => {
                "selected to order compact evidence by relevance and confidence".to_owned()
            }
            RetrievalOperator::Compress => {
                "selected to keep the AI-facing context pack compact".to_owned()
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntitySelector {
    Type { entity_type: String },
    TypedName { entity_type: String, name: String },
    Id(EntityId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathSelector {
    pub entity_type: Option<String>,
    pub name: Option<String>,
    pub id: Option<EntityId>,
}

impl From<EntitySelector> for PathSelector {
    fn from(selector: EntitySelector) -> Self {
        match selector {
            EntitySelector::Type { entity_type } => Self {
                entity_type: Some(entity_type),
                name: None,
                id: None,
            },
            EntitySelector::TypedName { entity_type, name } => Self {
                entity_type: Some(entity_type),
                name: Some(name),
                id: None,
            },
            EntitySelector::Id(id) => Self {
                entity_type: None,
                name: None,
                id: Some(id),
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurationLiteral {
    pub raw: String,
    pub amount: u64,
    pub unit: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CounterfactualIntervention {
    RemoveAssertion(AssertionId),
    RemoveEvent(EventId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RgqlPlan {
    pub statement: RgqlStatement,
    pub retrieval_plan: RetrievalPlan,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueryExplain {
    pub plan: RgqlPlan,
    pub trace: RetrievalTrace,
    pub estimated_cost: QueryCostEstimate,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueryCostEstimate {
    pub estimated_rows: usize,
    pub estimated_cost_units: f32,
    pub temporal_filters: usize,
    pub graph_expansions: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RgqlParseError {
    pub position: usize,
    pub message: String,
}

impl fmt::Display for RgqlParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "RGQL parse error at {}: {}",
            self.position, self.message
        )
    }
}

impl Error for RgqlParseError {}

pub struct RgqlParser;

impl RgqlParser {
    pub fn parse(input: &str) -> Result<RgqlStatement, RgqlParseError> {
        let tokens = Lexer::new(input).lex()?;
        Parser::new(tokens, input.len()).parse_statement()
    }
}

pub fn compile_natural_language(question: &str) -> Result<String, RgqlParseError> {
    let normalized = question.to_ascii_lowercase();
    let date = first_iso_date(question);
    let evidence = normalized.contains("evidence") || normalized.contains("source");
    if normalized.contains("work") || normalized.contains("employ") {
        let subject = extract_between(question, "did ", " work")
            .or_else(|| extract_between(question, "did ", " employ"))
            .unwrap_or_else(|| "Entity".to_owned());
        let mut query = format!(r#"MATCH Entity("{subject}") WHERE worked_at"#);
        if let Some(date) = date {
            query.push_str(&format!(r#" VALID_AT "{date}""#));
        }
        if evidence {
            query.push_str(" WITH EVIDENCE");
        }
        return Ok(query);
    }

    if normalized.contains("path") || normalized.contains("connected") {
        return Ok(
            r#"PATH FROM Entity("unknown") VIA related_to RETURN paths, evidence"#.to_owned(),
        );
    }

    if normalized.contains("cause") || normalized.contains("why") {
        return Ok(
            r#"CAUSES OF Event("unknown") RETURN causal_paths, mechanisms, sources"#.to_owned(),
        );
    }

    Err(RgqlParseError {
        position: 0,
        message: "could not compile natural-language query into RGQL".to_owned(),
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExecutorContext {
    storage: InMemoryStorage,
    causal_graph: CausalGraph,
}

impl ExecutorContext {
    pub fn new(storage: InMemoryStorage, causal_graph: CausalGraph) -> Self {
        Self {
            storage,
            causal_graph,
        }
    }

    pub fn storage(&self) -> &InMemoryStorage {
        &self.storage
    }

    pub fn causal_graph(&self) -> &CausalGraph {
        &self.causal_graph
    }
}

pub struct RgqlExecutor<'a> {
    context: &'a ExecutorContext,
}

impl<'a> RgqlExecutor<'a> {
    pub fn new(context: &'a ExecutorContext) -> Self {
        Self { context }
    }

    pub fn execute(
        &self,
        statement: &RgqlStatement,
    ) -> Result<RgqlExecutionResult, RgqlExecutionError> {
        match statement {
            RgqlStatement::Find { .. } => self.execute_find(statement),
            RgqlStatement::Path { .. } => self.execute_path(statement),
            RgqlStatement::Causes {
                event,
                min_confidence,
                max_depth,
                ..
            } => {
                let mut paths = self
                    .context
                    .causal_graph
                    .upstream_causes(event.clone(), *max_depth);
                if let Some(minimum) = min_confidence {
                    paths.retain(|path| path.confidence.as_f32() >= *minimum);
                }
                Ok(RgqlExecutionResult::Causal { paths })
            }
            RgqlStatement::Contradictions {
                entity, valid_at, ..
            } => Ok(RgqlExecutionResult::Contradictions {
                contradictions: self.detect_contradictions(entity.as_ref(), *valid_at),
                evidence_pack: None,
            }),
            RgqlStatement::Counterfactual {
                intervention,
                valid_at,
                max_depth,
                ..
            } => {
                let intervention = match intervention {
                    CounterfactualIntervention::RemoveAssertion(assertion_id) => {
                        Intervention::RemoveAssertion(assertion_id.clone())
                    }
                    CounterfactualIntervention::RemoveEvent(event_id) => {
                        Intervention::RemoveEvent(event_id.clone())
                    }
                };
                let trace = CounterfactualEngine::new(
                    &self.context.causal_graph,
                    self.context.storage.graph_state(),
                )
                .simulate(CounterfactualScenario {
                    intervention,
                    valid_at: ValidTime::new(valid_at.unwrap_or(0)),
                    max_depth: *max_depth,
                    assumptions: vec![
                        "RGQL COUNTERFACTUAL output is simulation, not asserted fact.".to_owned(),
                    ],
                });
                Ok(RgqlExecutionResult::Counterfactual { trace })
            }
        }
    }

    fn execute_find(
        &self,
        statement: &RgqlStatement,
    ) -> Result<RgqlExecutionResult, RgqlExecutionError> {
        let RgqlStatement::Find {
            entity,
            predicate,
            object,
            valid_at,
            known_at,
            with_evidence,
            min_confidence,
            limit,
            ..
        } = statement
        else {
            unreachable!("caller only passes find statements");
        };

        let subject = match entity {
            EntitySelector::Type { .. } => None,
            _ => Some(EntityPattern::Id(self.resolve_entity_id(entity)?)),
        };
        let object = object
            .as_ref()
            .map(|selector| self.resolve_entity_id(selector).map(ObjectPattern::Entity))
            .transpose()?;
        let graph_query = GraphQuery {
            subject,
            predicate: predicate.clone().map(PredicatePattern::Id),
            object,
            valid_at: *valid_at,
            known_at: *known_at,
            context: None,
            min_confidence: *min_confidence,
            limit: *limit,
        };
        let engine = QueryEngine::from_storage(self.context.storage.clone());
        let assertions = engine.execute_graph(graph_query.clone());
        let evidence_pack = if *with_evidence {
            Some(
                EvidencePackGenerator::new(&self.context.storage).generate(EvidencePackRequest {
                    query: statement_to_query_text(statement),
                    graph_query,
                    path_query: None,
                    generated_at: generated_at(&self.context.storage),
                }),
            )
        } else {
            None
        };
        Ok(RgqlExecutionResult::Graph {
            assertions,
            evidence_pack,
        })
    }

    fn execute_path(
        &self,
        statement: &RgqlStatement,
    ) -> Result<RgqlExecutionResult, RgqlExecutionError> {
        let RgqlStatement::Path {
            from,
            to,
            via,
            valid_at,
            valid_during,
            min_confidence,
            max_depth,
            ..
        } = statement
        else {
            unreachable!("caller only passes path statements");
        };
        let start = self.resolve_entity_id(from)?;
        let end = to
            .as_ref()
            .map(|selector| self.resolve_entity_id(selector))
            .transpose()?;
        let mut paths = Vec::new();
        let mut visited = BTreeSet::from([start.clone()]);
        let mut hops = Vec::new();
        self.walk_paths(PathWalk {
            current: start.clone(),
            end: end.clone(),
            start,
            via,
            valid_at: *valid_at,
            valid_during: *valid_during,
            min_confidence: *min_confidence,
            max_depth: *max_depth,
            visited: &mut visited,
            hops: &mut hops,
            paths: &mut paths,
        });
        paths.sort_by(|left, right| {
            left.hops
                .iter()
                .map(|hop| hop.assertion_id.as_str())
                .cmp(right.hops.iter().map(|hop| hop.assertion_id.as_str()))
        });

        let evidence_pack = if statement_returns(statement, "evidence") {
            Some(
                EvidencePackGenerator::new(&self.context.storage).generate(EvidencePackRequest {
                    query: statement_to_query_text(statement),
                    graph_query: GraphQuery {
                        subject: Some(EntityPattern::Id(self.resolve_entity_id(from)?)),
                        predicate: None,
                        object: None,
                        valid_at: *valid_at,
                        known_at: None,
                        context: None,
                        min_confidence: *min_confidence,
                        limit: None,
                    },
                    path_query: end.map(|end| PathQuery {
                        start: self.resolve_entity_id(from).expect("already resolved"),
                        end: Some(end),
                        predicates: Vec::new(),
                        valid_at: *valid_at,
                        max_depth: *max_depth,
                        min_confidence: *min_confidence,
                    }),
                    generated_at: generated_at(&self.context.storage),
                }),
            )
        } else {
            None
        };

        Ok(RgqlExecutionResult::Paths {
            paths,
            evidence_pack,
        })
    }

    fn walk_paths(&self, walk: PathWalk<'_>) {
        if walk.hops.len() >= walk.max_depth {
            return;
        }
        let mut candidates = self
            .context
            .storage
            .assertions_by_subject(&walk.current)
            .into_iter()
            .filter(|assertion| {
                edge_matches_path(
                    assertion,
                    walk.via,
                    walk.valid_at,
                    walk.valid_during,
                    walk.min_confidence,
                )
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| left.id.cmp(&right.id));

        for assertion in candidates {
            let GraphValue::Entity(next) = &assertion.object else {
                continue;
            };
            if walk.visited.contains(next) {
                continue;
            }
            walk.hops.push(QueryResult::from_assertion(assertion));
            if walk.end.as_ref().map_or(true, |end| end == next) {
                walk.paths.push(RgqlPathResult {
                    start: walk.start.clone(),
                    end: next.clone(),
                    hops: walk.hops.clone(),
                });
            }
            walk.visited.insert(next.clone());
            self.walk_paths(PathWalk {
                current: next.clone(),
                end: walk.end.clone(),
                start: walk.start.clone(),
                via: walk.via,
                valid_at: walk.valid_at,
                valid_during: walk.valid_during,
                min_confidence: walk.min_confidence,
                max_depth: walk.max_depth,
                visited: walk.visited,
                hops: walk.hops,
                paths: walk.paths,
            });
            walk.visited.remove(next);
            walk.hops.pop();
        }
    }

    fn resolve_entity_id(&self, selector: &EntitySelector) -> Result<EntityId, RgqlExecutionError> {
        match selector {
            EntitySelector::Id(id) => Ok(id.clone()),
            EntitySelector::Type { entity_type } => self
                .context
                .storage
                .graph_state()
                .entities
                .values()
                .find(|entity| entity_type_matches(entity, entity_type))
                .map(|entity| entity.id.clone())
                .ok_or_else(|| RgqlExecutionError::UnknownEntity(entity_type.clone())),
            EntitySelector::TypedName { entity_type, name } => self
                .context
                .storage
                .graph_state()
                .entities
                .values()
                .find(|entity| {
                    (entity_type.eq_ignore_ascii_case("Entity")
                        || entity_type_matches(entity, entity_type))
                        && entity_name_matches(entity, name)
                })
                .map(|entity| entity.id.clone())
                .or_else(|| {
                    self.context
                        .storage
                        .entity(&EntityId::new(name))
                        .map(|entity| entity.id.clone())
                })
                .ok_or_else(|| RgqlExecutionError::UnknownEntity(name.clone())),
        }
    }

    fn detect_contradictions(
        &self,
        entity: Option<&EntitySelector>,
        valid_at: Option<i64>,
    ) -> Vec<Contradiction> {
        let entity_id = entity.and_then(|selector| self.resolve_entity_id(selector).ok());
        let mut index = TemporalIndex::new();
        for assertion in self.context.storage.graph_state().assertions.values() {
            if valid_at.map_or(true, |instant| {
                assertion.valid_time.contains(ValidTime::new(instant))
            }) {
                index.insert_assertion(assertion.clone());
            }
        }
        let mut contradictions = index.contradictions();
        if let Some(entity_id) = entity_id {
            contradictions.retain(|contradiction| {
                [&contradiction.assertion_a, &contradiction.assertion_b]
                    .iter()
                    .filter_map(|id| self.context.storage.assertion(id))
                    .any(|assertion| {
                        assertion.subject == entity_id
                            || matches!(&assertion.object, GraphValue::Entity(id) if id == &entity_id)
                    })
            });
        }
        contradictions
    }
}

struct PathWalk<'a> {
    current: EntityId,
    end: Option<EntityId>,
    start: EntityId,
    via: &'a [PredicateId],
    valid_at: Option<i64>,
    valid_during: Option<(i64, i64)>,
    min_confidence: Option<f32>,
    max_depth: usize,
    visited: &'a mut BTreeSet<EntityId>,
    hops: &'a mut Vec<QueryResult>,
    paths: &'a mut Vec<RgqlPathResult>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RgqlExecutionResult {
    Graph {
        assertions: Vec<QueryResult>,
        evidence_pack: Option<EvidencePack>,
    },
    Paths {
        paths: Vec<RgqlPathResult>,
        evidence_pack: Option<EvidencePack>,
    },
    Causal {
        paths: Vec<CausalPath>,
    },
    Contradictions {
        contradictions: Vec<Contradiction>,
        evidence_pack: Option<EvidencePack>,
    },
    Counterfactual {
        trace: ImpactTrace,
    },
}

impl RgqlExecutionResult {
    pub fn assertions(&self) -> &[QueryResult] {
        match self {
            Self::Graph { assertions, .. } => assertions,
            _ => &[],
        }
    }

    pub fn evidence_pack(&self) -> Option<&EvidencePack> {
        match self {
            Self::Graph { evidence_pack, .. } | Self::Paths { evidence_pack, .. } => {
                evidence_pack.as_ref()
            }
            Self::Contradictions { evidence_pack, .. } => evidence_pack.as_ref(),
            Self::Causal { .. } | Self::Counterfactual { .. } => None,
        }
    }

    pub fn paths(&self) -> &[RgqlPathResult] {
        match self {
            Self::Paths { paths, .. } => paths,
            _ => &[],
        }
    }

    pub fn contradictions(&self) -> &[Contradiction] {
        match self {
            Self::Contradictions { contradictions, .. } => contradictions,
            _ => &[],
        }
    }

    pub fn impact_trace(&self) -> Option<&ImpactTrace> {
        match self {
            Self::Counterfactual { trace } => Some(trace),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RgqlPathResult {
    pub start: EntityId,
    pub end: EntityId,
    pub hops: Vec<QueryResult>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RgqlExecutionError {
    UnknownEntity(String),
}

impl fmt::Display for RgqlExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownEntity(entity) => write!(formatter, "unknown entity: {entity}"),
        }
    }
}

impl Error for RgqlExecutionError {}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TokenKind {
    Word(String),
    String(String),
    Number(String),
    LParen,
    RParen,
    Pipe,
    Comma,
    Range,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Token {
    kind: TokenKind,
    position: usize,
}

struct Lexer<'a> {
    input: &'a str,
    bytes: &'a [u8],
    index: usize,
}

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            bytes: input.as_bytes(),
            index: 0,
        }
    }

    fn lex(mut self) -> Result<Vec<Token>, RgqlParseError> {
        let mut tokens = Vec::new();
        while self.index < self.bytes.len() {
            let byte = self.bytes[self.index];
            if byte.is_ascii_whitespace() {
                self.index += 1;
                continue;
            }
            let position = self.index;
            match byte {
                b'(' => {
                    self.index += 1;
                    tokens.push(Token {
                        kind: TokenKind::LParen,
                        position,
                    });
                }
                b')' => {
                    self.index += 1;
                    tokens.push(Token {
                        kind: TokenKind::RParen,
                        position,
                    });
                }
                b'|' => {
                    self.index += 1;
                    tokens.push(Token {
                        kind: TokenKind::Pipe,
                        position,
                    });
                }
                b',' => {
                    self.index += 1;
                    tokens.push(Token {
                        kind: TokenKind::Comma,
                        position,
                    });
                }
                b'.' if self.bytes.get(self.index + 1) == Some(&b'.') => {
                    self.index += 2;
                    tokens.push(Token {
                        kind: TokenKind::Range,
                        position,
                    });
                }
                b'"' => tokens.push(self.lex_string(position)?),
                b'0'..=b'9' => tokens.push(self.lex_number(position)),
                _ if is_word_start(byte) => tokens.push(self.lex_word(position)),
                _ => {
                    return Err(RgqlParseError {
                        position,
                        message: format!("unexpected character {}", byte as char),
                    });
                }
            }
        }
        Ok(tokens)
    }

    fn lex_string(&mut self, position: usize) -> Result<Token, RgqlParseError> {
        self.index += 1;
        let start = self.index;
        while self.index < self.bytes.len() && self.bytes[self.index] != b'"' {
            self.index += 1;
        }
        if self.index >= self.bytes.len() {
            return Err(RgqlParseError {
                position,
                message: "unterminated string literal".to_owned(),
            });
        }
        let value = self.input[start..self.index].to_owned();
        self.index += 1;
        Ok(Token {
            kind: TokenKind::String(value),
            position,
        })
    }

    fn lex_number(&mut self, position: usize) -> Token {
        let start = self.index;
        while self
            .bytes
            .get(self.index)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'.')
        {
            if self.bytes.get(self.index) == Some(&b'.')
                && self.bytes.get(self.index + 1) == Some(&b'.')
            {
                break;
            }
            self.index += 1;
        }
        Token {
            kind: TokenKind::Number(self.input[start..self.index].to_owned()),
            position,
        }
    }

    fn lex_word(&mut self, position: usize) -> Token {
        let start = self.index;
        while self
            .bytes
            .get(self.index)
            .is_some_and(|byte| is_word_continue(*byte))
        {
            self.index += 1;
        }
        Token {
            kind: TokenKind::Word(self.input[start..self.index].to_owned()),
            position,
        }
    }
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
    input_len: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>, input_len: usize) -> Self {
        Self {
            tokens,
            index: 0,
            input_len,
        }
    }

    fn parse_statement(&mut self) -> Result<RgqlStatement, RgqlParseError> {
        let keyword = self.expect_word("expected RGQL statement keyword")?;
        let statement = match keyword.to_ascii_uppercase().as_str() {
            "FIND" | "MATCH" => self.parse_find(),
            "PATH" => self.parse_path(),
            "CAUSES" => self.parse_causes(),
            "CONTRADICTIONS" => self.parse_contradictions(),
            "COUNTERFACTUAL" => self.parse_counterfactual(),
            other => Err(self.error_at_current(format!(
                "expected MATCH, FIND, PATH, CAUSES, CONTRADICTIONS, or COUNTERFACTUAL; got {other}"
            )))?,
        }?;
        if !self.is_eof() {
            return Err(self.error_at_current("unexpected trailing tokens"));
        }
        Ok(statement)
    }

    fn parse_find(&mut self) -> Result<RgqlStatement, RgqlParseError> {
        let entity = self.parse_entity_selector()?;
        let mut predicate = None;
        let mut object = None;
        if self.consume_word_ci("WHERE") {
            predicate = Some(PredicateId::new(
                self.expect_word("expected predicate after WHERE")?,
            ));
            if !self.is_clause_start() && !self.is_eof() {
                object = Some(self.parse_entity_selector()?);
            }
        }

        let mut valid_at = None;
        let mut known_at = None;
        let mut with_evidence = false;
        let mut min_confidence = None;
        let mut contradictions = false;
        let mut limit = None;

        while !self.is_eof() {
            if self.consume_word_ci("VALID_AT") {
                valid_at =
                    Some(self.expect_timestamp("expected timestamp literal after VALID_AT")?);
            } else if self.consume_word_ci("KNOWN_AT") {
                known_at =
                    Some(self.expect_timestamp("expected timestamp literal after KNOWN_AT")?);
            } else if self.consume_word_ci("WITH") {
                self.expect_word_ci("EVIDENCE", "expected EVIDENCE after WITH")?;
                with_evidence = true;
            } else if self.consume_word_ci("MIN_CONFIDENCE") {
                min_confidence = Some(self.expect_f32("expected confidence after MIN_CONFIDENCE")?);
            } else if self.consume_word_ci("CONTRADICTIONS") {
                contradictions = true;
            } else if self.consume_word_ci("LIMIT") {
                limit = Some(self.expect_usize("expected integer after LIMIT")?);
            } else if self.consume_word_ci("RETURN") {
                self.parse_return_list()?;
            } else {
                return Err(self.error_at_current("expected RGQL clause"));
            }
        }

        Ok(RgqlStatement::Find {
            entity,
            predicate,
            object,
            valid_at,
            known_at,
            with_evidence,
            min_confidence,
            contradictions,
            limit,
        })
    }

    fn parse_path(&mut self) -> Result<RgqlStatement, RgqlParseError> {
        self.expect_word_ci("FROM", "expected FROM after PATH")?;
        let from = self.parse_entity_selector()?;
        let to = if self.consume_word_ci("TO") {
            Some(self.parse_entity_selector()?)
        } else {
            None
        };
        let mut via = Vec::new();
        if self.consume_word_ci("VIA") {
            via = self.parse_predicate_list()?;
        }
        let mut valid_at = None;
        let mut valid_during = None;
        let mut min_confidence = None;
        let mut max_depth = 3;
        let mut returns = Vec::new();
        while !self.is_eof() {
            if self.consume_word_ci("VALID_AT") {
                valid_at =
                    Some(self.expect_timestamp("expected timestamp literal after VALID_AT")?);
            } else if self.consume_word_ci("VALID_DURING") {
                let start = self.expect_timestamp("expected start timestamp after VALID_DURING")?;
                self.expect_range("expected .. in VALID_DURING interval")?;
                let end = self.expect_timestamp("expected end timestamp after VALID_DURING")?;
                valid_during = Some((start, end));
            } else if self.consume_word_ci("MIN_CONFIDENCE") {
                min_confidence = Some(self.expect_f32("expected confidence after MIN_CONFIDENCE")?);
            } else if self.consume_word_ci("MAX_DEPTH") {
                max_depth = self.expect_usize("expected integer after MAX_DEPTH")?;
            } else if self.consume_word_ci("RETURN") {
                returns = self.parse_return_list()?;
            } else {
                return Err(self.error_at_current("expected PATH clause"));
            }
        }
        Ok(RgqlStatement::Path {
            from,
            to,
            via,
            valid_at,
            valid_during,
            min_confidence,
            max_depth,
            returns,
        })
    }

    fn parse_causes(&mut self) -> Result<RgqlStatement, RgqlParseError> {
        self.expect_word_ci("OF", "expected OF after CAUSES")?;
        let event = self.parse_event_selector()?;
        let mut within = None;
        let mut min_confidence = None;
        let mut max_depth = 3;
        let mut returns = Vec::new();
        while !self.is_eof() {
            if self.consume_word_ci("WITHIN") {
                within = Some(self.expect_duration("expected duration after WITHIN")?);
            } else if self.consume_word_ci("MIN_CONFIDENCE") {
                min_confidence = Some(self.expect_f32("expected confidence after MIN_CONFIDENCE")?);
            } else if self.consume_word_ci("MAX_DEPTH") {
                max_depth = self.expect_usize("expected integer after MAX_DEPTH")?;
            } else if self.consume_word_ci("RETURN") {
                returns = self.parse_return_list()?;
            } else {
                return Err(self.error_at_current("expected CAUSES clause"));
            }
        }
        Ok(RgqlStatement::Causes {
            event,
            within,
            min_confidence,
            max_depth,
            returns,
        })
    }

    fn parse_contradictions(&mut self) -> Result<RgqlStatement, RgqlParseError> {
        let entity = if self.consume_word_ci("FOR") {
            Some(self.parse_entity_selector()?)
        } else {
            None
        };
        let mut valid_at = None;
        let mut known_at = None;
        let mut returns = Vec::new();
        while !self.is_eof() {
            if self.consume_word_ci("VALID_AT") {
                valid_at =
                    Some(self.expect_timestamp("expected timestamp literal after VALID_AT")?);
            } else if self.consume_word_ci("KNOWN_AT") {
                known_at =
                    Some(self.expect_timestamp("expected timestamp literal after KNOWN_AT")?);
            } else if self.consume_word_ci("RETURN") {
                returns = self.parse_return_list()?;
            } else {
                return Err(self.error_at_current("expected CONTRADICTIONS clause"));
            }
        }
        Ok(RgqlStatement::Contradictions {
            entity,
            valid_at,
            known_at,
            returns,
        })
    }

    fn parse_counterfactual(&mut self) -> Result<RgqlStatement, RgqlParseError> {
        self.expect_word_ci("REMOVE", "expected REMOVE after COUNTERFACTUAL")?;
        let intervention_kind = self.expect_word("expected ASSERTION or EVENT after REMOVE")?;
        let intervention = match intervention_kind.to_ascii_uppercase().as_str() {
            "ASSERTION" => CounterfactualIntervention::RemoveAssertion(AssertionId::new(
                self.parse_call_string("ASSERTION")?,
            )),
            "EVENT" => CounterfactualIntervention::RemoveEvent(EventId::new(
                self.parse_call_string("EVENT")?,
            )),
            _ => {
                return Err(self.error_at_current(
                    "expected ASSERTION or EVENT intervention after COUNTERFACTUAL REMOVE",
                ));
            }
        };
        let mut valid_at = None;
        let mut max_depth = 3;
        let mut returns = Vec::new();
        while !self.is_eof() {
            if self.consume_word_ci("VALID_AT") {
                valid_at =
                    Some(self.expect_timestamp("expected timestamp literal after VALID_AT")?);
            } else if self.consume_word_ci("MAX_DEPTH") {
                max_depth = self.expect_usize("expected integer after MAX_DEPTH")?;
            } else if self.consume_word_ci("RETURN") {
                returns = self.parse_return_list()?;
            } else {
                return Err(self.error_at_current("expected COUNTERFACTUAL clause"));
            }
        }
        Ok(RgqlStatement::Counterfactual {
            intervention,
            valid_at,
            max_depth,
            returns,
        })
    }

    fn parse_entity_selector(&mut self) -> Result<EntitySelector, RgqlParseError> {
        let word = self.expect_word("expected entity selector")?;
        if self.consume(TokenKind::LParen) {
            let name = self.expect_string("expected entity name string")?;
            self.expect(TokenKind::RParen, "expected ) after entity selector")?;
            if word.eq_ignore_ascii_case("Id") {
                Ok(EntitySelector::Id(EntityId::new(name)))
            } else {
                Ok(EntitySelector::TypedName {
                    entity_type: word,
                    name,
                })
            }
        } else {
            Ok(EntitySelector::Type { entity_type: word })
        }
    }

    fn parse_event_selector(&mut self) -> Result<EventId, RgqlParseError> {
        self.expect_word_ci("Event", "expected Event(\"id\")")?;
        Ok(EventId::new(self.parse_call_string("Event")?))
    }

    fn parse_call_string(&mut self, name: &str) -> Result<String, RgqlParseError> {
        self.expect(TokenKind::LParen, format!("expected ( after {name}"))?;
        let value = self.expect_string("expected string argument")?;
        self.expect(TokenKind::RParen, "expected ) after string argument")?;
        Ok(value)
    }

    fn parse_predicate_list(&mut self) -> Result<Vec<PredicateId>, RgqlParseError> {
        let mut predicates = vec![PredicateId::new(
            self.expect_word("expected predicate after VIA")?,
        )];
        while self.consume(TokenKind::Pipe) {
            predicates.push(PredicateId::new(
                self.expect_word("expected predicate after |")?,
            ));
        }
        Ok(predicates)
    }

    fn parse_return_list(&mut self) -> Result<Vec<String>, RgqlParseError> {
        let mut returns = vec![self.expect_word("expected return field")?];
        while self.consume(TokenKind::Comma) {
            returns.push(self.expect_word("expected return field after comma")?);
        }
        Ok(returns)
    }

    fn expect_timestamp(&mut self, message: &str) -> Result<i64, RgqlParseError> {
        let Some(token) = self.tokens.get(self.index).cloned() else {
            return Err(RgqlParseError {
                position: self.input_len,
                message: message.to_owned(),
            });
        };
        self.index += 1;
        let raw = match token.kind {
            TokenKind::String(value) | TokenKind::Number(value) | TokenKind::Word(value) => value,
            _ => {
                return Err(RgqlParseError {
                    position: token.position,
                    message: message.to_owned(),
                });
            }
        };
        parse_timestamp_value(&raw).ok_or_else(|| RgqlParseError {
            position: token.position,
            message: format!("invalid timestamp literal {raw:?}"),
        })
    }

    fn expect_duration(&mut self, message: &str) -> Result<DurationLiteral, RgqlParseError> {
        let Some(token) = self.tokens.get(self.index).cloned() else {
            return Err(RgqlParseError {
                position: self.input_len,
                message: message.to_owned(),
            });
        };
        self.index += 1;
        let raw = match token.kind {
            TokenKind::String(value) | TokenKind::Number(value) | TokenKind::Word(value) => value,
            _ => {
                return Err(RgqlParseError {
                    position: token.position,
                    message: message.to_owned(),
                });
            }
        };
        parse_duration(&raw).ok_or_else(|| RgqlParseError {
            position: token.position,
            message: format!("invalid duration literal {raw:?}"),
        })
    }

    fn expect_f32(&mut self, message: &str) -> Result<f32, RgqlParseError> {
        let token = self.next_token(message)?;
        let raw = match &token.kind {
            TokenKind::Number(value) | TokenKind::String(value) | TokenKind::Word(value) => value,
            _ => {
                return Err(RgqlParseError {
                    position: token.position,
                    message: message.to_owned(),
                });
            }
        };
        raw.parse::<f32>().map_err(|_| RgqlParseError {
            position: token.position,
            message: format!("invalid number {raw:?}"),
        })
    }

    fn expect_usize(&mut self, message: &str) -> Result<usize, RgqlParseError> {
        let token = self.next_token(message)?;
        let raw = match &token.kind {
            TokenKind::Number(value) | TokenKind::String(value) | TokenKind::Word(value) => value,
            _ => {
                return Err(RgqlParseError {
                    position: token.position,
                    message: message.to_owned(),
                });
            }
        };
        raw.parse::<usize>().map_err(|_| RgqlParseError {
            position: token.position,
            message: format!("invalid integer {raw:?}"),
        })
    }

    fn expect_word(&mut self, message: impl Into<String>) -> Result<String, RgqlParseError> {
        let token = self.next_token(message)?;
        match token.kind {
            TokenKind::Word(value) => Ok(value),
            _ => Err(RgqlParseError {
                position: token.position,
                message: "expected identifier".to_owned(),
            }),
        }
    }

    fn expect_word_ci(
        &mut self,
        expected: &str,
        message: impl Into<String>,
    ) -> Result<(), RgqlParseError> {
        let token = self.next_token(message)?;
        match token.kind {
            TokenKind::Word(value) if value.eq_ignore_ascii_case(expected) => Ok(()),
            _ => Err(RgqlParseError {
                position: token.position,
                message: format!("expected {expected}"),
            }),
        }
    }

    fn expect_string(&mut self, message: impl Into<String>) -> Result<String, RgqlParseError> {
        let token = self.next_token(message)?;
        match token.kind {
            TokenKind::String(value) => Ok(value),
            _ => Err(RgqlParseError {
                position: token.position,
                message: "expected string literal".to_owned(),
            }),
        }
    }

    fn expect_range(&mut self, message: impl Into<String>) -> Result<(), RgqlParseError> {
        self.expect(TokenKind::Range, message)
    }

    fn expect(
        &mut self,
        expected: TokenKind,
        message: impl Into<String>,
    ) -> Result<(), RgqlParseError> {
        if self.consume(expected) {
            Ok(())
        } else {
            Err(self.error_at_current(message))
        }
    }

    fn next_token(&mut self, message: impl Into<String>) -> Result<Token, RgqlParseError> {
        let Some(token) = self.tokens.get(self.index).cloned() else {
            return Err(RgqlParseError {
                position: self.input_len,
                message: message.into(),
            });
        };
        self.index += 1;
        Ok(token)
    }

    fn consume_word_ci(&mut self, expected: &str) -> bool {
        let Some(Token {
            kind: TokenKind::Word(value),
            ..
        }) = self.tokens.get(self.index)
        else {
            return false;
        };
        if value.eq_ignore_ascii_case(expected) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn consume(&mut self, expected: TokenKind) -> bool {
        if self
            .tokens
            .get(self.index)
            .is_some_and(|token| token.kind == expected)
        {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn is_clause_start(&self) -> bool {
        matches!(
            self.tokens.get(self.index),
            Some(Token {
                kind: TokenKind::Word(value),
                ..
            }) if is_clause_keyword(value)
        )
    }

    fn is_eof(&self) -> bool {
        self.index >= self.tokens.len()
    }

    fn error_at_current(&self, message: impl Into<String>) -> RgqlParseError {
        RgqlParseError {
            position: self
                .tokens
                .get(self.index)
                .map_or(self.input_len, |token| token.position),
            message: message.into(),
        }
    }
}

fn edge_matches_path(
    assertion: &Assertion,
    via: &[PredicateId],
    valid_at: Option<i64>,
    valid_during: Option<(i64, i64)>,
    min_confidence: Option<f32>,
) -> bool {
    (via.is_empty()
        || via
            .iter()
            .any(|predicate| predicate == &assertion.predicate))
        && valid_at.map_or(true, |instant| {
            assertion.valid_time.contains(ValidTime::new(instant))
        })
        && valid_during.map_or(true, |(start, end)| {
            assertion.valid_time.overlaps(
                &rg_core::TimeInterval::new(ValidTime::new(start), Some(ValidTime::new(end)))
                    .expect("parser produced ordered interval"),
            )
        })
        && min_confidence.map_or(true, |minimum| assertion.confidence.as_f32() >= minimum)
}

fn entity_type_matches(entity: &Entity, expected: &str) -> bool {
    format!("{:?}", entity.entity_type).eq_ignore_ascii_case(expected)
        || match &entity.entity_type {
            rg_core::EntityType::Organization => {
                expected.eq_ignore_ascii_case("Company")
                    || expected.eq_ignore_ascii_case("Organization")
            }
            rg_core::EntityType::Person => expected.eq_ignore_ascii_case("Person"),
            rg_core::EntityType::Place => expected.eq_ignore_ascii_case("Place"),
            rg_core::EntityType::Event => expected.eq_ignore_ascii_case("Event"),
            rg_core::EntityType::Document => expected.eq_ignore_ascii_case("Document"),
            rg_core::EntityType::Concept => expected.eq_ignore_ascii_case("Concept"),
            rg_core::EntityType::Custom(value) => value.eq_ignore_ascii_case(expected),
        }
}

fn entity_name_matches(entity: &Entity, expected: &str) -> bool {
    entity.id.as_str().eq_ignore_ascii_case(expected)
        || entity
            .canonical_name
            .as_ref()
            .is_some_and(|name| name.eq_ignore_ascii_case(expected))
}

fn generated_at(storage: &InMemoryStorage) -> TxTime {
    TxTime::new(storage.events().len() as i64)
}

fn statement_returns(statement: &RgqlStatement, field: &str) -> bool {
    let returns = match statement {
        RgqlStatement::Path { returns, .. }
        | RgqlStatement::Causes { returns, .. }
        | RgqlStatement::Contradictions { returns, .. }
        | RgqlStatement::Counterfactual { returns, .. } => returns,
        RgqlStatement::Find { .. } => return false,
    };
    returns
        .iter()
        .any(|value| value.eq_ignore_ascii_case(field))
}

fn statement_to_query_text(statement: &RgqlStatement) -> String {
    match statement {
        RgqlStatement::Find {
            predicate,
            valid_at,
            known_at,
            ..
        } => format!(
            "RGQL FIND predicate={} valid_at={} known_at={}",
            predicate.as_ref().map_or("*", PredicateId::as_str),
            valid_at
                .map(|value| value.to_string())
                .unwrap_or_else(|| "*".to_owned()),
            known_at
                .map(|value| value.to_string())
                .unwrap_or_else(|| "*".to_owned())
        ),
        RgqlStatement::Path { .. } => "RGQL PATH".to_owned(),
        RgqlStatement::Causes { .. } => "RGQL CAUSES".to_owned(),
        RgqlStatement::Contradictions { .. } => "RGQL CONTRADICTIONS".to_owned(),
        RgqlStatement::Counterfactual { .. } => "RGQL COUNTERFACTUAL".to_owned(),
    }
}

fn is_word_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_word_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

fn is_clause_keyword(value: &str) -> bool {
    matches!(
        value.to_ascii_uppercase().as_str(),
        "VALID_AT"
            | "KNOWN_AT"
            | "VALID_DURING"
            | "WITH"
            | "MIN_CONFIDENCE"
            | "CONTRADICTIONS"
            | "LIMIT"
            | "RETURN"
            | "MAX_DEPTH"
    )
}

fn parse_timestamp_value(raw: &str) -> Option<i64> {
    if raw.len() == 10
        && raw.as_bytes().get(4) == Some(&b'-')
        && raw.as_bytes().get(7) == Some(&b'-')
    {
        return raw.replace('-', "").parse::<i64>().ok();
    }
    raw.parse::<i64>().ok()
}

fn parse_duration(raw: &str) -> Option<DurationLiteral> {
    let split_at = raw
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(raw.len());
    if split_at == 0 || split_at == raw.len() {
        return None;
    }
    Some(DurationLiteral {
        raw: raw.to_owned(),
        amount: raw[..split_at].parse().ok()?,
        unit: raw[split_at..].to_owned(),
    })
}

fn first_iso_date(value: &str) -> Option<String> {
    value
        .as_bytes()
        .windows(10)
        .position(|window| {
            window[0..4].iter().all(u8::is_ascii_digit)
                && window[4] == b'-'
                && window[5..7].iter().all(u8::is_ascii_digit)
                && window[7] == b'-'
                && window[8..10].iter().all(u8::is_ascii_digit)
        })
        .map(|position| value[position..position + 10].to_owned())
}

fn extract_between(value: &str, start: &str, end: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let start_index = lower.find(start)? + start.len();
    let end_index = lower[start_index..].find(end)? + start_index;
    Some(value[start_index..end_index].trim().to_owned()).filter(|candidate| !candidate.is_empty())
}

fn round_two(value: f32) -> f32 {
    (value * 100.0).round() / 100.0
}
