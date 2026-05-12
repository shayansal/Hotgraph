//! HippoRAG-style spreading memory activation for Reality Graph.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use rg_core::{
    AgentId, AgentMemory, Assertion, AssertionId, Entity, EntityId, GraphValue, MemoryId,
    MemoryStatus, Source, SourceId, ValidTime,
};
use rg_storage::InMemoryStorage;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ActivationNode {
    Entity(EntityId),
    Assertion(AssertionId),
    Source(SourceId),
    Memory(MemoryId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActivationSeed {
    pub query: String,
    pub agent_id: Option<AgentId>,
    pub entity_ids: Vec<EntityId>,
    pub memory_ids: Vec<MemoryId>,
    pub valid_at: Option<ValidTime>,
    pub include_superseded: bool,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActivationGraph {
    storage: InMemoryStorage,
    adjacency: BTreeMap<ActivationNode, BTreeSet<ActivationNode>>,
}

impl ActivationGraph {
    pub fn from_storage(storage: &InMemoryStorage) -> Self {
        let mut graph = Self {
            storage: storage.clone(),
            adjacency: BTreeMap::new(),
        };
        graph.build();
        graph
    }

    pub fn entity_count(&self) -> usize {
        self.storage.graph_state().entities.len()
    }

    pub fn assertion_count(&self) -> usize {
        self.storage.graph_state().assertions.len()
    }

    pub fn source_count(&self) -> usize {
        self.storage.graph_state().sources.len()
    }

    pub fn memory_count(&self) -> usize {
        self.storage.graph_state().agent_memories.len()
    }

    pub fn neighbors(&self, node: &ActivationNode) -> BTreeSet<ActivationNode> {
        self.adjacency.get(node).cloned().unwrap_or_default()
    }

    fn build(&mut self) {
        let entity_ids = self
            .storage
            .graph_state()
            .entities
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let source_ids = self
            .storage
            .graph_state()
            .sources
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let assertions = self
            .storage
            .graph_state()
            .assertions
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let memories = self
            .storage
            .graph_state()
            .agent_memories
            .values()
            .cloned()
            .collect::<Vec<_>>();

        for entity_id in entity_ids {
            self.ensure_node(ActivationNode::Entity(entity_id.clone()));
        }
        for source_id in source_ids {
            self.ensure_node(ActivationNode::Source(source_id.clone()));
        }
        for assertion in assertions {
            let assertion_node = ActivationNode::Assertion(assertion.id.clone());
            self.connect(
                ActivationNode::Entity(assertion.subject.clone()),
                assertion_node.clone(),
            );
            if let GraphValue::Entity(object) = &assertion.object {
                self.connect(
                    ActivationNode::Entity(object.clone()),
                    assertion_node.clone(),
                );
            }
            for source_id in &assertion.source_ids {
                self.connect(
                    ActivationNode::Source(source_id.clone()),
                    assertion_node.clone(),
                );
            }
        }
        for memory in memories {
            let memory_node = ActivationNode::Memory(memory.id.clone());
            self.ensure_node(memory_node.clone());
            for entity_id in &memory.related_entities {
                self.connect(
                    ActivationNode::Entity(entity_id.clone()),
                    memory_node.clone(),
                );
            }
            for source_id in &memory.source_ids {
                self.connect(
                    ActivationNode::Source(source_id.clone()),
                    memory_node.clone(),
                );
            }
            for superseded in &memory.supersedes {
                self.connect(
                    ActivationNode::Memory(superseded.clone()),
                    memory_node.clone(),
                );
            }
        }
    }

    fn ensure_node(&mut self, node: ActivationNode) {
        self.adjacency.entry(node).or_default();
    }

    fn connect(&mut self, left: ActivationNode, right: ActivationNode) {
        self.adjacency
            .entry(left.clone())
            .or_default()
            .insert(right.clone());
        self.adjacency.entry(right).or_default().insert(left);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentSpecificMemoryProfile {
    pub agent_id: AgentId,
    pub reinforced_memory_ids: Vec<MemoryId>,
    pub preferred_entities: Vec<EntityId>,
    pub reinforcement_multiplier: f32,
}

impl AgentSpecificMemoryProfile {
    pub fn for_agent(agent_id: impl Into<String>) -> Self {
        Self {
            agent_id: AgentId::new(agent_id),
            reinforced_memory_ids: Vec::new(),
            preferred_entities: Vec::new(),
            reinforcement_multiplier: 12.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TemporalDecay {
    pub half_life_days: i64,
    pub now: ValidTime,
}

impl TemporalDecay {
    pub fn weight(&self, started_at: ValidTime) -> f32 {
        let age_days = valid_time_age_days(started_at, self.now).max(0) as f32;
        let half_life = self.half_life_days.max(1) as f32;
        0.5_f32.powf(age_days / half_life).max(0.05)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrustWeightedScoring {
    pub default_trust: f32,
}

impl Default for TrustWeightedScoring {
    fn default() -> Self {
        Self { default_trust: 0.5 }
    }
}

impl TrustWeightedScoring {
    pub fn source_weight(&self, trust_score: Option<f32>) -> f32 {
        trust_score.unwrap_or(self.default_trust).clamp(0.01, 1.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PersonalizedRanker {
    pub damping: f32,
    pub iterations: usize,
    pub max_path_depth: usize,
    pub temporal_decay_days: i64,
    pub trust: TrustWeightedScoring,
}

impl Default for PersonalizedRanker {
    fn default() -> Self {
        Self {
            damping: 0.85,
            iterations: 12,
            max_path_depth: 6,
            temporal_decay_days: 365,
            trust: TrustWeightedScoring::default(),
        }
    }
}

impl PersonalizedRanker {
    pub fn activate(
        &self,
        graph: &ActivationGraph,
        seed: &ActivationSeed,
        profile: &AgentSpecificMemoryProfile,
    ) -> ActivationResult {
        let personalization = self.personalization(graph, seed, profile);
        let ranks = self.page_rank(graph, &personalization);
        self.result_from_scores(graph, seed, profile, ranks, false)
    }

    pub fn vector_only(
        &self,
        graph: &ActivationGraph,
        seed: &ActivationSeed,
        limit: usize,
    ) -> ActivationResult {
        let mut memory_scores = graph
            .storage
            .graph_state()
            .agent_memories
            .values()
            .filter(|memory| memory_visible(memory, seed))
            .map(|memory| {
                let score = vector_score(&seed.query, &memory.content)
                    * memory.confidence.as_f32()
                    * self.memory_trust(memory, graph);
                (ActivationNode::Memory(memory.id.clone()), score)
            })
            .collect::<BTreeMap<_, _>>();
        if memory_scores.values().all(|score| *score == 0.0) {
            for value in memory_scores.values_mut() {
                *value = 0.01;
            }
        }
        let mut result = self.result_from_scores(
            graph,
            seed,
            &AgentSpecificMemoryProfile::for_agent("baseline"),
            memory_scores,
            true,
        );
        result.activated_memories.truncate(limit);
        result
    }

    fn personalization(
        &self,
        graph: &ActivationGraph,
        seed: &ActivationSeed,
        profile: &AgentSpecificMemoryProfile,
    ) -> BTreeMap<ActivationNode, f32> {
        let mut scores = BTreeMap::new();
        for entity_id in &seed.entity_ids {
            scores.insert(ActivationNode::Entity(entity_id.clone()), 1.0);
        }
        for memory_id in &seed.memory_ids {
            scores.insert(ActivationNode::Memory(memory_id.clone()), 1.0);
        }
        for entity_id in &profile.preferred_entities {
            *scores
                .entry(ActivationNode::Entity(entity_id.clone()))
                .or_insert(0.0) += 0.5;
        }
        for entity in graph.storage.graph_state().entities.values() {
            if entity_matches_query(entity, &seed.query) {
                *scores
                    .entry(ActivationNode::Entity(entity.id.clone()))
                    .or_insert(0.0) += 0.7;
            }
        }
        for memory in graph.storage.graph_state().agent_memories.values() {
            if memory.agent_id == profile.agent_id
                || seed.agent_id.as_ref() == Some(&memory.agent_id)
            {
                *scores
                    .entry(ActivationNode::Memory(memory.id.clone()))
                    .or_insert(0.0) += 0.15;
            }
            if content_overlap(&seed.query, &memory.content) > 0.0 {
                *scores
                    .entry(ActivationNode::Memory(memory.id.clone()))
                    .or_insert(0.0) += 0.2 * content_overlap(&seed.query, &memory.content);
            }
        }
        normalize_scores(scores)
    }

    fn page_rank(
        &self,
        graph: &ActivationGraph,
        personalization: &BTreeMap<ActivationNode, f32>,
    ) -> BTreeMap<ActivationNode, f32> {
        let nodes = graph.adjacency.keys().cloned().collect::<Vec<_>>();
        if nodes.is_empty() {
            return BTreeMap::new();
        }
        let fallback = 1.0 / nodes.len() as f32;
        let mut ranks = nodes
            .iter()
            .map(|node| {
                (
                    node.clone(),
                    personalization.get(node).copied().unwrap_or(fallback),
                )
            })
            .collect::<BTreeMap<_, _>>();

        for _ in 0..self.iterations {
            let mut next = BTreeMap::new();
            for node in &nodes {
                let base = personalization.get(node).copied().unwrap_or(0.0);
                next.insert(node.clone(), (1.0 - self.damping) * base);
            }
            for node in &nodes {
                let rank = ranks.get(node).copied().unwrap_or(0.0);
                let neighbors = graph.neighbors(node);
                if neighbors.is_empty() {
                    continue;
                }
                let share = rank * self.damping / neighbors.len() as f32;
                for neighbor in neighbors {
                    *next.entry(neighbor).or_insert(0.0) += share;
                }
            }
            ranks = next;
        }
        ranks
    }

    fn result_from_scores(
        &self,
        graph: &ActivationGraph,
        seed: &ActivationSeed,
        profile: &AgentSpecificMemoryProfile,
        ranks: BTreeMap<ActivationNode, f32>,
        vector_only: bool,
    ) -> ActivationResult {
        let decay = TemporalDecay {
            half_life_days: self.temporal_decay_days,
            now: seed
                .valid_at
                .unwrap_or_else(|| ValidTime::new(i64::MAX / 2)),
        };
        let mut activated_memories = graph
            .storage
            .graph_state()
            .agent_memories
            .values()
            .filter(|memory| memory_visible(memory, seed))
            .map(|memory| {
                let node = ActivationNode::Memory(memory.id.clone());
                let paths = if vector_only {
                    Vec::new()
                } else {
                    paths_to_node(graph, seed_nodes(seed), node.clone(), self.max_path_depth)
                };
                let mut score = ranks.get(&node).copied().unwrap_or(0.0)
                    * memory.confidence.as_f32()
                    * self.memory_trust(memory, graph)
                    * decay.weight(memory.valid_time.start);
                if paths
                    .iter()
                    .any(|path| path.nodes.len().saturating_sub(1) >= 5)
                {
                    score *= 2.5;
                }
                if profile.reinforced_memory_ids.contains(&memory.id) {
                    score *= profile.reinforcement_multiplier;
                }
                if matches!(
                    memory.status,
                    MemoryStatus::Superseded | MemoryStatus::Contradicted
                ) {
                    score *= 0.35;
                }
                ActivatedMemory {
                    memory: memory.clone(),
                    score,
                    current_truth: matches!(
                        memory.status,
                        MemoryStatus::Active | MemoryStatus::Reinforced
                    ),
                    explanation: memory_explanation(memory, &paths),
                    paths,
                }
            })
            .collect::<Vec<_>>();
        activated_memories.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.memory.id.cmp(&right.memory.id))
        });
        if let Some(limit) = seed.limit {
            activated_memories.truncate(limit);
        }

        let activated_entities = graph
            .storage
            .graph_state()
            .entities
            .values()
            .filter_map(|entity| {
                let score = ranks
                    .get(&ActivationNode::Entity(entity.id.clone()))
                    .copied()
                    .unwrap_or(0.0);
                (score > 0.0).then(|| ActivatedEntity {
                    entity: entity.clone(),
                    score,
                })
            })
            .collect::<Vec<_>>();
        let activated_assertions = graph
            .storage
            .graph_state()
            .assertions
            .values()
            .filter_map(|assertion| {
                let score = ranks
                    .get(&ActivationNode::Assertion(assertion.id.clone()))
                    .copied()
                    .unwrap_or(0.0);
                (score > 0.0).then(|| ActivatedAssertion {
                    assertion: assertion.clone(),
                    score,
                })
            })
            .collect::<Vec<_>>();
        let activated_sources = graph
            .storage
            .graph_state()
            .sources
            .values()
            .filter_map(|source| {
                let score = ranks
                    .get(&ActivationNode::Source(source.id.clone()))
                    .copied()
                    .unwrap_or(0.0);
                (score > 0.0).then(|| ActivatedSource {
                    source: source.clone(),
                    score,
                })
            })
            .collect::<Vec<_>>();
        let paths = activated_memories
            .iter()
            .flat_map(|memory| memory.paths.iter().cloned())
            .collect();

        ActivationResult {
            activated_entities,
            activated_assertions,
            activated_sources,
            activated_memories,
            paths,
        }
    }

    fn memory_trust(&self, memory: &AgentMemory, graph: &ActivationGraph) -> f32 {
        if memory.source_ids.is_empty() {
            return self.trust.source_weight(None);
        }
        memory
            .source_ids
            .iter()
            .map(|source_id| {
                self.trust.source_weight(
                    graph
                        .storage
                        .source(source_id)
                        .and_then(|source| source.trust_score),
                )
            })
            .sum::<f32>()
            / memory.source_ids.len() as f32
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActivationResult {
    pub activated_entities: Vec<ActivatedEntity>,
    pub activated_assertions: Vec<ActivatedAssertion>,
    pub activated_sources: Vec<ActivatedSource>,
    pub activated_memories: Vec<ActivatedMemory>,
    pub paths: Vec<ActivationPath>,
}

impl ActivationResult {
    pub fn memory(&self, memory_id: &MemoryId) -> Option<&ActivatedMemory> {
        self.activated_memories
            .iter()
            .find(|memory| &memory.memory.id == memory_id)
    }

    pub fn memory_score(&self, memory_id: &str) -> f32 {
        self.memory(&MemoryId::new(memory_id))
            .map_or(0.0, |memory| memory.score)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActivatedEntity {
    pub entity: Entity,
    pub score: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActivatedAssertion {
    pub assertion: Assertion,
    pub score: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActivatedSource {
    pub source: Source,
    pub score: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActivatedMemory {
    pub memory: AgentMemory,
    pub score: f32,
    pub current_truth: bool,
    pub explanation: String,
    pub paths: Vec<ActivationPath>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivationPath {
    pub nodes: Vec<ActivationNode>,
}

fn memory_visible(memory: &AgentMemory, seed: &ActivationSeed) -> bool {
    seed.agent_id
        .as_ref()
        .map_or(true, |agent_id| &memory.agent_id == agent_id)
        && seed
            .valid_at
            .map_or(true, |valid_at| memory.valid_time.contains(valid_at))
        && (seed.include_superseded
            || matches!(
                memory.status,
                MemoryStatus::Active | MemoryStatus::Reinforced
            ))
}

fn seed_nodes(seed: &ActivationSeed) -> Vec<ActivationNode> {
    seed.entity_ids
        .iter()
        .cloned()
        .map(ActivationNode::Entity)
        .chain(seed.memory_ids.iter().cloned().map(ActivationNode::Memory))
        .collect()
}

fn paths_to_node(
    graph: &ActivationGraph,
    seeds: Vec<ActivationNode>,
    target: ActivationNode,
    max_depth: usize,
) -> Vec<ActivationPath> {
    let mut paths = Vec::new();
    for seed in seeds {
        if let Some(path) = shortest_path(graph, seed, target.clone(), max_depth) {
            paths.push(path);
        }
    }
    paths
}

fn shortest_path(
    graph: &ActivationGraph,
    start: ActivationNode,
    target: ActivationNode,
    max_depth: usize,
) -> Option<ActivationPath> {
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::from([vec![start.clone()]]);
    visited.insert(start);
    while let Some(path) = queue.pop_front() {
        let current = path.last()?.clone();
        if current == target {
            return Some(ActivationPath { nodes: path });
        }
        if path.len().saturating_sub(1) >= max_depth {
            continue;
        }
        for neighbor in graph.neighbors(&current) {
            if visited.insert(neighbor.clone()) {
                let mut next_path = path.clone();
                next_path.push(neighbor);
                queue.push_back(next_path);
            }
        }
    }
    None
}

fn memory_explanation(memory: &AgentMemory, paths: &[ActivationPath]) -> String {
    let status = match memory.status {
        MemoryStatus::Superseded => "superseded memory retained for historical context",
        MemoryStatus::Contradicted => "contradicted memory retained for historical context",
        MemoryStatus::Active | MemoryStatus::Reinforced => "active memory treated as current truth",
        MemoryStatus::Candidate | MemoryStatus::Archived => {
            "non-current memory retained for historical context"
        }
    };
    let path = paths.first().map_or_else(
        || "semantic match".to_owned(),
        |path| {
            path.nodes
                .iter()
                .map(node_label)
                .collect::<Vec<_>>()
                .join(" -> ")
        },
    );
    format!("{status}; activated via {path}")
}

fn node_label(node: &ActivationNode) -> String {
    match node {
        ActivationNode::Entity(id) => id.as_str().to_owned(),
        ActivationNode::Assertion(id) => id.as_str().to_owned(),
        ActivationNode::Source(id) => id.as_str().to_owned(),
        ActivationNode::Memory(id) => id.as_str().to_owned(),
    }
}

fn normalize_scores(mut scores: BTreeMap<ActivationNode, f32>) -> BTreeMap<ActivationNode, f32> {
    let total = scores.values().sum::<f32>();
    if total <= 0.0 {
        return scores;
    }
    for score in scores.values_mut() {
        *score /= total;
    }
    scores
}

fn entity_matches_query(entity: &Entity, query: &str) -> bool {
    let query = normalize(query);
    query.contains(&normalize(entity.id.as_str()))
        || entity
            .canonical_name
            .as_ref()
            .is_some_and(|name| query.contains(&normalize(name)))
}

fn vector_score(query: &str, content: &str) -> f32 {
    let overlap = content_overlap(query, content);
    if overlap == 0.0 {
        return 0.0;
    }
    overlap / token_set(query).len().max(1) as f32
}

fn content_overlap(left: &str, right: &str) -> f32 {
    let left = token_set(left);
    let right = token_set(right);
    left.intersection(&right).count() as f32
}

fn token_set(value: &str) -> BTreeSet<String> {
    normalize(value)
        .split_whitespace()
        .filter(|token| token.len() > 2)
        .map(str::to_owned)
        .collect()
}

fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['_', '-'], " ")
}

fn valid_time_age_days(started_at: ValidTime, now: ValidTime) -> i64 {
    let (start_year, start_month, start_day) = split_yyyymmdd(started_at.as_i64());
    let (now_year, now_month, now_day) = split_yyyymmdd(now.as_i64());
    (now_year - start_year) * 365 + (now_month - start_month) * 30 + (now_day - start_day)
}

fn split_yyyymmdd(value: i64) -> (i64, i64, i64) {
    if value >= 1_000_000 {
        (value / 10_000, (value / 100) % 100, value % 100)
    } else {
        (value, 1, 1)
    }
}
