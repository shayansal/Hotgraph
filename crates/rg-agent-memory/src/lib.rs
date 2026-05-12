//! Agent memory lifecycle APIs for Reality Graph.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rg_core::{
    AgentId, AgentMemory, Confidence, EntityId, MemoryId, MemoryStatus, MemoryType, SourceId,
    TimeInterval, TxTime, ValidTime,
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AgentMemoryKind {
    Episodic,
    Semantic,
    Procedural,
    Preference,
    Goal,
    Plan,
    Reflection,
    Correction,
    Relationship,
    WorldState,
}

impl From<AgentMemoryKind> for MemoryType {
    fn from(kind: AgentMemoryKind) -> Self {
        match kind {
            AgentMemoryKind::Episodic => Self::Episodic,
            AgentMemoryKind::Semantic => Self::Semantic,
            AgentMemoryKind::Procedural => Self::Procedural,
            AgentMemoryKind::Preference => Self::Preference,
            AgentMemoryKind::Goal => Self::Goal,
            AgentMemoryKind::Plan => Self::Plan,
            AgentMemoryKind::Reflection => Self::Reflection,
            AgentMemoryKind::Correction => Self::Correction,
            AgentMemoryKind::Relationship => Self::Relationship,
            AgentMemoryKind::WorldState => Self::WorldState,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemoryRecord {
    pub id: MemoryId,
    pub agent_id: AgentId,
    pub memory_type: AgentMemoryKind,
    pub content: String,
    pub valid_time: TimeInterval<ValidTime>,
    pub confidence: Confidence,
    pub source_ids: Vec<SourceId>,
    pub related_entities: Vec<EntityId>,
    pub supersedes: Vec<MemoryId>,
    pub contradicts: Vec<MemoryId>,
    pub lifecycle: MemoryStatus,
    pub permissions: MemoryPermissions,
    pub compressed_from: Vec<MemoryId>,
    pub created_tx: TxTime,
    pub updated_tx: TxTime,
}

impl MemoryRecord {
    pub fn from_write(write: WriteMemory) -> Self {
        Self {
            id: write.id,
            agent_id: write.agent_id,
            memory_type: write.memory_type,
            content: write.content,
            valid_time: write.valid_time,
            confidence: write.confidence,
            source_ids: write.source_ids,
            related_entities: write.related_entities,
            supersedes: write.supersedes,
            contradicts: write.contradicts,
            lifecycle: write.lifecycle,
            permissions: write.permissions,
            compressed_from: Vec::new(),
            created_tx: TxTime::new(0),
            updated_tx: TxTime::new(0),
        }
    }

    pub fn to_core_memory(&self) -> AgentMemory {
        AgentMemory {
            id: self.id.clone(),
            agent_id: self.agent_id.clone(),
            memory_type: self.memory_type.into(),
            content: self.content.clone(),
            valid_time: self.valid_time.clone(),
            confidence: self.confidence,
            source_ids: self.source_ids.clone(),
            related_entities: self.related_entities.clone(),
            supersedes: self.supersedes.clone(),
            status: self.lifecycle.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryPermissions {
    pub owner_agent_id: AgentId,
    pub reader_agent_ids: Vec<AgentId>,
    pub public_read: bool,
}

impl MemoryPermissions {
    pub fn private(owner_agent_id: AgentId) -> Self {
        Self {
            owner_agent_id,
            reader_agent_ids: Vec::new(),
            public_read: false,
        }
    }

    fn can_read(&self, agent_id: &AgentId) -> bool {
        self.public_read
            || &self.owner_agent_id == agent_id
            || self.reader_agent_ids.contains(agent_id)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WriteMemory {
    pub id: MemoryId,
    pub agent_id: AgentId,
    pub memory_type: AgentMemoryKind,
    pub content: String,
    pub valid_time: TimeInterval<ValidTime>,
    pub confidence: Confidence,
    pub source_ids: Vec<SourceId>,
    pub related_entities: Vec<EntityId>,
    pub supersedes: Vec<MemoryId>,
    pub contradicts: Vec<MemoryId>,
    pub lifecycle: MemoryStatus,
    pub permissions: MemoryPermissions,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConsolidateMemory {
    pub new_id: MemoryId,
    pub agent_id: AgentId,
    pub memory_type: AgentMemoryKind,
    pub content: String,
    pub valid_time: TimeInterval<ValidTime>,
    pub confidence: Confidence,
    pub source_ids: Vec<SourceId>,
    pub related_entities: Vec<EntityId>,
    pub source_memory_ids: Vec<MemoryId>,
    pub permissions: MemoryPermissions,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SupersedeMemory {
    pub old_id: MemoryId,
    pub new_memory: WriteMemory,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemoryRetrievalMode {
    GraphTemporal,
    TranscriptVector,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemoryQuery {
    pub agent_id: AgentId,
    pub query: String,
    pub valid_at: Option<ValidTime>,
    pub related_entities: Vec<EntityId>,
    pub include_history: bool,
    pub mode: MemoryRetrievalMode,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemoryRetrieval {
    pub memories: Vec<RetrievedMemory>,
    pub quality_score: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RetrievedMemory {
    pub record: MemoryRecord,
    pub score: f32,
    pub current_truth: bool,
    pub explanation: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemoryExplanation {
    pub memory_id: MemoryId,
    pub lifecycle: MemoryStatus,
    pub current_truth: bool,
    pub reason: String,
    pub source_ids: Vec<SourceId>,
    pub supersedes: Vec<MemoryId>,
    pub superseded_by: Vec<MemoryId>,
    pub contradicted_by: Vec<MemoryId>,
    pub compressed_from: Vec<MemoryId>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MemoryJournalEvent {
    MemoryWritten {
        record: Box<MemoryRecord>,
    },
    MemoryLifecycleChanged {
        memory_id: MemoryId,
        lifecycle: MemoryStatus,
        reason: String,
        transaction_time: TxTime,
    },
    MemorySuperseded {
        old_id: MemoryId,
        new_id: MemoryId,
        reason: String,
        transaction_time: TxTime,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentMemoryError {
    DuplicateMemory(MemoryId),
    UnknownMemory(MemoryId),
    EmptyContent,
    EmptySourceList,
}

impl fmt::Display for AgentMemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateMemory(id) => write!(f, "duplicate memory {id}"),
            Self::UnknownMemory(id) => write!(f, "unknown memory {id}"),
            Self::EmptyContent => f.write_str("memory content cannot be empty"),
            Self::EmptySourceList => f.write_str("memory must include provenance source IDs"),
        }
    }
}

impl std::error::Error for AgentMemoryError {}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentMemoryService {
    memories: BTreeMap<MemoryId, MemoryRecord>,
    journal: Vec<MemoryJournalEvent>,
    last_tx: TxTime,
    superseded_by: BTreeMap<MemoryId, Vec<MemoryId>>,
    contradicted_by: BTreeMap<MemoryId, Vec<MemoryId>>,
    reasons: BTreeMap<MemoryId, Vec<String>>,
}

impl AgentMemoryService {
    pub fn new(start_tx: TxTime) -> Self {
        Self {
            memories: BTreeMap::new(),
            journal: Vec::new(),
            last_tx: start_tx,
            superseded_by: BTreeMap::new(),
            contradicted_by: BTreeMap::new(),
            reasons: BTreeMap::new(),
        }
    }

    pub fn replay(events: &[MemoryJournalEvent]) -> Result<Self, AgentMemoryError> {
        let mut service = Self::new(TxTime::new(0));
        for event in events {
            service.apply_event(event.clone())?;
            service.journal.push(event.clone());
        }
        Ok(service)
    }

    pub fn journal(&self) -> &[MemoryJournalEvent] {
        &self.journal
    }

    pub fn memory(&self, memory_id: &MemoryId) -> Option<&MemoryRecord> {
        self.memories.get(memory_id)
    }

    pub fn write_memory(&mut self, write: WriteMemory) -> Result<MemoryRecord, AgentMemoryError> {
        self.validate_write(&write)?;
        let mut record = MemoryRecord::from_write(write);
        let tx = self.next_tx();
        record.created_tx = tx;
        record.updated_tx = tx;
        self.append_event(MemoryJournalEvent::MemoryWritten {
            record: Box::new(record.clone()),
        })?;
        Ok(record)
    }

    pub fn consolidate_memory(
        &mut self,
        command: ConsolidateMemory,
    ) -> Result<MemoryRecord, AgentMemoryError> {
        for memory_id in &command.source_memory_ids {
            if !self.memories.contains_key(memory_id) {
                return Err(AgentMemoryError::UnknownMemory(memory_id.clone()));
            }
        }

        let write = WriteMemory {
            id: command.new_id,
            agent_id: command.agent_id,
            memory_type: command.memory_type,
            content: command.content,
            valid_time: command.valid_time,
            confidence: command.confidence,
            source_ids: command.source_ids,
            related_entities: command.related_entities,
            supersedes: Vec::new(),
            contradicts: Vec::new(),
            lifecycle: MemoryStatus::Active,
            permissions: command.permissions,
        };
        self.validate_write(&write)?;

        let mut record = MemoryRecord::from_write(write);
        record.compressed_from = command.source_memory_ids.clone();
        let tx = self.next_tx();
        record.created_tx = tx;
        record.updated_tx = tx;
        self.append_event(MemoryJournalEvent::MemoryWritten {
            record: Box::new(record.clone()),
        })?;

        for source_id in command.source_memory_ids {
            let reason = format!("consolidated into {}", record.id);
            let transaction_time = self.next_tx();
            self.append_event(MemoryJournalEvent::MemoryLifecycleChanged {
                memory_id: source_id,
                lifecycle: MemoryStatus::Archived,
                reason,
                transaction_time,
            })?;
        }

        Ok(record)
    }

    pub fn supersede_memory(
        &mut self,
        mut command: SupersedeMemory,
    ) -> Result<MemoryRecord, AgentMemoryError> {
        if !self.memories.contains_key(&command.old_id) {
            return Err(AgentMemoryError::UnknownMemory(command.old_id));
        }
        if !command.new_memory.supersedes.contains(&command.old_id) {
            command.new_memory.supersedes.push(command.old_id.clone());
        }
        let record = self.write_memory(command.new_memory)?;
        let transaction_time = self.next_tx();
        self.append_event(MemoryJournalEvent::MemorySuperseded {
            old_id: command.old_id,
            new_id: record.id.clone(),
            reason: command.reason,
            transaction_time,
        })?;
        Ok(record)
    }

    pub fn retrieve_memory(&self, query: MemoryQuery) -> MemoryRetrieval {
        let mut memories = self
            .memories
            .values()
            .filter(|record| record.permissions.can_read(&query.agent_id))
            .filter(|record| {
                if query.mode == MemoryRetrievalMode::GraphTemporal {
                    query
                        .valid_at
                        .map_or(true, |valid_at| record.valid_time.contains(valid_at))
                } else {
                    true
                }
            })
            .filter(|record| {
                query.include_history
                    || query.mode == MemoryRetrievalMode::TranscriptVector
                    || is_current_truth(&record.lifecycle)
            })
            .map(|record| {
                let score = match query.mode {
                    MemoryRetrievalMode::GraphTemporal => graph_temporal_score(record, &query),
                    MemoryRetrievalMode::TranscriptVector => {
                        transcript_vector_score(record, &query)
                    }
                };
                RetrievedMemory {
                    record: record.clone(),
                    score,
                    current_truth: is_current_truth(&record.lifecycle),
                    explanation: retrieval_explanation(record, &query),
                }
            })
            .filter(|memory| memory.score > 0.0)
            .collect::<Vec<_>>();

        memories.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.record.id.cmp(&right.record.id))
        });
        if let Some(limit) = query.limit {
            memories.truncate(limit);
        }

        let quality_score = retrieval_quality(&query.mode, &memories);
        MemoryRetrieval {
            memories,
            quality_score,
        }
    }

    pub fn explain_memory(&self, memory_id: &MemoryId) -> Option<MemoryExplanation> {
        let record = self.memories.get(memory_id)?;
        let superseded_by = self
            .superseded_by
            .get(memory_id)
            .cloned()
            .unwrap_or_default();
        let contradicted_by = self
            .contradicted_by
            .get(memory_id)
            .cloned()
            .unwrap_or_default();
        let reason = self
            .reasons
            .get(memory_id)
            .map(|reasons| reasons.join("; "))
            .unwrap_or_else(|| default_reason(record));
        Some(MemoryExplanation {
            memory_id: memory_id.clone(),
            lifecycle: record.lifecycle.clone(),
            current_truth: is_current_truth(&record.lifecycle),
            reason,
            source_ids: record.source_ids.clone(),
            supersedes: record.supersedes.clone(),
            superseded_by,
            contradicted_by,
            compressed_from: record.compressed_from.clone(),
        })
    }

    fn append_event(&mut self, event: MemoryJournalEvent) -> Result<(), AgentMemoryError> {
        self.apply_event(event.clone())?;
        self.journal.push(event);
        Ok(())
    }

    fn apply_event(&mut self, event: MemoryJournalEvent) -> Result<(), AgentMemoryError> {
        match event {
            MemoryJournalEvent::MemoryWritten { record } => {
                let record = *record;
                self.apply_record_references(&record)?;
                self.last_tx = self.last_tx.max(record.updated_tx);
                self.memories.insert(record.id.clone(), record);
            }
            MemoryJournalEvent::MemoryLifecycleChanged {
                memory_id,
                lifecycle,
                reason,
                transaction_time,
            } => {
                let memory = self
                    .memories
                    .get_mut(&memory_id)
                    .ok_or_else(|| AgentMemoryError::UnknownMemory(memory_id.clone()))?;
                memory.lifecycle = lifecycle;
                memory.updated_tx = transaction_time;
                self.last_tx = self.last_tx.max(transaction_time);
                self.reasons.entry(memory_id).or_default().push(reason);
            }
            MemoryJournalEvent::MemorySuperseded {
                old_id,
                new_id,
                reason,
                transaction_time,
            } => {
                let memory = self
                    .memories
                    .get_mut(&old_id)
                    .ok_or_else(|| AgentMemoryError::UnknownMemory(old_id.clone()))?;
                memory.lifecycle = MemoryStatus::Superseded;
                memory.updated_tx = transaction_time;
                self.last_tx = self.last_tx.max(transaction_time);
                self.superseded_by
                    .entry(old_id.clone())
                    .or_default()
                    .push(new_id.clone());
                self.reasons
                    .entry(old_id)
                    .or_default()
                    .push(format!("{reason}; superseded by {new_id}"));
            }
        }
        Ok(())
    }

    fn apply_record_references(&mut self, record: &MemoryRecord) -> Result<(), AgentMemoryError> {
        for old_id in &record.supersedes {
            let memory = self
                .memories
                .get_mut(old_id)
                .ok_or_else(|| AgentMemoryError::UnknownMemory(old_id.clone()))?;
            memory.lifecycle = MemoryStatus::Superseded;
            memory.updated_tx = record.updated_tx;
            self.superseded_by
                .entry(old_id.clone())
                .or_default()
                .push(record.id.clone());
            self.reasons
                .entry(old_id.clone())
                .or_default()
                .push(format!("superseded by {}", record.id));
        }

        for old_id in &record.contradicts {
            let memory = self
                .memories
                .get_mut(old_id)
                .ok_or_else(|| AgentMemoryError::UnknownMemory(old_id.clone()))?;
            memory.lifecycle = MemoryStatus::Contradicted;
            memory.updated_tx = record.updated_tx;
            self.contradicted_by
                .entry(old_id.clone())
                .or_default()
                .push(record.id.clone());
            self.reasons
                .entry(old_id.clone())
                .or_default()
                .push(format!("contradicted by {}", record.id));
        }
        Ok(())
    }

    fn validate_write(&self, write: &WriteMemory) -> Result<(), AgentMemoryError> {
        if self.memories.contains_key(&write.id) {
            return Err(AgentMemoryError::DuplicateMemory(write.id.clone()));
        }
        if write.content.trim().is_empty() {
            return Err(AgentMemoryError::EmptyContent);
        }
        if write.source_ids.is_empty() {
            return Err(AgentMemoryError::EmptySourceList);
        }
        for memory_id in write.supersedes.iter().chain(write.contradicts.iter()) {
            if !self.memories.contains_key(memory_id) {
                return Err(AgentMemoryError::UnknownMemory(memory_id.clone()));
            }
        }
        Ok(())
    }

    fn next_tx(&mut self) -> TxTime {
        let next = TxTime::new(self.last_tx.as_i64() + 1);
        self.last_tx = next;
        next
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EpisodicMemory(pub MemoryRecord);
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticMemory(pub MemoryRecord);
#[derive(Clone, Debug, PartialEq)]
pub struct ProceduralMemory(pub MemoryRecord);
#[derive(Clone, Debug, PartialEq)]
pub struct PreferenceMemory(pub MemoryRecord);
#[derive(Clone, Debug, PartialEq)]
pub struct GoalMemory(pub MemoryRecord);
#[derive(Clone, Debug, PartialEq)]
pub struct PlanMemory(pub MemoryRecord);
#[derive(Clone, Debug, PartialEq)]
pub struct ReflectionMemory(pub MemoryRecord);
#[derive(Clone, Debug, PartialEq)]
pub struct CorrectionMemory(pub MemoryRecord);
#[derive(Clone, Debug, PartialEq)]
pub struct RelationshipMemory(pub MemoryRecord);
#[derive(Clone, Debug, PartialEq)]
pub struct WorldStateMemory(pub MemoryRecord);

fn graph_temporal_score(record: &MemoryRecord, query: &MemoryQuery) -> f32 {
    let lexical = lexical_score(&query.query, &record.content);
    let entity_score = if query.related_entities.is_empty() {
        0.0
    } else {
        query
            .related_entities
            .iter()
            .filter(|entity_id| record.related_entities.contains(entity_id))
            .count() as f32
            / query.related_entities.len() as f32
    };
    let lifecycle_score = match record.lifecycle {
        MemoryStatus::Active => 1.0,
        MemoryStatus::Reinforced => 1.2,
        MemoryStatus::Superseded | MemoryStatus::Contradicted => 0.15,
        MemoryStatus::Candidate | MemoryStatus::Archived => 0.05,
    };
    let type_score = match record.memory_type {
        AgentMemoryKind::Correction => 0.35,
        AgentMemoryKind::Semantic | AgentMemoryKind::Relationship | AgentMemoryKind::WorldState => {
            0.2
        }
        AgentMemoryKind::Preference | AgentMemoryKind::Goal | AgentMemoryKind::Plan => 0.15,
        AgentMemoryKind::Episodic | AgentMemoryKind::Procedural | AgentMemoryKind::Reflection => {
            0.1
        }
    };

    ((lexical * 0.45) + (entity_score * 0.35) + type_score + record.confidence.as_f32() * 0.2)
        * lifecycle_score
}

fn transcript_vector_score(record: &MemoryRecord, query: &MemoryQuery) -> f32 {
    lexical_score(&query.query, &record.content) * record.confidence.as_f32()
}

fn lexical_score(query: &str, content: &str) -> f32 {
    let query_tokens = tokens(query);
    if query_tokens.is_empty() {
        return 0.0;
    }
    let content_tokens = tokens(content);
    let overlap = query_tokens
        .iter()
        .filter(|token| content_tokens.contains(*token))
        .count();
    overlap as f32 / query_tokens.len() as f32
}

fn tokens(text: &str) -> BTreeSet<String> {
    text.split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

fn is_current_truth(status: &MemoryStatus) -> bool {
    matches!(status, MemoryStatus::Active | MemoryStatus::Reinforced)
}

fn retrieval_explanation(record: &MemoryRecord, query: &MemoryQuery) -> String {
    let mut parts = Vec::new();
    if is_current_truth(&record.lifecycle) {
        parts.push("current memory".to_owned());
    } else {
        parts.push(format!("historical {:?} memory", record.lifecycle));
    }
    let entity_hits = query
        .related_entities
        .iter()
        .filter(|entity_id| record.related_entities.contains(entity_id))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if !entity_hits.is_empty() {
        parts.push(format!("related entities: {}", entity_hits.join(", ")));
    }
    if !record.source_ids.is_empty() {
        parts.push(format!(
            "provenance: {}",
            record
                .source_ids
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    parts.join("; ")
}

fn retrieval_quality(mode: &MemoryRetrievalMode, memories: &[RetrievedMemory]) -> f32 {
    let Some(top) = memories.first() else {
        return 0.0;
    };
    match mode {
        MemoryRetrievalMode::GraphTemporal if top.current_truth => 1.0,
        MemoryRetrievalMode::GraphTemporal => 0.35,
        MemoryRetrievalMode::TranscriptVector if top.current_truth => 0.65,
        MemoryRetrievalMode::TranscriptVector => 0.2,
    }
}

fn default_reason(record: &MemoryRecord) -> String {
    match record.lifecycle {
        MemoryStatus::Candidate => "candidate memory awaiting consolidation".to_owned(),
        MemoryStatus::Active => "active memory treated as current truth".to_owned(),
        MemoryStatus::Reinforced => "reinforced memory treated as current truth".to_owned(),
        MemoryStatus::Superseded => "superseded memory retained for history".to_owned(),
        MemoryStatus::Contradicted => "contradicted memory retained for auditability".to_owned(),
        MemoryStatus::Archived => "archived memory retained for replay".to_owned(),
    }
}
