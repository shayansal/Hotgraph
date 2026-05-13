//! Deterministic fixture AI providers for tests and local demos.
//!
//! These providers are deliberately tiny and rule-based. They are useful for
//! repeatable API tests, but they are not production model integrations.

pub trait QuestionEmbeddingProvider {
    fn embed_question(&self, question: &str) -> Vec<f32>;
}

pub trait ContextPackIntentProvider {
    fn infer_predicate(&self, question: &str, embedding: &[f32]) -> Option<String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FixtureQuestionEmbeddingProvider;

impl QuestionEmbeddingProvider for FixtureQuestionEmbeddingProvider {
    fn embed_question(&self, question: &str) -> Vec<f32> {
        let normalized = question.to_ascii_lowercase();
        if contains_any(&normalized, &["work", "employ", "job"]) {
            vec![1.0, 0.0, 0.0, 0.0]
        } else if contains_any(&normalized, &["located", "location", "based", "where is"]) {
            vec![0.0, 1.0, 0.0, 0.0]
        } else if contains_any(&normalized, &["supply", "supplier", "chain"]) {
            vec![0.0, 0.0, 1.0, 0.0]
        } else if contains_any(&normalized, &["memory", "remember", "preference"]) {
            vec![0.0, 0.0, 0.0, 1.0]
        } else {
            vec![0.25, 0.25, 0.25, 0.25]
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FixtureContextPackIntentProvider;

impl ContextPackIntentProvider for FixtureContextPackIntentProvider {
    fn infer_predicate(&self, question: &str, embedding: &[f32]) -> Option<String> {
        let normalized = question.to_ascii_lowercase();
        if contains_any(&normalized, &["work", "worked", "employ", "job"]) {
            Some("WORKED_AT".to_owned())
        } else if contains_any(&normalized, &["ceo", "chief executive"]) {
            Some("CEO_OF".to_owned())
        } else if contains_any(&normalized, &["own", "owns", "ownership", "acquired"]) {
            Some("OWNS".to_owned())
        } else if contains_any(&normalized, &["supply", "supplier", "chain"]) {
            Some("SUPPLIES".to_owned())
        } else if contains_any(&normalized, &["located", "location", "based", "where is"]) {
            Some("LOCATED_IN".to_owned())
        } else if embedding.first().copied() == Some(1.0) {
            Some("WORKED_AT".to_owned())
        } else if embedding.get(1).copied() == Some(1.0) {
            Some("LOCATED_IN".to_owned())
        } else if embedding.get(2).copied() == Some(1.0) {
            Some("SUPPLIES".to_owned())
        } else {
            None
        }
    }
}

#[deprecated(
    since = "0.1.0",
    note = "use FixtureQuestionEmbeddingProvider; this is a deterministic fixture, not production AI"
)]
pub type DeterministicQuestionEmbeddingProvider = FixtureQuestionEmbeddingProvider;

#[deprecated(
    since = "0.1.0",
    note = "use FixtureContextPackIntentProvider; this is a deterministic fixture, not production AI"
)]
pub type DeterministicContextPackModelProvider = FixtureContextPackIntentProvider;

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}
