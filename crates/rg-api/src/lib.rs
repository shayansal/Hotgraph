//! Public surface for the Hotgraph HTTP API crate.
//!
//! Production API code is separated from deterministic fixture providers so
//! mock AI behavior cannot be mistaken for a real model integration.

mod api;
pub mod fixture_ai;

pub use api::*;
pub use fixture_ai::{
    ContextPackIntentProvider, FixtureContextPackIntentProvider, FixtureQuestionEmbeddingProvider,
    QuestionEmbeddingProvider,
};
