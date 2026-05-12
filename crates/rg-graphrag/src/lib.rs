//! Temporal GraphRAG community summaries.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rg_core::{
    Assertion, AssertionId, EntityId, GraphValue, SourceId, TimeInterval, TxTime, ValidTime,
};
use rg_storage::InMemoryStorage;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CommunityId {
    pub value: String,
}

impl CommunityId {
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.value
    }
}

impl fmt::Display for CommunityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.value)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CommunitySnapshot {
    pub community_id: CommunityId,
    pub entity_ids: Vec<EntityId>,
    pub assertion_ids: Vec<AssertionId>,
    pub valid_time: SummaryValidTime,
    pub transaction_time: SummaryTxTime,
    pub source_set: SummarySourceSet,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CommunitySummary {
    pub community_id: CommunityId,
    pub snapshot: CommunitySnapshot,
    pub text: String,
    pub valid_time: SummaryValidTime,
    pub transaction_time: SummaryTxTime,
    pub source_set: SummarySourceSet,
    pub stale: bool,
    pub invalidation_reason: Option<SummaryInvalidationReason>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SummaryValidTime(TimeInterval<ValidTime>);

impl SummaryValidTime {
    pub fn new(
        start: ValidTime,
        end: Option<ValidTime>,
    ) -> Result<Self, rg_core::TimeIntervalError> {
        TimeInterval::new(start, end).map(Self)
    }

    pub fn contains(&self, instant: ValidTime) -> bool {
        self.0.contains(instant)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SummaryTxTime(TimeInterval<TxTime>);

impl SummaryTxTime {
    pub fn new(start: TxTime, end: Option<TxTime>) -> Result<Self, rg_core::TimeIntervalError> {
        TimeInterval::new(start, end).map(Self)
    }

    pub fn contains(&self, instant: TxTime) -> bool {
        self.0.contains(instant)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SummarySourceSet {
    pub source_ids: BTreeSet<SourceId>,
}

impl SummarySourceSet {
    pub fn from_sources(sources: Vec<SourceId>) -> Self {
        Self {
            source_ids: sources.into_iter().collect(),
        }
    }

    pub fn contains(&self, source_id: &SourceId) -> bool {
        self.source_ids.contains(source_id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SummaryInvalidationReason {
    AssertionAdded(AssertionId),
    AssertionRetracted(AssertionId),
    AssertionUpdated(AssertionId),
    SourceUpdated(SourceId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TemporalCommunitySummarizer {
    summaries: Vec<CommunitySummary>,
}

impl TemporalCommunitySummarizer {
    pub fn from_storage(storage: &InMemoryStorage) -> Self {
        Self {
            summaries: build_summaries(storage),
        }
    }

    pub fn summaries(&self) -> &[CommunitySummary] {
        &self.summaries
    }

    pub fn summaries_at(&self, valid_time: ValidTime, known_at: TxTime) -> Vec<&CommunitySummary> {
        self.summaries
            .iter()
            .filter(|summary| !summary.stale)
            .filter(|summary| summary.valid_time.contains(valid_time))
            .filter(|summary| summary.transaction_time.contains(known_at))
            .collect()
    }

    pub fn summary_at(
        &self,
        community_id: &CommunityId,
        valid_time: ValidTime,
        known_at: TxTime,
    ) -> Option<&CommunitySummary> {
        self.summaries_at(valid_time, known_at)
            .into_iter()
            .find(|summary| &summary.community_id == community_id)
    }

    pub fn stale_summaries(&self) -> Vec<&CommunitySummary> {
        self.summaries
            .iter()
            .filter(|summary| summary.stale)
            .collect()
    }

    pub fn invalidate_for_assertion(
        &mut self,
        assertion: &Assertion,
        reason: SummaryInvalidationReason,
    ) -> Vec<CommunityId> {
        let touched_entities = assertion_entities(assertion);
        let mut affected = Vec::new();
        for summary in &mut self.summaries {
            if summary
                .snapshot
                .entity_ids
                .iter()
                .any(|entity| touched_entities.contains(entity))
            {
                summary.stale = true;
                summary.invalidation_reason = Some(reason.clone());
                affected.push(summary.community_id.clone());
            }
        }
        affected.sort();
        affected.dedup();
        affected
    }

    pub fn recompute_affected(
        &mut self,
        storage: &InMemoryStorage,
        affected: &[CommunityId],
        recomputed_at: TxTime,
    ) -> Vec<CommunitySummary> {
        let mut recomputed = Vec::new();
        for community_id in affected {
            let Some(existing) = self
                .summaries
                .iter()
                .find(|summary| &summary.community_id == community_id)
                .cloned()
            else {
                continue;
            };
            let component = component_for_seed(storage, &existing.snapshot.entity_ids);
            if component.is_empty() {
                continue;
            }
            let mut summary = summary_for_component(storage, community_id.clone(), component);
            summary.transaction_time =
                SummaryTxTime::new(recomputed_at, None).expect("open tx interval is valid");
            summary.snapshot.transaction_time = summary.transaction_time.clone();
            summary.stale = false;
            summary.invalidation_reason = None;

            if let Some(slot) = self
                .summaries
                .iter_mut()
                .find(|summary| &summary.community_id == community_id)
            {
                *slot = summary.clone();
            }
            recomputed.push(summary);
        }
        recomputed
    }
}

fn build_summaries(storage: &InMemoryStorage) -> Vec<CommunitySummary> {
    connected_components(storage)
        .into_iter()
        .map(|component| {
            let community_id = CommunityId::new(format!(
                "community-{}",
                component
                    .first()
                    .map(|entity| slugify(entity.as_str()))
                    .unwrap_or_else(|| "empty".to_owned())
            ));
            summary_for_component(storage, community_id, component)
        })
        .collect()
}

fn connected_components(storage: &InMemoryStorage) -> Vec<Vec<EntityId>> {
    let adjacency = adjacency(storage);
    let mut visited = BTreeSet::new();
    let mut components = Vec::new();
    for entity in adjacency.keys() {
        if visited.contains(entity) {
            continue;
        }
        let mut stack = vec![entity.clone()];
        let mut component = BTreeSet::new();
        while let Some(current) = stack.pop() {
            if !visited.insert(current.clone()) {
                continue;
            }
            component.insert(current.clone());
            if let Some(neighbors) = adjacency.get(&current) {
                for neighbor in neighbors {
                    if !visited.contains(neighbor) {
                        stack.push(neighbor.clone());
                    }
                }
            }
        }
        components.push(component.into_iter().collect::<Vec<_>>());
    }
    components.sort_by(|left, right| left.first().cmp(&right.first()));
    components
}

fn component_for_seed(storage: &InMemoryStorage, seed_entities: &[EntityId]) -> Vec<EntityId> {
    let adjacency = adjacency(storage);
    let mut visited = BTreeSet::new();
    let mut stack = seed_entities.to_vec();
    while let Some(current) = stack.pop() {
        if !visited.insert(current.clone()) {
            continue;
        }
        if let Some(neighbors) = adjacency.get(&current) {
            for neighbor in neighbors {
                if !visited.contains(neighbor) {
                    stack.push(neighbor.clone());
                }
            }
        }
    }
    visited.into_iter().collect()
}

fn adjacency(storage: &InMemoryStorage) -> BTreeMap<EntityId, BTreeSet<EntityId>> {
    let mut adjacency = BTreeMap::<EntityId, BTreeSet<EntityId>>::new();
    for assertion in storage.graph_state().assertions.values() {
        let GraphValue::Entity(object) = &assertion.object else {
            continue;
        };
        adjacency
            .entry(assertion.subject.clone())
            .or_default()
            .insert(object.clone());
        adjacency
            .entry(object.clone())
            .or_default()
            .insert(assertion.subject.clone());
    }
    adjacency
}

fn summary_for_component(
    storage: &InMemoryStorage,
    community_id: CommunityId,
    entity_ids: Vec<EntityId>,
) -> CommunitySummary {
    let entity_set = entity_ids.iter().cloned().collect::<BTreeSet<_>>();
    let assertions = storage
        .graph_state()
        .assertions
        .values()
        .filter(|assertion| {
            entity_set.contains(&assertion.subject)
                || matches!(&assertion.object, GraphValue::Entity(object) if entity_set.contains(object))
        })
        .collect::<Vec<_>>();
    let assertion_ids = assertions
        .iter()
        .map(|assertion| assertion.id.clone())
        .collect::<Vec<_>>();
    let source_set = SummarySourceSet {
        source_ids: assertions
            .iter()
            .flat_map(|assertion| assertion.source_ids.iter().cloned())
            .collect(),
    };
    let valid_time = summary_valid_time(&assertions);
    let transaction_time = summary_tx_time(&assertions);
    let snapshot = CommunitySnapshot {
        community_id: community_id.clone(),
        entity_ids: entity_ids.clone(),
        assertion_ids,
        valid_time: valid_time.clone(),
        transaction_time: transaction_time.clone(),
        source_set: source_set.clone(),
    };
    CommunitySummary {
        community_id,
        snapshot,
        text: render_summary(storage, &entity_ids, &assertions),
        valid_time,
        transaction_time,
        source_set,
        stale: false,
        invalidation_reason: None,
    }
}

fn summary_valid_time(assertions: &[&Assertion]) -> SummaryValidTime {
    let start = assertions
        .iter()
        .map(|assertion| assertion.valid_time.start)
        .max()
        .unwrap_or_else(|| ValidTime::new(0));
    let end = assertions
        .iter()
        .filter_map(|assertion| assertion.valid_time.end)
        .min();
    SummaryValidTime::new(start, end).expect("summary valid interval is derived from assertions")
}

fn summary_tx_time(assertions: &[&Assertion]) -> SummaryTxTime {
    let start = assertions
        .iter()
        .map(|assertion| assertion.transaction_time.start)
        .max()
        .unwrap_or_else(|| TxTime::new(0));
    let end = assertions
        .iter()
        .filter_map(|assertion| assertion.transaction_time.end)
        .min();
    SummaryTxTime::new(start, end).expect("summary tx interval is derived from assertions")
}

fn render_summary(
    storage: &InMemoryStorage,
    entity_ids: &[EntityId],
    assertions: &[&Assertion],
) -> String {
    let entities = entity_ids
        .iter()
        .map(|entity_id| entity_name(storage, entity_id))
        .collect::<Vec<_>>()
        .join(", ");
    let relationships = assertions
        .iter()
        .map(|assertion| {
            format!(
                "{} {} {}",
                entity_name(storage, &assertion.subject),
                assertion.predicate.as_str(),
                graph_value_name(storage, &assertion.object)
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!("Community contains {entities}. Relationships: {relationships}.")
}

fn entity_name(storage: &InMemoryStorage, entity_id: &EntityId) -> String {
    storage
        .entity(entity_id)
        .and_then(|entity| entity.canonical_name.clone())
        .unwrap_or_else(|| entity_id.as_str().to_owned())
}

fn graph_value_name(storage: &InMemoryStorage, value: &GraphValue) -> String {
    match value {
        GraphValue::Entity(entity_id) => entity_name(storage, entity_id),
        GraphValue::Text(value) => value.clone(),
        GraphValue::Integer(value) => value.to_string(),
        GraphValue::Decimal(value) => value.to_string(),
        GraphValue::Boolean(value) => value.to_string(),
        GraphValue::Time(value) => value.as_i64().to_string(),
        GraphValue::Null => "null".to_owned(),
    }
}

fn assertion_entities(assertion: &Assertion) -> BTreeSet<EntityId> {
    let mut entities = BTreeSet::from([assertion.subject.clone()]);
    if let GraphValue::Entity(object) = &assertion.object {
        entities.insert(object.clone());
    }
    entities
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut previous_separator = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            previous_separator = false;
        } else if !previous_separator {
            slug.push('-');
            previous_separator = true;
        }
    }
    let slug = slug.trim_matches('-').to_owned();
    if slug.is_empty() {
        "unknown".to_owned()
    } else {
        slug
    }
}
