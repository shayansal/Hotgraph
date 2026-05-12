//! Contradiction-aware belief state engine for Reality Graph.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rg_core::{
    Confidence, EntityId, GraphValue, PredicateId, SourceId, TimeInterval, TxTime, ValidTime,
};

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

string_newtype!(ClaimId);
string_newtype!(ConflictSetId);

#[derive(Clone, Debug, PartialEq)]
pub struct Claim {
    pub id: ClaimId,
    pub subject: EntityId,
    pub predicate: PredicateId,
    pub object: GraphValue,
    pub valid_time: TimeInterval<ValidTime>,
    pub transaction_time: TxTime,
    pub confidence: Confidence,
    pub source_ids: Vec<SourceId>,
    pub evidence: String,
}

impl Claim {
    pub fn with_confidence(mut self, confidence: Confidence) -> Self {
        self.confidence = confidence;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BeliefQuery {
    pub subject: EntityId,
    pub predicate: PredicateId,
    pub valid_at: ValidTime,
    pub known_at: TxTime,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BeliefState {
    pub query: BeliefQuery,
    pub preferred_claim: Option<Claim>,
    pub competing_claims: Vec<Claim>,
    pub conflict_sets: Vec<ConflictSet>,
    pub explanation: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BeliefRevision {
    pub transaction_time: TxTime,
    pub subject: EntityId,
    pub predicate: PredicateId,
    pub previous_belief: Option<ClaimId>,
    pub new_belief: Option<ClaimId>,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConflictSet {
    pub id: ConflictSetId,
    pub claim_ids: Vec<ClaimId>,
    pub conflict_type: ConflictType,
    pub resolution_status: ResolutionStatus,
    pub preferred_claim_id: Option<ClaimId>,
    pub explanation: String,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ConflictType {
    MutuallyExclusiveStatus,
    DateMismatch,
    NumericMismatch,
    EntityIdentityMismatch,
    CausalDisagreement,
    SourceTrustDisagreement,
    ValidTimeOverlapConflict,
}

impl ConflictType {
    fn slug(self) -> &'static str {
        match self {
            Self::MutuallyExclusiveStatus => "mutually-exclusive-status",
            Self::DateMismatch => "date-mismatch",
            Self::NumericMismatch => "numeric-mismatch",
            Self::EntityIdentityMismatch => "entity-identity-mismatch",
            Self::CausalDisagreement => "causal-disagreement",
            Self::SourceTrustDisagreement => "source-trust-disagreement",
            Self::ValidTimeOverlapConflict => "valid-time-overlap-conflict",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolutionStatus {
    Preferred,
    Unresolved,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolutionPolicy {
    pub confidence_weight: f32,
    pub source_trust_weight: f32,
    pub recency_weight: f32,
    pub source_trust_disagreement_threshold: f32,
}

impl ResolutionPolicy {
    pub fn trust_weighted() -> Self {
        Self {
            confidence_weight: 0.25,
            source_trust_weight: 0.7,
            recency_weight: 0.05,
            source_trust_disagreement_threshold: 0.5,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceTrustModel {
    default_trust: f32,
    trust_by_source: BTreeMap<SourceId, f32>,
}

impl SourceTrustModel {
    pub fn new(default_trust: f32) -> Self {
        Self {
            default_trust: default_trust.clamp(0.0, 1.0),
            trust_by_source: BTreeMap::new(),
        }
    }

    pub fn set_trust(&mut self, source_id: SourceId, trust: f32) {
        self.trust_by_source
            .insert(source_id, trust.clamp(0.0, 1.0));
    }

    pub fn trust_for(&self, source_id: &SourceId) -> f32 {
        self.trust_by_source
            .get(source_id)
            .copied()
            .unwrap_or(self.default_trust)
    }

    pub fn average_trust(&self, source_ids: &[SourceId]) -> f32 {
        if source_ids.is_empty() {
            return self.default_trust;
        }
        source_ids
            .iter()
            .map(|source_id| self.trust_for(source_id))
            .sum::<f32>()
            / source_ids.len() as f32
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BeliefEngine {
    claims: BTreeMap<ClaimId, Claim>,
    conflict_sets: Vec<ConflictSet>,
    revisions: Vec<BeliefRevision>,
    policy: ResolutionPolicy,
    trust_model: SourceTrustModel,
}

impl BeliefEngine {
    pub fn new(policy: ResolutionPolicy, trust_model: SourceTrustModel) -> Self {
        Self {
            claims: BTreeMap::new(),
            conflict_sets: Vec::new(),
            revisions: Vec::new(),
            policy,
            trust_model,
        }
    }

    pub fn ingest_claim(&mut self, claim: Claim) {
        let previous = self.preferred_for_claim_key(
            &claim.subject,
            &claim.predicate,
            claim.valid_time.start,
            claim.transaction_time,
        );
        let subject = claim.subject.clone();
        let predicate = claim.predicate.clone();
        let valid_at = claim.valid_time.start;
        let transaction_time = claim.transaction_time;
        let source_hint = claim
            .source_ids
            .first()
            .map(ToString::to_string)
            .unwrap_or_else(|| "unknown source".to_owned());

        self.claims.insert(claim.id.clone(), claim);
        self.recompute_conflicts();

        let next = self.preferred_for_claim_key(&subject, &predicate, valid_at, transaction_time);
        if previous.as_ref().map(|claim| &claim.id) != next.as_ref().map(|claim| &claim.id)
            && previous.is_some()
        {
            self.revisions.push(BeliefRevision {
                transaction_time,
                subject,
                predicate,
                previous_belief: previous.map(|claim| claim.id),
                new_belief: next.map(|claim| claim.id),
                reason: format!(
                    "what we believed changed after evidence from {source_hint}; source trust and confidence changed the preferred claim"
                ),
            });
        }
    }

    pub fn conflict_sets(&self) -> Vec<ConflictSet> {
        self.conflict_sets.clone()
    }

    pub fn belief_revisions(&self) -> &[BeliefRevision] {
        &self.revisions
    }

    pub fn belief_state(&self, query: BeliefQuery) -> BeliefState {
        let mut competing_claims = self
            .claims
            .values()
            .filter(|claim| claim_matches_query(claim, &query))
            .cloned()
            .collect::<Vec<_>>();
        competing_claims.sort_by(|left, right| left.id.cmp(&right.id));

        let preferred_claim =
            preferred_claim(competing_claims.iter(), &self.policy, &self.trust_model).cloned();
        let relevant_ids = competing_claims
            .iter()
            .map(|claim| claim.id.clone())
            .collect::<BTreeSet<_>>();
        let conflict_sets = self
            .conflict_sets
            .iter()
            .filter(|set| {
                set.claim_ids
                    .iter()
                    .all(|claim_id| relevant_ids.contains(claim_id))
            })
            .cloned()
            .collect::<Vec<_>>();
        let explanation = preferred_claim.as_ref().map_or_else(
            || "no claim was known for this bitemporal belief query".to_owned(),
            |claim| {
                format!(
                    "{} is preferred for {} at valid {} known {}; {}",
                    claim.id,
                    claim.predicate,
                    query.valid_at.as_i64(),
                    query.known_at.as_i64(),
                    claim_preference_reason(claim, &self.trust_model)
                )
            },
        );

        BeliefState {
            query,
            preferred_claim,
            competing_claims,
            conflict_sets,
            explanation,
        }
    }

    pub fn explain_belief_changes(&self, query: BeliefQuery) -> String {
        let mut matching = self
            .revisions
            .iter()
            .filter(|revision| {
                revision.subject == query.subject
                    && revision.predicate == query.predicate
                    && revision.transaction_time <= query.known_at
            })
            .collect::<Vec<_>>();
        matching.sort_by_key(|revision| revision.transaction_time);

        if matching.is_empty() {
            return "no belief revisions are known for this query".to_owned();
        }

        matching
            .into_iter()
            .map(|revision| {
                format!(
                    "At tx {}, what we believed changed from {} to {} because {}.",
                    revision.transaction_time.as_i64(),
                    option_claim_id(&revision.previous_belief),
                    option_claim_id(&revision.new_belief),
                    revision.reason
                )
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn recompute_conflicts(&mut self) {
        let mut sets = Vec::new();
        let claims = self.claims.values().collect::<Vec<_>>();
        for left_index in 0..claims.len() {
            let left = claims[left_index];
            for right in claims.iter().skip(left_index + 1) {
                if let Some(conflict_type) =
                    classify_conflict(left, right, &self.policy, &self.trust_model)
                {
                    sets.push(build_conflict_set(
                        left,
                        right,
                        conflict_type,
                        &self.policy,
                        &self.trust_model,
                    ));
                }
            }
        }
        sets.sort_by(|left, right| left.id.cmp(&right.id));
        self.conflict_sets = sets;
    }

    fn preferred_for_claim_key(
        &self,
        subject: &EntityId,
        predicate: &PredicateId,
        valid_at: ValidTime,
        known_at: TxTime,
    ) -> Option<Claim> {
        preferred_claim(
            self.claims.values().filter(|claim| {
                &claim.subject == subject
                    && &claim.predicate == predicate
                    && claim.valid_time.contains(valid_at)
                    && claim.transaction_time <= known_at
            }),
            &self.policy,
            &self.trust_model,
        )
        .cloned()
    }
}

fn classify_conflict(
    left: &Claim,
    right: &Claim,
    policy: &ResolutionPolicy,
    trust_model: &SourceTrustModel,
) -> Option<ConflictType> {
    if left.subject != right.subject
        || left.predicate != right.predicate
        || !left.valid_time.overlaps(&right.valid_time)
    {
        return None;
    }

    if left.object == right.object {
        let trust_gap = (trust_model.average_trust(&left.source_ids)
            - trust_model.average_trust(&right.source_ids))
        .abs();
        return (trust_gap >= policy.source_trust_disagreement_threshold)
            .then_some(ConflictType::SourceTrustDisagreement);
    }

    let predicate = left.predicate.as_str().to_ascii_uppercase();
    if predicate.contains("STATUS") || predicate == "STATE" {
        return Some(ConflictType::MutuallyExclusiveStatus);
    }
    if predicate.contains("CAUSE") {
        return Some(ConflictType::CausalDisagreement);
    }
    if matches!(
        (&left.object, &right.object),
        (GraphValue::Time(_), GraphValue::Time(_))
    ) {
        return Some(ConflictType::DateMismatch);
    }
    if is_numeric(&left.object) && is_numeric(&right.object) {
        return Some(ConflictType::NumericMismatch);
    }
    if predicate.contains("IDENTITY") || predicate == "SAME_AS" {
        return Some(ConflictType::EntityIdentityMismatch);
    }
    if matches!(
        (&left.object, &right.object),
        (GraphValue::Entity(_), GraphValue::Entity(_))
    ) {
        return Some(ConflictType::ValidTimeOverlapConflict);
    }

    Some(ConflictType::ValidTimeOverlapConflict)
}

fn build_conflict_set(
    left: &Claim,
    right: &Claim,
    conflict_type: ConflictType,
    policy: &ResolutionPolicy,
    trust_model: &SourceTrustModel,
) -> ConflictSet {
    let mut claim_ids = vec![left.id.clone(), right.id.clone()];
    claim_ids.sort();
    let preferred = preferred_claim([left, right], policy, trust_model);
    let preferred_claim_id = preferred.map(|claim| claim.id.clone());
    let explanation = preferred.map_or_else(
        || format!("{} conflict is unresolved", conflict_type.slug()),
        |claim| {
            format!(
                "{} is preferred in this {} conflict because {}; source trust comparison: {}={:.2}, {}={:.2}",
                claim.id,
                conflict_type.slug(),
                claim_preference_reason(claim, trust_model),
                left.id,
                trust_model.average_trust(&left.source_ids),
                right.id,
                trust_model.average_trust(&right.source_ids)
            )
        },
    );
    ConflictSet {
        id: conflict_id(&claim_ids, conflict_type),
        claim_ids,
        conflict_type,
        resolution_status: preferred_claim_id
            .as_ref()
            .map_or(ResolutionStatus::Unresolved, |_| {
                ResolutionStatus::Preferred
            }),
        preferred_claim_id,
        explanation,
    }
}

fn preferred_claim<'a>(
    claims: impl IntoIterator<Item = &'a Claim>,
    policy: &ResolutionPolicy,
    trust_model: &SourceTrustModel,
) -> Option<&'a Claim> {
    claims.into_iter().max_by(|left, right| {
        claim_score(left, policy, trust_model)
            .total_cmp(&claim_score(right, policy, trust_model))
            .then_with(|| left.transaction_time.cmp(&right.transaction_time))
            .then_with(|| right.id.cmp(&left.id))
    })
}

fn claim_score(claim: &Claim, policy: &ResolutionPolicy, trust_model: &SourceTrustModel) -> f32 {
    let trust = trust_model.average_trust(&claim.source_ids);
    let recency = (claim.transaction_time.as_i64().max(0) as f32 / 1_000_000.0).min(1.0);
    (claim.confidence.as_f32() * policy.confidence_weight)
        + (trust * policy.source_trust_weight)
        + (recency * policy.recency_weight)
}

fn claim_preference_reason(claim: &Claim, trust_model: &SourceTrustModel) -> String {
    format!(
        "source trust {:.2}, confidence {:.2}, sources {}",
        trust_model.average_trust(&claim.source_ids),
        claim.confidence.as_f32(),
        claim
            .source_ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn claim_matches_query(claim: &Claim, query: &BeliefQuery) -> bool {
    claim.subject == query.subject
        && claim.predicate == query.predicate
        && claim.valid_time.contains(query.valid_at)
        && claim.transaction_time <= query.known_at
}

fn conflict_id(claim_ids: &[ClaimId], conflict_type: ConflictType) -> ConflictSetId {
    ConflictSetId::new(format!(
        "conflict-{}-{}-{}",
        claim_ids[0],
        claim_ids[1],
        conflict_type.slug()
    ))
}

fn is_numeric(value: &GraphValue) -> bool {
    matches!(value, GraphValue::Integer(_) | GraphValue::Decimal(_))
}

fn option_claim_id(claim_id: &Option<ClaimId>) -> String {
    claim_id
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_else(|| "nothing".to_owned())
}
