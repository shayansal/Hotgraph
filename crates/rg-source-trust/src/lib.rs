//! Source trust and reputation models for Reality Graph.

use std::collections::{BTreeMap, BTreeSet};

use rg_core::{Confidence, SourceId, SourceType, TxTime};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceIdentity {
    pub source_id: SourceId,
    pub source_type: SourceType,
    pub issuer: String,
    pub domain: Option<String>,
    pub signature_key_id: Option<String>,
    pub signature_verified: bool,
    pub issuer_verified: bool,
}

impl SourceIdentity {
    pub fn new(source_id: SourceId, source_type: SourceType, issuer: impl Into<String>) -> Self {
        Self {
            source_id,
            source_type,
            issuer: issuer.into(),
            domain: None,
            signature_key_id: None,
            signature_verified: false,
            issuer_verified: false,
        }
    }

    pub fn with_domain(mut self, domain: impl Into<String>) -> Self {
        self.domain = Some(domain.into());
        self
    }

    pub fn with_signature(mut self, key_id: impl Into<String>, verified: bool) -> Self {
        self.signature_key_id = Some(key_id.into());
        self.signature_verified = verified;
        self
    }

    pub fn with_issuer_verified(mut self, verified: bool) -> Self {
        self.issuer_verified = verified;
        self
    }

    pub fn identity_score(&self) -> f32 {
        let type_score = source_type_baseline(&self.source_type);
        let signature_score = if self.signature_verified { 1.0 } else { 0.0 };
        let issuer_score = if self.issuer_verified { 1.0 } else { 0.25 };
        let domain_score = if self.domain.is_some() { 0.8 } else { 0.35 };
        bounded(
            type_score * 0.2 + signature_score * 0.35 + issuer_score * 0.3 + domain_score * 0.15,
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceAuthority {
    pub domain: String,
    domain_authority: f32,
    human_rating: f32,
    source_type_weights: BTreeMap<String, f32>,
    issuer_authority: BTreeMap<String, f32>,
}

impl SourceAuthority {
    pub fn new(domain: impl Into<String>) -> Self {
        Self {
            domain: domain.into(),
            domain_authority: 0.5,
            human_rating: 0.5,
            source_type_weights: BTreeMap::new(),
            issuer_authority: BTreeMap::new(),
        }
    }

    pub fn with_domain_authority(mut self, authority: f32) -> Self {
        self.domain_authority = bounded(authority);
        self
    }

    pub fn with_human_rating(mut self, rating: f32) -> Self {
        self.human_rating = bounded(rating);
        self
    }

    pub fn with_source_type_weight(mut self, source_type: SourceType, weight: f32) -> Self {
        self.source_type_weights
            .insert(source_type_key(&source_type), bounded(weight));
        self
    }

    pub fn with_issuer_authority(mut self, issuer: impl Into<String>, authority: f32) -> Self {
        self.issuer_authority
            .insert(issuer.into(), bounded(authority));
        self
    }

    pub fn authority_score(&self, issuer: &str, source_type: &SourceType) -> f32 {
        let issuer_authority = self.issuer_authority.get(issuer).copied().unwrap_or(0.45);
        let source_type_weight = self
            .source_type_weights
            .get(&source_type_key(source_type))
            .copied()
            .unwrap_or_else(|| source_type_baseline(source_type));
        bounded(
            self.domain_authority * 0.35
                + issuer_authority * 0.3
                + source_type_weight * 0.2
                + self.human_rating * 0.15,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrustUpdateKind {
    AccurateClaim,
    InaccurateClaim,
    ConflictObserved,
    TamperEvidence,
    HumanUpvote,
    HumanDownvote,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustUpdateEvent {
    pub source_id: SourceId,
    pub transaction_time: TxTime,
    pub kind: TrustUpdateKind,
}

impl TrustUpdateEvent {
    pub fn new(source_id: SourceId, transaction_time: TxTime, kind: TrustUpdateKind) -> Self {
        Self {
            source_id,
            transaction_time,
            kind,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceReputation {
    pub source_id: SourceId,
    first_seen: TxTime,
    last_seen: TxTime,
    accurate_claims: u32,
    inaccurate_claims: u32,
    conflicts: u32,
    tamper_events: u32,
    human_rating_delta: f32,
}

impl SourceReputation {
    pub fn new(source_id: SourceId, first_seen: TxTime) -> Self {
        Self {
            source_id,
            first_seen,
            last_seen: first_seen,
            accurate_claims: 0,
            inaccurate_claims: 0,
            conflicts: 0,
            tamper_events: 0,
            human_rating_delta: 0.0,
        }
    }

    pub fn with_observations(
        mut self,
        accurate_claims: u32,
        inaccurate_claims: u32,
        conflicts: u32,
    ) -> Self {
        self.accurate_claims = accurate_claims;
        self.inaccurate_claims = inaccurate_claims;
        self.conflicts = conflicts;
        self
    }

    pub fn apply(&mut self, event: TrustUpdateEvent) {
        if event.source_id != self.source_id {
            return;
        }
        self.last_seen = event.transaction_time;
        match event.kind {
            TrustUpdateKind::AccurateClaim => self.accurate_claims += 1,
            TrustUpdateKind::InaccurateClaim => self.inaccurate_claims += 1,
            TrustUpdateKind::ConflictObserved => self.conflicts += 1,
            TrustUpdateKind::TamperEvidence => self.tamper_events += 1,
            TrustUpdateKind::HumanUpvote => self.human_rating_delta += 0.05,
            TrustUpdateKind::HumanDownvote => self.human_rating_delta -= 0.05,
        }
        self.human_rating_delta = self.human_rating_delta.clamp(-0.25, 0.25);
    }

    pub fn historical_accuracy(&self) -> f32 {
        let total = self.accurate_claims + self.inaccurate_claims + self.conflicts;
        if total == 0 {
            0.5
        } else {
            self.accurate_claims as f32 / total as f32
        }
    }

    pub fn conflict_rate(&self) -> f32 {
        let total = self.accurate_claims + self.inaccurate_claims + self.conflicts;
        if total == 0 {
            0.0
        } else {
            self.conflicts as f32 / total as f32
        }
    }

    pub fn tamper_penalty(&self) -> f32 {
        (self.tamper_events as f32 * 0.25).min(1.0)
    }

    pub fn human_rating_delta(&self) -> f32 {
        self.human_rating_delta
    }

    pub fn recency_score(&self, now: TxTime) -> f32 {
        let age = (now.as_i64() - self.last_seen.as_i64()).max(0) as f32;
        bounded(1.0 / (1.0 + age / 1_000.0))
    }

    pub fn age_span(&self) -> i64 {
        self.last_seen.as_i64() - self.first_seen.as_i64()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CorroborationGraph {
    claim_support: BTreeMap<SourceId, BTreeSet<SourceId>>,
    shared_issuers: BTreeSet<(SourceId, SourceId)>,
}

impl CorroborationGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_support(&mut self, source_id: SourceId, claim_id: SourceId) {
        self.claim_support
            .entry(claim_id)
            .or_default()
            .insert(source_id);
    }

    pub fn add_shared_issuer(&mut self, left: SourceId, right: SourceId) {
        self.shared_issuers.insert(pair(left, right));
    }

    pub fn corroborating_sources(&self, claim_id: &SourceId) -> Vec<SourceId> {
        self.claim_support
            .get(claim_id)
            .map(|sources| sources.iter().cloned().collect())
            .unwrap_or_default()
    }

    fn shared_issuer_pairs(&self, sources: &[SourceId]) -> usize {
        let source_set = sources.iter().cloned().collect::<BTreeSet<_>>();
        self.shared_issuers
            .iter()
            .filter(|(left, right)| source_set.contains(left) && source_set.contains(right))
            .count()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct IndependenceScore {
    pub score: f32,
    pub explanation: String,
}

impl IndependenceScore {
    pub fn from_corroboration(graph: &CorroborationGraph, sources: &[SourceId]) -> Self {
        let unique_count = sources.iter().collect::<BTreeSet<_>>().len();
        let shared_pairs = graph.shared_issuer_pairs(sources);
        let base = if unique_count <= 1 {
            0.35
        } else {
            (0.45 + unique_count as f32 * 0.18).min(1.0)
        };
        let penalty = shared_pairs as f32 * 0.12;
        let score = bounded(base - penalty);
        let explanation = if shared_pairs == 0 {
            format!("{unique_count} independent source(s) with no shared issuer evidence")
        } else {
            format!("{unique_count} source(s) with {shared_pairs} shared issuer relationship(s)")
        };
        Self { score, explanation }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrustFactor {
    pub name: String,
    pub value: f32,
    pub weight: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceTrustScore {
    pub source_id: SourceId,
    pub score: f32,
    pub factors: Vec<TrustFactor>,
    pub explanation: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrustPolicy {
    pub source_type_weight: f32,
    pub issuer_identity_weight: f32,
    pub cryptographic_signature_weight: f32,
    pub historical_accuracy_weight: f32,
    pub domain_authority_weight: f32,
    pub recency_weight: f32,
    pub independence_weight: f32,
    pub corroboration_weight: f32,
    pub conflict_penalty_weight: f32,
    pub tamper_penalty_weight: f32,
    pub human_rating_weight: f32,
}

impl Default for TrustPolicy {
    fn default() -> Self {
        Self {
            source_type_weight: 0.08,
            issuer_identity_weight: 0.12,
            cryptographic_signature_weight: 0.12,
            historical_accuracy_weight: 0.16,
            domain_authority_weight: 0.14,
            recency_weight: 0.08,
            independence_weight: 0.1,
            corroboration_weight: 0.08,
            conflict_penalty_weight: 0.06,
            tamper_penalty_weight: 0.04,
            human_rating_weight: 0.02,
        }
    }
}

impl TrustPolicy {
    pub fn score_source(
        &self,
        identity: &SourceIdentity,
        authority: &SourceAuthority,
        reputation: &SourceReputation,
        independence: IndependenceScore,
        now: TxTime,
    ) -> SourceTrustScore {
        let source_type = source_type_baseline(&identity.source_type);
        let issuer_identity = if identity.issuer_verified { 1.0 } else { 0.25 };
        let cryptographic_signature = if identity.signature_verified {
            1.0
        } else {
            0.0
        };
        let historical_accuracy = reputation.historical_accuracy();
        let domain_authority = authority.authority_score(&identity.issuer, &identity.source_type);
        let recency = reputation.recency_score(now);
        let corroboration = corroboration_from_independence(independence.score);
        let conflict_rate = reputation.conflict_rate();
        let tamper_penalty = reputation.tamper_penalty();
        let human_rating = bounded(0.5 + reputation.human_rating_delta());

        let factors = vec![
            factor("source_type", source_type, self.source_type_weight),
            factor(
                "issuer_identity",
                issuer_identity,
                self.issuer_identity_weight,
            ),
            factor(
                "cryptographic_signature",
                cryptographic_signature,
                self.cryptographic_signature_weight,
            ),
            factor(
                "historical_accuracy",
                historical_accuracy,
                self.historical_accuracy_weight,
            ),
            factor(
                "domain_authority",
                domain_authority,
                self.domain_authority_weight,
            ),
            factor("recency", recency, self.recency_weight),
            factor("independence", independence.score, self.independence_weight),
            factor("corroboration", corroboration, self.corroboration_weight),
            factor(
                "conflict_rate",
                1.0 - conflict_rate,
                self.conflict_penalty_weight,
            ),
            factor(
                "tamper_evidence",
                1.0 - tamper_penalty,
                self.tamper_penalty_weight,
            ),
            factor("human_rating", human_rating, self.human_rating_weight),
        ];

        let score = normalize_weighted_score(&factors);
        SourceTrustScore {
            source_id: identity.source_id.clone(),
            score,
            factors,
            explanation: format!(
                "source {} scored {:.2}; {}",
                identity.source_id, score, independence.explanation
            ),
        }
    }

    pub fn belief_confidence(&self, input: BeliefConfidenceInput) -> Confidence {
        let value = bounded(
            input.source_confidence * 0.34
                + input.extraction_confidence.as_f32() * 0.26
                + input.corroboration * 0.18
                + (1.0 - input.contradiction) * 0.14
                + input.temporal_freshness * 0.08,
        );
        Confidence::new(value).expect("bounded confidence score is valid")
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeliefConfidenceInput {
    pub source_confidence: f32,
    pub extraction_confidence: Confidence,
    pub corroboration: f32,
    pub contradiction: f32,
    pub temporal_freshness: f32,
}

fn factor(name: &str, value: f32, weight: f32) -> TrustFactor {
    TrustFactor {
        name: name.to_owned(),
        value: bounded(value),
        weight,
    }
}

fn normalize_weighted_score(factors: &[TrustFactor]) -> f32 {
    let total_weight = factors.iter().map(|factor| factor.weight).sum::<f32>();
    if total_weight == 0.0 {
        return 0.0;
    }
    bounded(
        factors
            .iter()
            .map(|factor| factor.value * factor.weight)
            .sum::<f32>()
            / total_weight,
    )
}

fn source_type_baseline(source_type: &SourceType) -> f32 {
    match source_type {
        SourceType::Document => 0.74,
        SourceType::WebPage => 0.48,
        SourceType::DatabaseRecord => 0.82,
        SourceType::ApiResponse => 0.76,
        SourceType::HumanReport => 0.58,
        SourceType::SensorReading => 0.72,
        SourceType::Custom(_) => 0.5,
    }
}

fn source_type_key(source_type: &SourceType) -> String {
    match source_type {
        SourceType::Document => "document".to_owned(),
        SourceType::WebPage => "web_page".to_owned(),
        SourceType::DatabaseRecord => "database_record".to_owned(),
        SourceType::ApiResponse => "api_response".to_owned(),
        SourceType::HumanReport => "human_report".to_owned(),
        SourceType::SensorReading => "sensor_reading".to_owned(),
        SourceType::Custom(value) => format!("custom:{value}"),
    }
}

fn corroboration_from_independence(independence: f32) -> f32 {
    bounded(0.25 + independence * 0.75)
}

fn pair(left: SourceId, right: SourceId) -> (SourceId, SourceId) {
    if left <= right {
        (left, right)
    } else {
        (right, left)
    }
}

fn bounded(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}
