//! Event-sourced write path for Reality Graph.

use std::collections::BTreeMap;

pub use rg_core::{
    AgentId, AgentMemory, Assertion, AssertionId, AssertionStatus, CausalLink, CausalLinkId,
    Confidence, ContentHash, ContextScope, Entity, EntityId, EntityType, EventId, GraphOntology,
    GraphValue, MemoryId, MemoryStatus, MemoryType, OntologyValidationError, PredicateId,
    PropertyKey, PropertyMap, Source, SourceId, SourceType, TimeInterval, TxTime, ValidTime,
};

#[derive(Clone, Debug, PartialEq)]
pub enum GraphCommand {
    CreateEntity(CreateEntity),
    AddAssertion(AddAssertion),
    RetractAssertion(RetractAssertion),
    AddSource(AddSource),
    LinkEvidence(LinkEvidence),
    MergeEntities(MergeEntities),
    UpdateConfidence(UpdateConfidence),
    AddCausalLink(AddCausalLink),
    RecordAgentMemory(RecordAgentMemory),
}

#[derive(Clone, Debug, PartialEq)]
pub struct CreateEntity {
    pub id: EntityId,
    pub entity_type: EntityType,
    pub canonical_name: Option<String>,
    pub properties: PropertyMap,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AddAssertion {
    pub id: AssertionId,
    pub subject: EntityId,
    pub predicate: PredicateId,
    pub object: GraphValue,
    pub valid_time: TimeInterval<ValidTime>,
    pub confidence: Confidence,
    pub source_ids: Vec<SourceId>,
    pub context: ContextScope,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetractAssertion {
    pub id: AssertionId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AddSource {
    pub id: SourceId,
    pub source_type: SourceType,
    pub uri: Option<String>,
    pub content_hash: ContentHash,
    pub trust_score: Option<f32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinkEvidence {
    pub assertion_id: AssertionId,
    pub source_id: SourceId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeEntities {
    pub from: EntityId,
    pub into: EntityId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UpdateConfidence {
    pub assertion_id: AssertionId,
    pub confidence: Confidence,
    pub source_ids: Vec<SourceId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AddCausalLink {
    pub id: CausalLinkId,
    pub cause_event: EventId,
    pub effect_event: EventId,
    pub confidence: Confidence,
    pub mechanism: Option<String>,
    pub source_ids: Vec<SourceId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecordAgentMemory {
    pub memory: AgentMemory,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GraphEvent {
    EntityCreated(EntityCreated),
    AssertionAdded(AssertionAdded),
    AssertionRetracted(AssertionRetracted),
    SourceAdded(SourceAdded),
    EvidenceLinked(EvidenceLinked),
    EntityMerged(EntityMerged),
    ConfidenceUpdated(ConfidenceUpdated),
    CausalLinkAdded(CausalLinkAdded),
    AgentMemoryRecorded(AgentMemoryRecorded),
}

impl GraphEvent {
    pub fn event_id(&self) -> &EventId {
        match self {
            Self::EntityCreated(event) => &event.event_id,
            Self::AssertionAdded(event) => &event.event_id,
            Self::AssertionRetracted(event) => &event.event_id,
            Self::SourceAdded(event) => &event.event_id,
            Self::EvidenceLinked(event) => &event.event_id,
            Self::EntityMerged(event) => &event.event_id,
            Self::ConfidenceUpdated(event) => &event.event_id,
            Self::CausalLinkAdded(event) => &event.event_id,
            Self::AgentMemoryRecorded(event) => &event.event_id,
        }
    }

    pub fn transaction_time(&self) -> TxTime {
        match self {
            Self::EntityCreated(event) => event.transaction_time,
            Self::AssertionAdded(event) => event.transaction_time,
            Self::AssertionRetracted(event) => event.transaction_time,
            Self::SourceAdded(event) => event.transaction_time,
            Self::EvidenceLinked(event) => event.transaction_time,
            Self::EntityMerged(event) => event.transaction_time,
            Self::ConfidenceUpdated(event) => event.transaction_time,
            Self::CausalLinkAdded(event) => event.transaction_time,
            Self::AgentMemoryRecorded(event) => event.transaction_time,
        }
    }

    fn kind_slug(&self) -> &'static str {
        match self {
            Self::EntityCreated(_) => "entity-created",
            Self::AssertionAdded(_) => "assertion-added",
            Self::AssertionRetracted(_) => "assertion-retracted",
            Self::SourceAdded(_) => "source-added",
            Self::EvidenceLinked(_) => "evidence-linked",
            Self::EntityMerged(_) => "entity-merged",
            Self::ConfidenceUpdated(_) => "confidence-updated",
            Self::CausalLinkAdded(_) => "causal-link-added",
            Self::AgentMemoryRecorded(_) => "agent-memory-recorded",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EntityCreated {
    pub event_id: EventId,
    pub transaction_time: TxTime,
    pub entity: Entity,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AssertionAdded {
    pub event_id: EventId,
    pub transaction_time: TxTime,
    pub assertion: Assertion,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AssertionRetracted {
    pub event_id: EventId,
    pub transaction_time: TxTime,
    pub assertion_id: AssertionId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceAdded {
    pub event_id: EventId,
    pub transaction_time: TxTime,
    pub source: Source,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EvidenceLinked {
    pub event_id: EventId,
    pub transaction_time: TxTime,
    pub assertion_id: AssertionId,
    pub source_id: SourceId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EntityMerged {
    pub event_id: EventId,
    pub transaction_time: TxTime,
    pub from: EntityId,
    pub into: EntityId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConfidenceUpdated {
    pub event_id: EventId,
    pub transaction_time: TxTime,
    pub assertion_id: AssertionId,
    pub confidence: Confidence,
    pub source_ids: Vec<SourceId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CausalLinkAdded {
    pub event_id: EventId,
    pub transaction_time: TxTime,
    pub causal_link: CausalLink,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentMemoryRecorded {
    pub event_id: EventId,
    pub transaction_time: TxTime,
    pub memory: AgentMemory,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GraphState {
    pub entities: BTreeMap<EntityId, Entity>,
    pub assertions: BTreeMap<AssertionId, Assertion>,
    pub sources: BTreeMap<SourceId, Source>,
    pub evidence_links: BTreeMap<AssertionId, Vec<SourceId>>,
    pub merged_entities: BTreeMap<EntityId, EntityId>,
    pub causal_links: BTreeMap<CausalLinkId, CausalLink>,
    pub agent_memories: BTreeMap<MemoryId, AgentMemory>,
    outgoing: BTreeMap<EntityId, Vec<AssertionId>>,
}

impl GraphState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn replay(events: &[GraphEvent]) -> Result<Self, GraphReplayError> {
        let mut state = Self::new();
        for event in events {
            state.apply_event(event)?;
        }
        Ok(state)
    }

    pub fn outgoing_assertions(&self, entity: &EntityId) -> Vec<&Assertion> {
        self.outgoing
            .get(entity)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| self.assertions.get(id))
            .collect()
    }

    fn apply_event(&mut self, event: &GraphEvent) -> Result<(), GraphReplayError> {
        match event {
            GraphEvent::EntityCreated(event) => {
                self.entities
                    .insert(event.entity.id.clone(), event.entity.clone());
            }
            GraphEvent::AssertionAdded(event) => {
                if !self.entities.contains_key(&event.assertion.subject) {
                    return Err(GraphReplayError::UnknownEntity(
                        event.assertion.subject.clone(),
                    ));
                }
                if let GraphValue::Entity(entity_id) = &event.assertion.object {
                    if !self.entities.contains_key(entity_id) {
                        return Err(GraphReplayError::UnknownEntity(entity_id.clone()));
                    }
                }
                for source_id in &event.assertion.source_ids {
                    if !self.sources.contains_key(source_id) {
                        return Err(GraphReplayError::UnknownSource(source_id.clone()));
                    }
                }

                self.outgoing
                    .entry(event.assertion.subject.clone())
                    .or_default()
                    .push(event.assertion.id.clone());
                if let Some(ids) = self.outgoing.get_mut(&event.assertion.subject) {
                    ids.sort();
                    ids.dedup();
                }
                self.assertions
                    .insert(event.assertion.id.clone(), event.assertion.clone());
            }
            GraphEvent::AssertionRetracted(event) => {
                let assertion = self
                    .assertions
                    .get_mut(&event.assertion_id)
                    .ok_or_else(|| {
                        GraphReplayError::UnknownAssertion(event.assertion_id.clone())
                    })?;
                assertion.status = AssertionStatus::Retracted;
                assertion.transaction_time.end = Some(event.transaction_time);
            }
            GraphEvent::SourceAdded(event) => {
                self.sources
                    .insert(event.source.id.clone(), event.source.clone());
            }
            GraphEvent::EvidenceLinked(event) => {
                if !self.assertions.contains_key(&event.assertion_id) {
                    return Err(GraphReplayError::UnknownAssertion(
                        event.assertion_id.clone(),
                    ));
                }
                if !self.sources.contains_key(&event.source_id) {
                    return Err(GraphReplayError::UnknownSource(event.source_id.clone()));
                }
                let links = self
                    .evidence_links
                    .entry(event.assertion_id.clone())
                    .or_default();
                links.push(event.source_id.clone());
                links.sort();
                links.dedup();
            }
            GraphEvent::EntityMerged(event) => {
                if !self.entities.contains_key(&event.from) {
                    return Err(GraphReplayError::UnknownEntity(event.from.clone()));
                }
                if !self.entities.contains_key(&event.into) {
                    return Err(GraphReplayError::UnknownEntity(event.into.clone()));
                }
                self.merged_entities
                    .insert(event.from.clone(), event.into.clone());
            }
            GraphEvent::ConfidenceUpdated(event) => {
                for source_id in &event.source_ids {
                    if !self.sources.contains_key(source_id) {
                        return Err(GraphReplayError::UnknownSource(source_id.clone()));
                    }
                }
                let assertion = self
                    .assertions
                    .get_mut(&event.assertion_id)
                    .ok_or_else(|| {
                        GraphReplayError::UnknownAssertion(event.assertion_id.clone())
                    })?;
                assertion.confidence = event.confidence;
                for source_id in &event.source_ids {
                    if !assertion.source_ids.contains(source_id) {
                        assertion.source_ids.push(source_id.clone());
                    }
                }
                assertion.source_ids.sort();
                assertion.source_ids.dedup();
            }
            GraphEvent::CausalLinkAdded(event) => {
                for source_id in &event.causal_link.source_ids {
                    if !self.sources.contains_key(source_id) {
                        return Err(GraphReplayError::UnknownSource(source_id.clone()));
                    }
                }
                self.causal_links
                    .insert(event.causal_link.id.clone(), event.causal_link.clone());
            }
            GraphEvent::AgentMemoryRecorded(event) => {
                for source_id in &event.memory.source_ids {
                    if !self.sources.contains_key(source_id) {
                        return Err(GraphReplayError::UnknownSource(source_id.clone()));
                    }
                }
                for entity_id in &event.memory.related_entities {
                    if !self.entities.contains_key(entity_id) {
                        return Err(GraphReplayError::UnknownEntity(entity_id.clone()));
                    }
                }
                for memory_id in &event.memory.supersedes {
                    let memory = self
                        .agent_memories
                        .get_mut(memory_id)
                        .ok_or_else(|| GraphReplayError::UnknownMemory(memory_id.clone()))?;
                    memory.status = MemoryStatus::Superseded;
                }
                self.agent_memories
                    .insert(event.memory.id.clone(), event.memory.clone());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphReplayError {
    UnknownEntity(EntityId),
    UnknownAssertion(AssertionId),
    UnknownSource(SourceId),
    UnknownMemory(MemoryId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct EventLog {
    events: Vec<GraphEvent>,
    state: GraphState,
    last_tx: TxTime,
    next_sequence: u64,
    ontology: Option<GraphOntology>,
}

impl EventLog {
    pub fn new(start_tx: TxTime) -> Self {
        Self {
            events: Vec::new(),
            state: GraphState::new(),
            last_tx: start_tx,
            next_sequence: 1,
            ontology: None,
        }
    }

    pub fn with_ontology(start_tx: TxTime, ontology: GraphOntology) -> Self {
        Self {
            ontology: Some(ontology),
            ..Self::new(start_tx)
        }
    }

    pub fn execute(&mut self, command: GraphCommand) -> Result<GraphEvent, GraphCommandError> {
        self.validate(&command)?;
        let sequence = self.next_sequence;
        let transaction_time = TxTime::new(self.last_tx.as_i64() + 1);
        let mut event = self.event_from_command(command, sequence, transaction_time);
        let event_id = deterministic_event_id(sequence, event.kind_slug());
        set_event_id(&mut event, event_id);

        self.events.push(event.clone());
        self.state
            .apply_event(&event)
            .map_err(GraphCommandError::Replay)?;
        self.last_tx = transaction_time;
        self.next_sequence += 1;

        Ok(event)
    }

    pub fn events(&self) -> &[GraphEvent] {
        &self.events
    }

    pub fn state(&self) -> &GraphState {
        &self.state
    }

    pub fn rebuild_indexes(&mut self) -> Result<(), GraphReplayError> {
        self.state = GraphState::replay(&self.events)?;
        Ok(())
    }

    #[cfg(test)]
    pub fn state_mut_for_tests(&mut self) -> &mut GraphState {
        &mut self.state
    }

    fn validate(&self, command: &GraphCommand) -> Result<(), GraphCommandError> {
        match command {
            GraphCommand::CreateEntity(command) => {
                if self.state.entities.contains_key(&command.id) {
                    return Err(GraphCommandError::DuplicateEntity(command.id.clone()));
                }
                if let Some(ontology) = &self.ontology {
                    let entity = Entity {
                        id: command.id.clone(),
                        entity_type: command.entity_type.clone(),
                        canonical_name: command.canonical_name.clone(),
                        properties: command.properties.clone(),
                        created_tx: self.last_tx,
                    };
                    ontology
                        .validate_entity(&entity)
                        .map_err(GraphCommandError::Ontology)?;
                }
            }
            GraphCommand::AddAssertion(command) => {
                validate_assertion_refs(&self.state, command)?;
                if self.state.assertions.contains_key(&command.id) {
                    return Err(GraphCommandError::DuplicateAssertion(command.id.clone()));
                }
                if command.source_ids.is_empty() {
                    return Err(GraphCommandError::EmptySourceList);
                }
                if let Some(ontology) = &self.ontology {
                    let assertion = assertion_from_command(command, self.last_tx);
                    let mut assertions = self.state.assertions.values().collect::<Vec<_>>();
                    assertions.push(&assertion);
                    ontology
                        .validate_assertions(assertions, &self.state.entities)
                        .map_err(GraphCommandError::Ontology)?;
                }
            }
            GraphCommand::RetractAssertion(command) => {
                if !self.state.assertions.contains_key(&command.id) {
                    return Err(GraphCommandError::UnknownAssertion(command.id.clone()));
                }
            }
            GraphCommand::AddSource(command) => {
                if self.state.sources.contains_key(&command.id) {
                    return Err(GraphCommandError::DuplicateSource(command.id.clone()));
                }
                if !command
                    .trust_score
                    .map(|score| (0.0..=1.0).contains(&score))
                    .unwrap_or(true)
                {
                    return Err(GraphCommandError::InvalidTrustScore);
                }
            }
            GraphCommand::LinkEvidence(command) => {
                if !self.state.assertions.contains_key(&command.assertion_id) {
                    return Err(GraphCommandError::UnknownAssertion(
                        command.assertion_id.clone(),
                    ));
                }
                if !self.state.sources.contains_key(&command.source_id) {
                    return Err(GraphCommandError::UnknownSource(command.source_id.clone()));
                }
            }
            GraphCommand::MergeEntities(command) => {
                if command.from == command.into {
                    return Err(GraphCommandError::InvalidEntityMerge);
                }
                if !self.state.entities.contains_key(&command.from) {
                    return Err(GraphCommandError::UnknownEntity(command.from.clone()));
                }
                if !self.state.entities.contains_key(&command.into) {
                    return Err(GraphCommandError::UnknownEntity(command.into.clone()));
                }
            }
            GraphCommand::UpdateConfidence(command) => {
                if !self.state.assertions.contains_key(&command.assertion_id) {
                    return Err(GraphCommandError::UnknownAssertion(
                        command.assertion_id.clone(),
                    ));
                }
                validate_sources(&self.state, &command.source_ids)?;
            }
            GraphCommand::AddCausalLink(command) => {
                if self.state.causal_links.contains_key(&command.id) {
                    return Err(GraphCommandError::DuplicateCausalLink(command.id.clone()));
                }
                validate_sources(&self.state, &command.source_ids)?;
            }
            GraphCommand::RecordAgentMemory(command) => {
                if self.state.agent_memories.contains_key(&command.memory.id) {
                    return Err(GraphCommandError::DuplicateMemory(
                        command.memory.id.clone(),
                    ));
                }
                if command.memory.source_ids.is_empty() {
                    return Err(GraphCommandError::EmptySourceList);
                }
                validate_sources(&self.state, &command.memory.source_ids)?;
                for entity_id in &command.memory.related_entities {
                    if !self.state.entities.contains_key(entity_id) {
                        return Err(GraphCommandError::UnknownEntity(entity_id.clone()));
                    }
                }
                for memory_id in &command.memory.supersedes {
                    if !self.state.agent_memories.contains_key(memory_id) {
                        return Err(GraphCommandError::UnknownMemory(memory_id.clone()));
                    }
                }
            }
        }
        Ok(())
    }

    fn event_from_command(
        &self,
        command: GraphCommand,
        _sequence: u64,
        transaction_time: TxTime,
    ) -> GraphEvent {
        match command {
            GraphCommand::CreateEntity(command) => GraphEvent::EntityCreated(EntityCreated {
                event_id: EventId::new(""),
                transaction_time,
                entity: Entity {
                    id: command.id,
                    entity_type: command.entity_type,
                    canonical_name: command.canonical_name,
                    properties: command.properties,
                    created_tx: transaction_time,
                },
            }),
            GraphCommand::AddAssertion(command) => GraphEvent::AssertionAdded(AssertionAdded {
                event_id: EventId::new(""),
                transaction_time,
                assertion: Assertion {
                    id: command.id,
                    subject: command.subject,
                    predicate: command.predicate,
                    object: command.object,
                    valid_time: command.valid_time,
                    transaction_time: TimeInterval::new(transaction_time, None)
                        .expect("open transaction interval is valid"),
                    confidence: command.confidence,
                    source_ids: command.source_ids,
                    context: command.context,
                    status: AssertionStatus::Active,
                },
            }),
            GraphCommand::RetractAssertion(command) => {
                GraphEvent::AssertionRetracted(AssertionRetracted {
                    event_id: EventId::new(""),
                    transaction_time,
                    assertion_id: command.id,
                })
            }
            GraphCommand::AddSource(command) => GraphEvent::SourceAdded(SourceAdded {
                event_id: EventId::new(""),
                transaction_time,
                source: Source {
                    id: command.id,
                    source_type: command.source_type,
                    uri: command.uri,
                    content_hash: command.content_hash,
                    observed_at: transaction_time,
                    trust_score: command.trust_score,
                },
            }),
            GraphCommand::LinkEvidence(command) => GraphEvent::EvidenceLinked(EvidenceLinked {
                event_id: EventId::new(""),
                transaction_time,
                assertion_id: command.assertion_id,
                source_id: command.source_id,
            }),
            GraphCommand::MergeEntities(command) => GraphEvent::EntityMerged(EntityMerged {
                event_id: EventId::new(""),
                transaction_time,
                from: command.from,
                into: command.into,
            }),
            GraphCommand::UpdateConfidence(command) => {
                GraphEvent::ConfidenceUpdated(ConfidenceUpdated {
                    event_id: EventId::new(""),
                    transaction_time,
                    assertion_id: command.assertion_id,
                    confidence: command.confidence,
                    source_ids: command.source_ids,
                })
            }
            GraphCommand::AddCausalLink(command) => GraphEvent::CausalLinkAdded(CausalLinkAdded {
                event_id: EventId::new(""),
                transaction_time,
                causal_link: CausalLink {
                    id: command.id,
                    cause_event: command.cause_event,
                    effect_event: command.effect_event,
                    confidence: command.confidence,
                    mechanism: command.mechanism,
                    source_ids: command.source_ids,
                },
            }),
            GraphCommand::RecordAgentMemory(command) => {
                GraphEvent::AgentMemoryRecorded(AgentMemoryRecorded {
                    event_id: EventId::new(""),
                    transaction_time,
                    memory: command.memory,
                })
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphCommandError {
    DuplicateEntity(EntityId),
    DuplicateAssertion(AssertionId),
    DuplicateSource(SourceId),
    DuplicateCausalLink(CausalLinkId),
    DuplicateMemory(MemoryId),
    UnknownEntity(EntityId),
    UnknownAssertion(AssertionId),
    UnknownSource(SourceId),
    UnknownMemory(MemoryId),
    EmptySourceList,
    InvalidEntityMerge,
    InvalidTrustScore,
    Ontology(OntologyValidationError),
    Replay(GraphReplayError),
}

fn assertion_from_command(command: &AddAssertion, transaction_time: TxTime) -> Assertion {
    Assertion {
        id: command.id.clone(),
        subject: command.subject.clone(),
        predicate: command.predicate.clone(),
        object: command.object.clone(),
        valid_time: command.valid_time.clone(),
        transaction_time: TimeInterval::new(transaction_time, None)
            .expect("open transaction interval is valid"),
        confidence: command.confidence,
        source_ids: command.source_ids.clone(),
        context: command.context.clone(),
        status: AssertionStatus::Active,
    }
}

fn validate_assertion_refs(
    state: &GraphState,
    command: &AddAssertion,
) -> Result<(), GraphCommandError> {
    if !state.entities.contains_key(&command.subject) {
        return Err(GraphCommandError::UnknownEntity(command.subject.clone()));
    }
    if let GraphValue::Entity(entity_id) = &command.object {
        if !state.entities.contains_key(entity_id) {
            return Err(GraphCommandError::UnknownEntity(entity_id.clone()));
        }
    }
    validate_sources(state, &command.source_ids)
}

fn validate_sources(state: &GraphState, source_ids: &[SourceId]) -> Result<(), GraphCommandError> {
    for source_id in source_ids {
        if !state.sources.contains_key(source_id) {
            return Err(GraphCommandError::UnknownSource(source_id.clone()));
        }
    }
    Ok(())
}

fn deterministic_event_id(sequence: u64, kind_slug: &str) -> EventId {
    EventId::new(format!("evt-{sequence:018}-{kind_slug}"))
}

fn set_event_id(event: &mut GraphEvent, event_id: EventId) {
    match event {
        GraphEvent::EntityCreated(event) => event.event_id = event_id,
        GraphEvent::AssertionAdded(event) => event.event_id = event_id,
        GraphEvent::AssertionRetracted(event) => event.event_id = event_id,
        GraphEvent::SourceAdded(event) => event.event_id = event_id,
        GraphEvent::EvidenceLinked(event) => event.event_id = event_id,
        GraphEvent::EntityMerged(event) => event.event_id = event_id,
        GraphEvent::ConfidenceUpdated(event) => event.event_id = event_id,
        GraphEvent::CausalLinkAdded(event) => event.event_id = event_id,
        GraphEvent::AgentMemoryRecorded(event) => event.event_id = event_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_command() -> GraphCommand {
        GraphCommand::AddSource(AddSource {
            id: SourceId::new("source-1"),
            source_type: SourceType::Document,
            uri: Some("file://source.md".to_owned()),
            content_hash: ContentHash::new("sha256:source"),
            trust_score: Some(0.8),
        })
    }

    fn entity_command(id: &str) -> GraphCommand {
        GraphCommand::CreateEntity(CreateEntity {
            id: EntityId::new(id),
            entity_type: EntityType::Person,
            canonical_name: Some(id.to_owned()),
            properties: PropertyMap::default(),
        })
    }

    fn assertion_command() -> GraphCommand {
        GraphCommand::AddAssertion(AddAssertion {
            id: AssertionId::new("assertion-1"),
            subject: EntityId::new("person-a"),
            predicate: PredicateId::new("works_at"),
            object: GraphValue::Entity(EntityId::new("company-b")),
            valid_time: TimeInterval::new(ValidTime::new(10), Some(ValidTime::new(20)))
                .expect("valid interval"),
            confidence: Confidence::new(0.9).expect("valid confidence"),
            source_ids: vec![SourceId::new("source-1")],
            context: ContextScope::Global,
        })
    }

    fn company_entity_command(id: &str) -> GraphCommand {
        GraphCommand::CreateEntity(CreateEntity {
            id: EntityId::new(id),
            entity_type: EntityType::Organization,
            canonical_name: Some(id.to_owned()),
            properties: PropertyMap(
                [
                    (
                        PropertyKey::new("name"),
                        GraphValue::Text(format!("Company {id}")),
                    ),
                    (
                        PropertyKey::new("jurisdiction"),
                        GraphValue::Text("US-DE".to_owned()),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
        })
    }

    fn person_entity_command(id: &str) -> GraphCommand {
        GraphCommand::CreateEntity(CreateEntity {
            id: EntityId::new(id),
            entity_type: EntityType::Person,
            canonical_name: Some(id.to_owned()),
            properties: PropertyMap(
                [
                    (PropertyKey::new("name"), GraphValue::Text(id.to_owned())),
                    (
                        PropertyKey::new("birth_date"),
                        GraphValue::Time(ValidTime::new(10)),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
        })
    }

    fn ontology() -> rg_core::GraphOntology {
        rg_core::GraphOntology::from_yaml_str(include_str!(
            "../../../schemas/ontology/reality-graph.yaml"
        ))
        .expect("ontology yaml parses")
    }

    #[test]
    fn invalid_commands_do_not_append_events() {
        let mut log = EventLog::new(TxTime::new(1));
        let invalid = GraphCommand::AddAssertion(AddAssertion {
            id: AssertionId::new("assertion-1"),
            subject: EntityId::new("missing-person"),
            predicate: PredicateId::new("works_at"),
            object: GraphValue::Entity(EntityId::new("missing-company")),
            valid_time: TimeInterval::new(ValidTime::new(10), None).expect("valid interval"),
            confidence: Confidence::new(0.9).expect("valid confidence"),
            source_ids: vec![SourceId::new("missing-source")],
            context: ContextScope::Global,
        });

        let error = log.execute(invalid).expect_err("validation should fail");

        assert_eq!(
            error,
            GraphCommandError::UnknownEntity(EntityId::new("missing-person"))
        );
        assert!(log.events().is_empty());
    }

    #[test]
    fn ontology_backed_event_log_rejects_assertions_with_invalid_entity_types() {
        let mut log = EventLog::with_ontology(TxTime::new(100), ontology());
        log.execute(source_command()).expect("source added");
        log.execute(person_entity_command("person-a"))
            .expect("person added");
        log.execute(company_entity_command("company-a"))
            .expect("company added");

        let error = log
            .execute(GraphCommand::AddAssertion(AddAssertion {
                id: AssertionId::new("invalid-ceo"),
                subject: EntityId::new("company-a"),
                predicate: PredicateId::new("CEO_OF"),
                object: GraphValue::Entity(EntityId::new("company-a")),
                valid_time: TimeInterval::new(ValidTime::new(10), Some(ValidTime::new(20)))
                    .expect("valid interval"),
                confidence: Confidence::new(0.9).expect("valid confidence"),
                source_ids: vec![SourceId::new("source-1")],
                context: ContextScope::Global,
            }))
            .expect_err("invalid subject type rejected");

        assert_eq!(
            error,
            GraphCommandError::Ontology(rg_core::OntologyValidationError::SubjectTypeMismatch {
                predicate: PredicateId::new("CEO_OF"),
                subject: EntityId::new("company-a"),
                expected: rg_core::OntologyTypeRef::Named("Person".to_owned()),
                actual: EntityType::Organization,
            })
        );
        assert_eq!(log.events().len(), 3);
    }

    #[test]
    fn ontology_backed_event_log_rejects_predicate_constraint_violations() {
        let mut log = EventLog::with_ontology(TxTime::new(100), ontology());
        log.execute(source_command()).expect("source added");
        log.execute(person_entity_command("person-a"))
            .expect("person added");
        log.execute(company_entity_command("company-a"))
            .expect("company a added");
        log.execute(company_entity_command("company-b"))
            .expect("company b added");
        log.execute(GraphCommand::AddAssertion(AddAssertion {
            id: AssertionId::new("ceo-a"),
            subject: EntityId::new("person-a"),
            predicate: PredicateId::new("CEO_OF"),
            object: GraphValue::Entity(EntityId::new("company-a")),
            valid_time: TimeInterval::new(ValidTime::new(10), Some(ValidTime::new(20)))
                .expect("valid interval"),
            confidence: Confidence::new(0.9).expect("valid confidence"),
            source_ids: vec![SourceId::new("source-1")],
            context: ContextScope::Global,
        }))
        .expect("first CEO assertion accepted");

        let error = log
            .execute(GraphCommand::AddAssertion(AddAssertion {
                id: AssertionId::new("ceo-b"),
                subject: EntityId::new("person-a"),
                predicate: PredicateId::new("CEO_OF"),
                object: GraphValue::Entity(EntityId::new("company-b")),
                valid_time: TimeInterval::new(ValidTime::new(15), Some(ValidTime::new(25)))
                    .expect("valid interval"),
                confidence: Confidence::new(0.9).expect("valid confidence"),
                source_ids: vec![SourceId::new("source-1")],
                context: ContextScope::Global,
            }))
            .expect_err("overlapping CEO assertions rejected");

        assert_eq!(
            error,
            GraphCommandError::Ontology(
                rg_core::OntologyValidationError::MaxActiveObjectsPerSubject {
                    predicate: PredicateId::new("CEO_OF"),
                    subject: EntityId::new("person-a"),
                    max: 1,
                    assertion_ids: vec![AssertionId::new("ceo-a"), AssertionId::new("ceo-b")],
                }
            )
        );
        assert_eq!(log.events().len(), 5);
    }

    fn memory(
        id: &str,
        memory_type: rg_core::MemoryType,
        content: &str,
        supersedes: Vec<rg_core::MemoryId>,
    ) -> rg_core::AgentMemory {
        rg_core::AgentMemory {
            id: rg_core::MemoryId::new(id),
            agent_id: rg_core::AgentId::new("agent-1"),
            memory_type,
            content: content.to_owned(),
            valid_time: TimeInterval::new(ValidTime::new(10), None).expect("valid interval"),
            confidence: Confidence::new(0.8).expect("valid confidence"),
            source_ids: vec![SourceId::new("source-1")],
            related_entities: vec![EntityId::new("person-a")],
            supersedes,
            status: rg_core::MemoryStatus::Active,
        }
    }

    #[test]
    fn records_agent_memory_as_event_and_materialized_state() {
        let mut log = EventLog::new(TxTime::new(0));
        log.execute(source_command()).expect("source added");
        log.execute(entity_command("person-a"))
            .expect("related entity added");

        let event = log
            .execute(GraphCommand::RecordAgentMemory(RecordAgentMemory {
                memory: memory(
                    "memory-observation",
                    rg_core::MemoryType::Observation,
                    "Person A prefers concise answers.",
                    Vec::new(),
                ),
            }))
            .expect("memory recorded");

        assert_eq!(
            event.event_id().as_str(),
            "evt-000000000000000003-agent-memory-recorded"
        );
        assert_eq!(
            log.state()
                .agent_memories
                .get(&rg_core::MemoryId::new("memory-observation"))
                .expect("memory materialized")
                .memory_type,
            rg_core::MemoryType::Observation
        );
    }

    #[test]
    fn correction_memory_supersedes_prior_memory_without_deleting_it() {
        let mut log = EventLog::new(TxTime::new(0));
        log.execute(source_command()).expect("source added");
        log.execute(entity_command("person-a"))
            .expect("related entity added");
        log.execute(GraphCommand::RecordAgentMemory(RecordAgentMemory {
            memory: memory(
                "memory-old",
                rg_core::MemoryType::Observation,
                "Person A works at Company B.",
                Vec::new(),
            ),
        }))
        .expect("old memory recorded");

        log.execute(GraphCommand::RecordAgentMemory(RecordAgentMemory {
            memory: memory(
                "memory-correction",
                rg_core::MemoryType::Correction,
                "Person A never worked at Company B.",
                vec![rg_core::MemoryId::new("memory-old")],
            ),
        }))
        .expect("correction recorded");

        assert_eq!(
            log.state()
                .agent_memories
                .get(&rg_core::MemoryId::new("memory-old"))
                .expect("old memory retained")
                .status,
            rg_core::MemoryStatus::Superseded
        );
        assert!(log
            .state()
            .agent_memories
            .contains_key(&rg_core::MemoryId::new("memory-correction")));
    }

    #[test]
    fn appended_events_have_monotonic_transaction_time_and_deterministic_ids() {
        let mut log = EventLog::new(TxTime::new(41));

        let source = log.execute(source_command()).expect("source added");
        let entity = log
            .execute(entity_command("person-a"))
            .expect("entity added");

        assert_eq!(
            source.event_id().as_str(),
            "evt-000000000000000001-source-added"
        );
        assert_eq!(
            entity.event_id().as_str(),
            "evt-000000000000000002-entity-created"
        );
        assert_eq!(source.transaction_time(), TxTime::new(42));
        assert_eq!(entity.transaction_time(), TxTime::new(43));
    }

    #[test]
    fn events_replay_from_zero_to_rebuild_graph_state_deterministically() {
        let mut log = EventLog::new(TxTime::new(0));
        log.execute(source_command()).expect("source added");
        log.execute(entity_command("person-a"))
            .expect("subject added");
        log.execute(entity_command("company-b"))
            .expect("object added");
        log.execute(assertion_command()).expect("assertion added");

        let rebuilt = GraphState::replay(log.events()).expect("replay succeeds");

        assert_eq!(rebuilt, *log.state());
        assert!(rebuilt.entities.contains_key(&EntityId::new("person-a")));
        assert!(rebuilt
            .assertions
            .contains_key(&AssertionId::new("assertion-1")));
        assert_eq!(
            rebuilt
                .outgoing_assertions(&EntityId::new("person-a"))
                .len(),
            1
        );
    }

    #[test]
    fn failed_index_updates_can_be_repaired_by_replay() {
        let mut log = EventLog::new(TxTime::new(0));
        log.execute(source_command()).expect("source added");
        log.execute(entity_command("person-a"))
            .expect("subject added");
        log.execute(entity_command("company-b"))
            .expect("object added");
        log.execute(assertion_command()).expect("assertion added");

        log.state_mut_for_tests()
            .assertions
            .remove(&AssertionId::new("assertion-1"));
        assert!(!log
            .state()
            .assertions
            .contains_key(&AssertionId::new("assertion-1")));

        log.rebuild_indexes()
            .expect("replay repairs materialized state");

        assert!(log
            .state()
            .assertions
            .contains_key(&AssertionId::new("assertion-1")));
        assert_eq!(
            log.state()
                .outgoing_assertions(&EntityId::new("person-a"))
                .len(),
            1
        );
    }
}
