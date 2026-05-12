//! Temporal reasoning primitives for Reality Graph.

use rg_ai::EvidencePack;
use rg_core::{Assertion, AssertionId, AssertionStatus, TimeInterval, TxTime, ValidTime};
use rg_query::{PathQuery, PathResult, QueryEngine, QueryResult};
use rg_storage::InMemoryStorage;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AllenRelation {
    Before,
    After,
    Meets,
    Overlaps,
    During,
    Contains,
    Starts,
    Finishes,
    Equals,
}

impl AllenRelation {
    pub fn classify(
        left: &TimeInterval<ValidTime>,
        right: &TimeInterval<ValidTime>,
    ) -> Option<Self> {
        if equals(left, right) {
            Some(Self::Equals)
        } else if before(left, right) {
            Some(Self::Before)
        } else if after(left, right) {
            Some(Self::After)
        } else if meets(left, right) {
            Some(Self::Meets)
        } else if starts(left, right) {
            Some(Self::Starts)
        } else if finishes(left, right) {
            Some(Self::Finishes)
        } else if during(left, right) {
            Some(Self::During)
        } else if contains(left, right) {
            Some(Self::Contains)
        } else if overlaps(left, right) {
            Some(Self::Overlaps)
        } else {
            None
        }
    }
}

pub fn before(left: &TimeInterval<ValidTime>, right: &TimeInterval<ValidTime>) -> bool {
    left.end
        .map(|left_end| left_end < right.start)
        .unwrap_or(false)
}

pub fn after(left: &TimeInterval<ValidTime>, right: &TimeInterval<ValidTime>) -> bool {
    before(right, left)
}

pub fn meets(left: &TimeInterval<ValidTime>, right: &TimeInterval<ValidTime>) -> bool {
    left.end == Some(right.start)
}

pub fn overlaps(left: &TimeInterval<ValidTime>, right: &TimeInterval<ValidTime>) -> bool {
    left.start < right.start
        && end_after_start(left.end, right.start)
        && end_before_end(left.end, right.end)
}

pub fn during(left: &TimeInterval<ValidTime>, right: &TimeInterval<ValidTime>) -> bool {
    right.start < left.start && end_before_end(left.end, right.end)
}

pub fn contains(left: &TimeInterval<ValidTime>, right: &TimeInterval<ValidTime>) -> bool {
    during(right, left)
}

pub fn starts(left: &TimeInterval<ValidTime>, right: &TimeInterval<ValidTime>) -> bool {
    left.start == right.start && end_before_end(left.end, right.end)
}

pub fn finishes(left: &TimeInterval<ValidTime>, right: &TimeInterval<ValidTime>) -> bool {
    right.start < left.start && left.end == right.end
}

pub fn equals(left: &TimeInterval<ValidTime>, right: &TimeInterval<ValidTime>) -> bool {
    left.start == right.start && left.end == right.end
}

pub fn valid_at(storage: &InMemoryStorage, instant: ValidTime) -> Vec<Assertion> {
    sorted_assertions(
        storage
            .graph_state()
            .assertions
            .values()
            .filter(|assertion| assertion.status == AssertionStatus::Active)
            .filter(|assertion| assertion.valid_time.contains(instant))
            .cloned(),
    )
}

pub fn known_at(storage: &InMemoryStorage, instant: TxTime) -> Vec<Assertion> {
    sorted_assertions(
        storage
            .graph_state()
            .assertions
            .values()
            .filter(|assertion| assertion.transaction_time.contains(instant))
            .cloned(),
    )
}

pub fn changed_between(storage: &InMemoryStorage, start: TxTime, end: TxTime) -> Vec<Assertion> {
    sorted_assertions_by_tx(
        storage
            .graph_state()
            .assertions
            .values()
            .filter(|assertion| {
                tx_in_range(assertion.transaction_time.start, start, end)
                    || assertion
                        .transaction_time
                        .end
                        .is_some_and(|tx_end| tx_in_range(tx_end, start, end))
            })
            .cloned(),
    )
}

pub fn active_during(
    storage: &InMemoryStorage,
    interval: &TimeInterval<ValidTime>,
) -> Vec<Assertion> {
    sorted_assertions(
        storage
            .graph_state()
            .assertions
            .values()
            .filter(|assertion| assertion.status == AssertionStatus::Active)
            .filter(|assertion| assertion.valid_time.overlaps(interval))
            .cloned(),
    )
}

pub fn superseded_after(storage: &InMemoryStorage, after_tx: TxTime) -> Vec<Assertion> {
    sorted_assertions_by_tx(
        storage
            .graph_state()
            .assertions
            .values()
            .filter(|assertion| assertion.status != AssertionStatus::Active)
            .filter(|assertion| {
                assertion
                    .transaction_time
                    .end
                    .is_some_and(|tx_end| tx_end > after_tx)
                    || assertion.transaction_time.start > after_tx
            })
            .cloned(),
    )
}

pub struct TemporalReasoner<'a> {
    storage: &'a InMemoryStorage,
}

impl<'a> TemporalReasoner<'a> {
    pub fn new(storage: &'a InMemoryStorage) -> Self {
        Self { storage }
    }

    pub fn valid_at(&self, instant: ValidTime) -> Vec<Assertion> {
        valid_at(self.storage, instant)
    }

    pub fn known_at(&self, instant: TxTime) -> Vec<Assertion> {
        known_at(self.storage, instant)
    }

    pub fn changed_between(&self, start: TxTime, end: TxTime) -> Vec<Assertion> {
        changed_between(self.storage, start, end)
    }

    pub fn active_during(&self, interval: &TimeInterval<ValidTime>) -> Vec<Assertion> {
        active_during(self.storage, interval)
    }

    pub fn superseded_after(&self, after_tx: TxTime) -> Vec<Assertion> {
        superseded_after(self.storage, after_tx)
    }

    pub fn was_true_until(&self, assertion_id: &AssertionId) -> Option<ValidTime> {
        self.storage
            .graph_state()
            .assertions
            .get(assertion_id)
            .and_then(|assertion| assertion.valid_time.end)
    }

    pub fn became_false_when(&self, assertion_id: &AssertionId) -> Option<TxTime> {
        self.storage
            .graph_state()
            .assertions
            .get(assertion_id)
            .and_then(|assertion| assertion.transaction_time.end)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TemporalPathResult {
    pub path: PathResult,
    pub temporal_explanation: String,
}

pub struct TemporalPathReasoner<'a> {
    storage: &'a InMemoryStorage,
}

impl<'a> TemporalPathReasoner<'a> {
    pub fn new(storage: &'a InMemoryStorage) -> Self {
        Self { storage }
    }

    pub fn paths_active_during(
        &self,
        query: PathQuery,
        interval: &TimeInterval<ValidTime>,
    ) -> Vec<TemporalPathResult> {
        let engine = QueryEngine::from_storage(self.storage.clone());
        let mut paths = engine
            .execute_path(query)
            .into_iter()
            .filter(|path| path_active_during(path, interval))
            .map(|path| TemporalPathResult {
                temporal_explanation: path_temporal_explanation(&path, interval),
                path,
            })
            .collect::<Vec<_>>();
        paths.sort_by(|left, right| {
            left.path
                .hops
                .iter()
                .map(|hop| hop.assertion_id.as_str())
                .cmp(right.path.hops.iter().map(|hop| hop.assertion_id.as_str()))
        });
        paths
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TemporalEvidenceExplanation {
    pub assertion_id: AssertionId,
    pub explanation: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TemporalEvidenceExplainer {
    valid_at: ValidTime,
    known_at: TxTime,
}

impl TemporalEvidenceExplainer {
    pub fn new(valid_at: ValidTime, known_at: TxTime) -> Self {
        Self { valid_at, known_at }
    }

    pub fn explain_pack(&self, pack: &EvidencePack) -> Vec<TemporalEvidenceExplanation> {
        let mut explanations = pack
            .assertions
            .iter()
            .map(|assertion| TemporalEvidenceExplanation {
                assertion_id: assertion.id.clone(),
                explanation: assertion_temporal_explanation(
                    assertion,
                    self.valid_at,
                    self.known_at,
                ),
            })
            .collect::<Vec<_>>();
        explanations.sort_by(|left, right| left.assertion_id.cmp(&right.assertion_id));
        explanations
    }
}

fn end_after_start(end: Option<ValidTime>, start: ValidTime) -> bool {
    end.map(|end| start < end).unwrap_or(true)
}

fn end_before_end(left: Option<ValidTime>, right: Option<ValidTime>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left < right,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

fn tx_in_range(value: TxTime, start: TxTime, end: TxTime) -> bool {
    start <= value && value <= end
}

fn sorted_assertions(assertions: impl Iterator<Item = Assertion>) -> Vec<Assertion> {
    let mut assertions = assertions.collect::<Vec<_>>();
    assertions.sort_by(|left, right| left.id.cmp(&right.id));
    assertions
}

fn sorted_assertions_by_tx(assertions: impl Iterator<Item = Assertion>) -> Vec<Assertion> {
    let mut assertions = assertions.collect::<Vec<_>>();
    assertions.sort_by(|left, right| {
        left.transaction_time
            .start
            .cmp(&right.transaction_time.start)
            .then_with(|| left.id.cmp(&right.id))
    });
    assertions
}

fn path_active_during(path: &PathResult, interval: &TimeInterval<ValidTime>) -> bool {
    path.hops
        .iter()
        .all(|hop| query_result_valid_interval(hop).overlaps(interval))
}

fn query_result_valid_interval(result: &QueryResult) -> TimeInterval<ValidTime> {
    TimeInterval::new(result.valid_from, result.valid_to).expect("query result interval is valid")
}

fn path_temporal_explanation(path: &PathResult, interval: &TimeInterval<ValidTime>) -> String {
    format!(
        "path {} -> {} is active during {} because every hop overlaps the requested temporal window: {}",
        path.start,
        path.end,
        interval_name(interval),
        path.hops
            .iter()
            .map(|hop| format!("{} valid {}", hop.assertion_id, result_interval_name(hop)))
            .collect::<Vec<_>>()
            .join("; ")
    )
}

fn assertion_temporal_explanation(
    assertion: &Assertion,
    valid_at: ValidTime,
    known_at: TxTime,
) -> String {
    let valid = if assertion.valid_time.contains(valid_at) {
        format!("valid at {}", valid_at.as_i64())
    } else {
        format!("not valid at {}", valid_at.as_i64())
    };
    let known = if assertion.transaction_time.contains(known_at) {
        format!("known at tx {}", known_at.as_i64())
    } else {
        format!("not known at tx {}", known_at.as_i64())
    };
    format!(
        "{} is {}; {}; valid interval {}; transaction interval {}",
        assertion.id,
        valid,
        known,
        interval_name(&assertion.valid_time),
        tx_interval_name(&assertion.transaction_time)
    )
}

fn interval_name(interval: &TimeInterval<ValidTime>) -> String {
    format!(
        "{}..{}",
        interval.start.as_i64(),
        interval
            .end
            .map(|end| end.as_i64().to_string())
            .unwrap_or_else(|| "open".to_owned())
    )
}

fn result_interval_name(result: &QueryResult) -> String {
    format!(
        "{}..{}",
        result.valid_from.as_i64(),
        result
            .valid_to
            .map(|end| end.as_i64().to_string())
            .unwrap_or_else(|| "open".to_owned())
    )
}

fn tx_interval_name(interval: &TimeInterval<TxTime>) -> String {
    format!(
        "{}..{}",
        interval.start.as_i64(),
        interval
            .end
            .map(|end| end.as_i64().to_string())
            .unwrap_or_else(|| "open".to_owned())
    )
}
