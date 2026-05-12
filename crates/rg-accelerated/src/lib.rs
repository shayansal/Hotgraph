//! Optional accelerated graph kernels for Reality Graph.
//!
//! The crate starts with deterministic CPU kernels. GPU backends are only
//! feature-gated experiment descriptors until benchmarks prove they beat the
//! optimized CPU paths.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CsrGraph {
    node_count: usize,
    row_offsets: Vec<usize>,
    column_indices: Vec<u32>,
}

impl CsrGraph {
    pub fn from_edges(node_count: usize, edges: &[(u32, u32)]) -> Result<Self, String> {
        let mut sorted_edges = edges.to_vec();
        sorted_edges.sort_unstable();
        sorted_edges.dedup();

        for (source, target) in &sorted_edges {
            if *source as usize >= node_count || *target as usize >= node_count {
                return Err("edge endpoint exceeds node_count".to_string());
            }
        }

        let mut row_offsets = vec![0; node_count + 1];
        for (source, _) in &sorted_edges {
            row_offsets[*source as usize + 1] += 1;
        }
        for index in 1..row_offsets.len() {
            row_offsets[index] += row_offsets[index - 1];
        }

        let column_indices = sorted_edges.into_iter().map(|(_, target)| target).collect();

        Ok(Self {
            node_count,
            row_offsets,
            column_indices,
        })
    }

    pub fn node_count(&self) -> usize {
        self.node_count
    }

    pub fn edge_count(&self) -> usize {
        self.column_indices.len()
    }

    pub fn row_offsets(&self) -> &[usize] {
        &self.row_offsets
    }

    pub fn column_indices(&self) -> &[u32] {
        &self.column_indices
    }

    pub fn neighbors(&self, node: u32) -> Option<&[u32]> {
        let node = node as usize;
        if node >= self.node_count {
            return None;
        }
        let start = self.row_offsets[node];
        let end = self.row_offsets[node + 1];
        Some(&self.column_indices[start..end])
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RoaringCandidateSet {
    values: Vec<u32>,
}

impl RoaringCandidateSet {
    pub fn from_unsorted(values: impl IntoIterator<Item = u32>) -> Self {
        let mut values = values.into_iter().collect::<Vec<_>>();
        values.sort_unstable();
        values.dedup();
        Self { values }
    }

    pub fn contains(&self, value: u32) -> bool {
        self.values.binary_search(&value).is_ok()
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, u32> {
        self.values.iter()
    }

    pub fn to_vec(&self) -> Vec<u32> {
        self.values.clone()
    }

    pub fn intersection(&self, other: &Self) -> Self {
        let mut output = Vec::new();
        let mut left_index = 0;
        let mut right_index = 0;

        while left_index < self.values.len() && right_index < other.values.len() {
            match self.values[left_index].cmp(&other.values[right_index]) {
                std::cmp::Ordering::Less => left_index += 1,
                std::cmp::Ordering::Greater => right_index += 1,
                std::cmp::Ordering::Equal => {
                    output.push(self.values[left_index]);
                    left_index += 1;
                    right_index += 1;
                }
            }
        }

        Self { values: output }
    }

    pub fn union(&self, other: &Self) -> Self {
        let mut output = Vec::with_capacity(self.values.len() + other.values.len());
        output.extend_from_slice(&self.values);
        output.extend_from_slice(&other.values);
        Self::from_unsorted(output)
    }

    pub fn difference(&self, other: &Self) -> Self {
        Self {
            values: self
                .values
                .iter()
                .copied()
                .filter(|value| !other.contains(*value))
                .collect(),
        }
    }
}

pub struct SimdTimestampFilter;

impl SimdTimestampFilter {
    pub fn valid_at(
        valid_from: &[i64],
        valid_to: &[Option<i64>],
        instant: i64,
    ) -> Result<RoaringCandidateSet, String> {
        Self::validate_lengths(valid_from, valid_to)?;
        let matches =
            valid_from
                .iter()
                .zip(valid_to)
                .enumerate()
                .filter_map(|(index, (start, end))| {
                    timestamp_contains(*start, *end, instant).then_some(index as u32)
                });
        Ok(RoaringCandidateSet::from_unsorted(matches))
    }

    pub fn valid_at_candidates(
        valid_from: &[i64],
        valid_to: &[Option<i64>],
        instant: i64,
        candidates: &RoaringCandidateSet,
    ) -> Result<RoaringCandidateSet, String> {
        Self::validate_lengths(valid_from, valid_to)?;
        let matches = candidates.iter().copied().filter(|candidate| {
            let index = *candidate as usize;
            index < valid_from.len()
                && timestamp_contains(valid_from[index], valid_to[index], instant)
        });
        Ok(RoaringCandidateSet::from_unsorted(matches))
    }

    fn validate_lengths(valid_from: &[i64], valid_to: &[Option<i64>]) -> Result<(), String> {
        if valid_from.len() != valid_to.len() {
            return Err("valid_from and valid_to must have the same length".to_string());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReachedNode {
    pub node: u32,
    pub depth: usize,
    pub parent: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathExpansion {
    pub start: u32,
    pub reached: Vec<ReachedNode>,
}

pub struct BatchBfsKernel;

impl BatchBfsKernel {
    pub fn expand(graph: &CsrGraph, starts: &[u32], max_depth: usize) -> Vec<PathExpansion> {
        starts
            .iter()
            .copied()
            .filter(|start| (*start as usize) < graph.node_count())
            .map(|start| expand_one(graph, start, max_depth))
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageRankConfig {
    pub damping: f32,
    pub iterations: usize,
    pub tolerance: f32,
    pub top_k: usize,
}

impl Default for PageRankConfig {
    fn default() -> Self {
        Self {
            damping: 0.85,
            iterations: 16,
            tolerance: 0.000_01,
            top_k: 10,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Personalization {
    weights: Vec<(u32, f32)>,
}

impl Personalization {
    pub fn single_seed(node: u32) -> Self {
        Self {
            weights: vec![(node, 1.0)],
        }
    }

    pub fn weighted(weights: Vec<(u32, f32)>) -> Result<Self, String> {
        if weights.is_empty() {
            return Err("personalization requires at least one seed".to_string());
        }
        if weights.iter().any(|(_, weight)| *weight <= 0.0) {
            return Err("personalization weights must be positive".to_string());
        }
        Ok(Self { weights })
    }

    pub fn seed_nodes(&self) -> Vec<u32> {
        let mut nodes = self
            .weights
            .iter()
            .map(|(node, _)| *node)
            .collect::<Vec<_>>();
        nodes.sort_unstable();
        nodes.dedup();
        nodes
    }

    fn normalized(&self, node_count: usize) -> Vec<f32> {
        let mut output = vec![0.0; node_count];
        let total = self
            .weights
            .iter()
            .filter(|(node, _)| (*node as usize) < node_count)
            .map(|(_, weight)| *weight)
            .sum::<f32>();
        if total <= f32::EPSILON {
            return output;
        }
        for (node, weight) in &self.weights {
            let index = *node as usize;
            if index < node_count {
                output[index] += *weight / total;
            }
        }
        output
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeScore {
    pub node: u32,
    pub score: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PageRankResult {
    pub seed_nodes: Vec<u32>,
    pub top_scores: Vec<NodeScore>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PersonalizedPageRankKernel {
    config: PageRankConfig,
}

impl PersonalizedPageRankKernel {
    pub fn new(config: PageRankConfig) -> Self {
        Self { config }
    }

    pub fn rank_batch(
        &self,
        graph: &CsrGraph,
        personalizations: &[Personalization],
    ) -> Vec<PageRankResult> {
        personalizations
            .iter()
            .map(|personalization| self.rank_one(graph, personalization))
            .collect()
    }

    fn rank_one(&self, graph: &CsrGraph, personalization: &Personalization) -> PageRankResult {
        let node_count = graph.node_count();
        let teleport = personalization.normalized(node_count);
        let mut ranks = teleport.clone();
        let damping = self.config.damping.clamp(0.0, 1.0);
        let restart = 1.0 - damping;

        for _ in 0..self.config.iterations {
            let mut next = teleport
                .iter()
                .map(|weight| restart * *weight)
                .collect::<Vec<_>>();

            for (node, rank) in ranks.iter().copied().enumerate().take(node_count) {
                if rank <= f32::EPSILON {
                    continue;
                }
                let neighbors = graph.neighbors(node as u32).unwrap_or(&[]);
                if neighbors.is_empty() {
                    for (target, weight) in teleport.iter().enumerate() {
                        next[target] += damping * rank * *weight;
                    }
                } else {
                    let contribution = damping * rank / neighbors.len() as f32;
                    for target in neighbors {
                        next[*target as usize] += contribution;
                    }
                }
            }

            let delta = ranks
                .iter()
                .zip(&next)
                .map(|(left, right)| (*left - *right).abs())
                .sum::<f32>();
            ranks = next;
            if delta <= self.config.tolerance {
                break;
            }
        }

        let mut top_scores = ranks
            .into_iter()
            .enumerate()
            .map(|(node, score)| NodeScore {
                node: node as u32,
                score,
            })
            .collect::<Vec<_>>();
        top_scores.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.node.cmp(&right.node))
        });
        top_scores.truncate(self.config.top_k.min(top_scores.len()));

        PageRankResult {
            seed_nodes: personalization.seed_nodes(),
            top_scores,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccelerationBackend {
    Cpu,
    Cuda,
    Metal,
    VulkanWgpu,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GpuExperiment {
    pub backend: AccelerationBackend,
    pub requires_benchmark_evidence: bool,
}

impl GpuExperiment {
    pub fn available() -> Vec<Self> {
        let mut experiments = Vec::new();
        if cfg!(feature = "cuda") {
            experiments.push(Self::new(AccelerationBackend::Cuda));
        }
        if cfg!(feature = "metal") {
            experiments.push(Self::new(AccelerationBackend::Metal));
        }
        if cfg!(feature = "vulkan-wgpu") {
            experiments.push(Self::new(AccelerationBackend::VulkanWgpu));
        }
        experiments
    }

    fn new(backend: AccelerationBackend) -> Self {
        Self {
            backend,
            requires_benchmark_evidence: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KernelBenchmarkEvidence {
    pub cpu_p95_micros: u64,
    pub accelerated_p95_micros: u64,
    pub minimum_speedup: f32,
}

impl KernelBenchmarkEvidence {
    pub fn observed_speedup(self) -> f32 {
        if self.accelerated_p95_micros == 0 {
            return f32::INFINITY;
        }
        self.cpu_p95_micros as f32 / self.accelerated_p95_micros as f32
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuAccelerationPolicy {
    allow_without_benchmark: bool,
}

impl GpuAccelerationPolicy {
    pub fn cpu_first() -> Self {
        Self {
            allow_without_benchmark: false,
        }
    }

    pub fn allows_gpu_without_benchmark(self) -> bool {
        self.allow_without_benchmark
    }

    pub fn approves_gpu(self, evidence: &KernelBenchmarkEvidence) -> bool {
        self.allow_without_benchmark || evidence.observed_speedup() >= evidence.minimum_speedup
    }
}

fn timestamp_contains(start: i64, end: Option<i64>, instant: i64) -> bool {
    instant >= start && end.map_or(true, |end| instant < end)
}

fn expand_one(graph: &CsrGraph, start: u32, max_depth: usize) -> PathExpansion {
    let mut visited = BTreeSet::new();
    let mut parent = BTreeMap::new();
    let mut depths = BTreeMap::new();
    let mut queue = VecDeque::new();

    visited.insert(start);
    depths.insert(start, 0);
    queue.push_back(start);

    while let Some(node) = queue.pop_front() {
        let depth = depths[&node];
        if depth == max_depth {
            continue;
        }
        for neighbor in graph.neighbors(node).unwrap_or(&[]) {
            if visited.insert(*neighbor) {
                parent.insert(*neighbor, node);
                depths.insert(*neighbor, depth + 1);
                queue.push_back(*neighbor);
            }
        }
    }

    let mut reached = depths
        .into_iter()
        .map(|(node, depth)| ReachedNode {
            node,
            depth,
            parent: parent.get(&node).copied(),
        })
        .collect::<Vec<_>>();
    reached.sort_by(|left, right| {
        left.depth
            .cmp(&right.depth)
            .then_with(|| left.node.cmp(&right.node))
    });

    PathExpansion { start, reached }
}
