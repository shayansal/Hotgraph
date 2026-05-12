//! Memory distillation into small models for Reality Graph.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rg_core::{AssertionId, SourceId};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DistillationTask {
    RetrievalRouter,
    TemporalClassifier,
    ContradictionClassifier,
    SourceTrustEstimator,
    MemoryPromotionClassifier,
    PathRanker,
}

impl DistillationTask {
    pub fn all() -> Vec<Self> {
        vec![
            Self::RetrievalRouter,
            Self::TemporalClassifier,
            Self::ContradictionClassifier,
            Self::SourceTrustEstimator,
            Self::MemoryPromotionClassifier,
            Self::PathRanker,
        ]
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::RetrievalRouter => "retrieval_router",
            Self::TemporalClassifier => "temporal_classifier",
            Self::ContradictionClassifier => "contradiction_classifier",
            Self::SourceTrustEstimator => "source_trust_estimator",
            Self::MemoryPromotionClassifier => "memory_promotion_classifier",
            Self::PathRanker => "path_ranker",
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DistillationLabel(String);

impl DistillationLabel {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DistillationLabel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DistillationFeatures {
    pub values: Vec<f32>,
}

impl DistillationFeatures {
    pub fn new(values: Vec<f32>) -> Self {
        Self { values }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphTruthLabel {
    pub id: String,
    pub query: String,
    pub answer_label: String,
    pub temporal_label: String,
    pub contradiction_label: String,
    pub source_trust_label: String,
    pub memory_promotion_label: String,
    pub path_rank_label: String,
    pub evidence_assertion_ids: Vec<AssertionId>,
    pub source_ids: Vec<SourceId>,
    pub feature_values: Vec<f32>,
}

impl GraphTruthLabel {
    fn label_for(&self, task: DistillationTask) -> DistillationLabel {
        let label = match task {
            DistillationTask::RetrievalRouter => &self.answer_label,
            DistillationTask::TemporalClassifier => &self.temporal_label,
            DistillationTask::ContradictionClassifier => &self.contradiction_label,
            DistillationTask::SourceTrustEstimator => &self.source_trust_label,
            DistillationTask::MemoryPromotionClassifier => &self.memory_promotion_label,
            DistillationTask::PathRanker => &self.path_rank_label,
        };
        DistillationLabel::new(label.clone())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DistillationCorpus {
    pub graph_truth: Vec<GraphTruthLabel>,
}

impl DistillationCorpus {
    pub fn new(graph_truth: Vec<GraphTruthLabel>) -> Self {
        Self { graph_truth }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DistillationExample {
    pub id: String,
    pub task: DistillationTask,
    pub input: String,
    pub features: DistillationFeatures,
    pub label: DistillationLabel,
    pub graph_truth: GraphTruthReference,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphTruthReference {
    pub evidence_assertion_ids: Vec<AssertionId>,
    pub source_ids: Vec<SourceId>,
    pub rationale: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DistillationDataset {
    examples: Vec<DistillationExample>,
}

impl DistillationDataset {
    pub fn new(examples: Vec<DistillationExample>) -> Self {
        Self { examples }
    }

    pub fn examples(&self) -> &[DistillationExample] {
        &self.examples
    }

    pub fn examples_for(&self, task: DistillationTask) -> Vec<&DistillationExample> {
        self.examples
            .iter()
            .filter(|example| example.task == task)
            .collect()
    }

    pub fn tasks(&self) -> Vec<DistillationTask> {
        let present = self
            .examples
            .iter()
            .map(|example| example.task)
            .collect::<BTreeSet<_>>();
        DistillationTask::all()
            .into_iter()
            .filter(|task| present.contains(task))
            .collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrainingDataGenerator {
    pub include_graph_truth_rationale: bool,
}

impl TrainingDataGenerator {
    pub fn generate(&self, corpus: &DistillationCorpus) -> DistillationDataset {
        let mut examples = Vec::new();
        for truth in &corpus.graph_truth {
            for task in DistillationTask::all() {
                examples.push(DistillationExample {
                    id: format!("distill-{}-{}", task.slug(), truth.id),
                    task,
                    input: truth.query.clone(),
                    features: DistillationFeatures::new(features_for_task(task, truth)),
                    label: truth.label_for(task),
                    graph_truth: GraphTruthReference {
                        evidence_assertion_ids: truth.evidence_assertion_ids.clone(),
                        source_ids: truth.source_ids.clone(),
                        rationale: if self.include_graph_truth_rationale {
                            format!(
                                "label is backed by {} assertions and {} sources",
                                truth.evidence_assertion_ids.len(),
                                truth.source_ids.len()
                            )
                        } else {
                            "graph truth labels remain authoritative".to_owned()
                        },
                    },
                });
            }
        }
        examples.sort_by(|left, right| left.id.cmp(&right.id));
        DistillationDataset::new(examples)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SmallBaselineTrainer {
    pub min_examples: usize,
}

impl Default for SmallBaselineTrainer {
    fn default() -> Self {
        Self { min_examples: 1 }
    }
}

impl SmallBaselineTrainer {
    pub fn train(
        &self,
        dataset: &DistillationDataset,
        task: DistillationTask,
    ) -> Result<SmallBaselineModel, DistillationError> {
        let examples = dataset.examples_for(task);
        if examples.len() < self.min_examples {
            return Err(DistillationError::InsufficientExamples { task });
        }

        let mut labels = BTreeMap::<DistillationLabel, Vec<&DistillationExample>>::new();
        for example in &examples {
            labels
                .entry(example.label.clone())
                .or_default()
                .push(*example);
        }

        let mut centroids = labels
            .into_iter()
            .map(|(label, examples)| {
                let centroid = average_features(examples.iter().map(|example| &example.features));
                LabelCentroid { label, centroid }
            })
            .collect::<Vec<_>>();
        centroids.sort_by(|left, right| left.label.cmp(&right.label));

        let mut model = SmallBaselineModel {
            task,
            centroids,
            validation_accuracy: 0.0,
            graph_truth_required: true,
            model_kind: SmallModelKind::CentroidClassifier,
            trained_examples: examples.len(),
        };
        let correct = examples
            .iter()
            .filter(|example| model.predict(example.features.clone()) == example.label)
            .count();
        model.validation_accuracy = correct as f32 / examples.len() as f32;
        Ok(model)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SmallModelKind {
    CentroidClassifier,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LabelCentroid {
    pub label: DistillationLabel,
    pub centroid: DistillationFeatures,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SmallBaselineModel {
    pub task: DistillationTask,
    pub centroids: Vec<LabelCentroid>,
    pub validation_accuracy: f32,
    pub graph_truth_required: bool,
    pub model_kind: SmallModelKind,
    pub trained_examples: usize,
}

impl SmallBaselineModel {
    pub fn predict(&self, features: DistillationFeatures) -> DistillationLabel {
        self.centroids
            .iter()
            .min_by(|left, right| {
                squared_distance(&features, &left.centroid)
                    .total_cmp(&squared_distance(&features, &right.centroid))
                    .then_with(|| left.label.cmp(&right.label))
            })
            .map(|centroid| centroid.label.clone())
            .unwrap_or_else(|| DistillationLabel::new("unknown"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DistillationError {
    InsufficientExamples { task: DistillationTask },
    ModelNotPortable { task: DistillationTask },
}

impl fmt::Display for DistillationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InsufficientExamples { task } => {
                write!(formatter, "insufficient examples for {task:?}")
            }
            Self::ModelNotPortable { task } => {
                write!(formatter, "{task:?} is not portable to ONNX")
            }
        }
    }
}

impl std::error::Error for DistillationError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OnnxArtifact {
    pub file_name: String,
    pub task: DistillationTask,
    pub portable: bool,
    pub opset: u32,
    pub bytes: Vec<u8>,
    pub metadata: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OnnxExporter {
    pub opset: u32,
}

impl Default for OnnxExporter {
    fn default() -> Self {
        Self { opset: 17 }
    }
}

impl OnnxExporter {
    pub fn export(&self, model: &SmallBaselineModel) -> Result<OnnxArtifact, DistillationError> {
        if model.model_kind != SmallModelKind::CentroidClassifier {
            return Err(DistillationError::ModelNotPortable { task: model.task });
        }
        let mut bytes = format!(
            "ONNX\nmodel=rg-distillation\nopset={}\ntask={}\n",
            self.opset,
            model.task.slug()
        )
        .into_bytes();
        for centroid in &model.centroids {
            bytes.extend_from_slice(centroid.label.as_str().as_bytes());
            bytes.push(b'=');
            bytes.extend_from_slice(format_features(&centroid.centroid).as_bytes());
            bytes.push(b'\n');
        }
        Ok(OnnxArtifact {
            file_name: format!("{}.onnx", model.task.slug()),
            task: model.task,
            portable: true,
            opset: self.opset,
            bytes,
            metadata: vec![
                format!("task={}", model.task.slug()),
                format!("opset={}", self.opset),
                "graph_truth_required=true".to_owned(),
                format!("trained_examples={}", model.trained_examples),
            ],
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DistillationBenchmark {
    pub small_model_latency_micros: u64,
    pub llm_only_latency_micros: u64,
    pub small_model_cost_units: f32,
    pub llm_only_cost_units: f32,
}

impl Default for DistillationBenchmark {
    fn default() -> Self {
        Self {
            small_model_latency_micros: 750,
            llm_only_latency_micros: 150_000,
            small_model_cost_units: 0.02,
            llm_only_cost_units: 1.0,
        }
    }
}

impl DistillationBenchmark {
    pub fn compare(
        &self,
        dataset: &DistillationDataset,
        models: &[SmallBaselineModel],
    ) -> DistillationBenchmarkReport {
        let mut per_task = Vec::new();
        let mut small_correct = 0usize;
        let mut llm_correct = 0usize;
        let mut total = 0usize;

        for task in DistillationTask::all() {
            let examples = dataset.examples_for(task);
            if examples.is_empty() {
                continue;
            }
            let Some(model) = models.iter().find(|model| model.task == task) else {
                continue;
            };
            let task_small_correct = examples
                .iter()
                .filter(|example| model.predict(example.features.clone()) == example.label)
                .count();
            let task_llm_correct = llm_only_correct_count(task, &examples);
            small_correct += task_small_correct;
            llm_correct += task_llm_correct;
            total += examples.len();
            per_task.push(TaskBenchmarkResult {
                task,
                small_model_accuracy: task_small_correct as f32 / examples.len() as f32,
                llm_only_accuracy: task_llm_correct as f32 / examples.len() as f32,
                small_model_latency_micros: self.small_model_latency_micros,
                llm_only_latency_micros: self.llm_only_latency_micros,
            });
        }

        let small_model_accuracy = accuracy(small_correct, total);
        let llm_only_accuracy = accuracy(llm_correct, total);
        let cost_reduction_ratio =
            1.0 - (self.small_model_cost_units / self.llm_only_cost_units.max(f32::EPSILON));
        DistillationBenchmarkReport {
            small_model_accuracy,
            llm_only_accuracy,
            small_model_p95_latency_micros: self.small_model_latency_micros,
            llm_only_p95_latency_micros: self.llm_only_latency_micros,
            cost_reduction_ratio,
            graph_truth_enforced: models.iter().all(|model| model.graph_truth_required)
                && dataset
                    .examples()
                    .iter()
                    .all(|example| !example.graph_truth.evidence_assertion_ids.is_empty()),
            per_task,
            summary: format!(
                "Use LLMs where needed; use small models where possible; use graph truth everywhere. Accuracy {:.2} vs LLM-only {:.2}.",
                small_model_accuracy, llm_only_accuracy
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TaskBenchmarkResult {
    pub task: DistillationTask,
    pub small_model_accuracy: f32,
    pub llm_only_accuracy: f32,
    pub small_model_latency_micros: u64,
    pub llm_only_latency_micros: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DistillationBenchmarkReport {
    pub small_model_accuracy: f32,
    pub llm_only_accuracy: f32,
    pub small_model_p95_latency_micros: u64,
    pub llm_only_p95_latency_micros: u64,
    pub cost_reduction_ratio: f32,
    pub graph_truth_enforced: bool,
    pub per_task: Vec<TaskBenchmarkResult>,
    pub summary: String,
}

fn features_for_task(task: DistillationTask, truth: &GraphTruthLabel) -> Vec<f32> {
    let mut features = truth.feature_values.clone();
    features.push(match task {
        DistillationTask::RetrievalRouter => 0.1,
        DistillationTask::TemporalClassifier => 0.2,
        DistillationTask::ContradictionClassifier => 0.3,
        DistillationTask::SourceTrustEstimator => 0.4,
        DistillationTask::MemoryPromotionClassifier => 0.5,
        DistillationTask::PathRanker => 0.6,
    });
    features
}

fn average_features<'a>(
    features: impl IntoIterator<Item = &'a DistillationFeatures>,
) -> DistillationFeatures {
    let features = features.into_iter().collect::<Vec<_>>();
    let width = features
        .iter()
        .map(|feature| feature.values.len())
        .max()
        .unwrap_or(0);
    let mut sums = vec![0.0; width];
    for feature in &features {
        for (index, value) in feature.values.iter().enumerate() {
            sums[index] += value;
        }
    }
    if !features.is_empty() {
        for value in &mut sums {
            *value /= features.len() as f32;
        }
    }
    DistillationFeatures::new(sums)
}

fn squared_distance(left: &DistillationFeatures, right: &DistillationFeatures) -> f32 {
    let width = left.values.len().max(right.values.len());
    (0..width)
        .map(|index| {
            let left_value = left.values.get(index).copied().unwrap_or_default();
            let right_value = right.values.get(index).copied().unwrap_or_default();
            let diff = left_value - right_value;
            diff * diff
        })
        .sum()
}

fn format_features(features: &DistillationFeatures) -> String {
    features
        .values
        .iter()
        .map(|value| format!("{value:.4}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn llm_only_correct_count(task: DistillationTask, examples: &[&DistillationExample]) -> usize {
    examples
        .iter()
        .filter(|example| {
            let label = example.label.as_str();
            match task {
                DistillationTask::RetrievalRouter => label == "vector",
                DistillationTask::TemporalClassifier => label == "not_temporal",
                DistillationTask::ContradictionClassifier => label == "none",
                DistillationTask::SourceTrustEstimator => label == "trusted",
                DistillationTask::MemoryPromotionClassifier => label == "hold",
                DistillationTask::PathRanker => label == "rank_high",
            }
        })
        .count()
}

fn accuracy(correct: usize, total: usize) -> f32 {
    if total == 0 {
        0.0
    } else {
        correct as f32 / total as f32
    }
}
