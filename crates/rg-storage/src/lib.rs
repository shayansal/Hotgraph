//! Single-node storage primitives for Reality Graph.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use rg_events::{
    AgentId, AgentMemory, AgentMemoryRecorded, Assertion, AssertionAdded, AssertionId,
    AssertionRetracted, AssertionStatus, CausalLink, CausalLinkAdded, CausalLinkId, Confidence,
    ConfidenceUpdated, ContentHash, ContextScope, Entity, EntityCreated, EntityId, EntityMerged,
    EntityType, EventId, EvidenceLinked, GraphEvent, GraphReplayError, GraphState, GraphValue,
    MemoryId, MemoryStatus, MemoryType, PredicateId, PropertyKey, PropertyMap, Source, SourceAdded,
    SourceId, SourceType, TimeInterval, TxTime, ValidTime,
};

#[derive(Debug, Eq, PartialEq)]
pub enum StorageError {
    AppendFailed,
    Io(String),
    Codec(String),
    Replay(GraphReplayError),
    SnapshotMismatch,
}

const EVENT_RECORD_KIND: &str = "RGEVENT";
const EVENT_RECORD_VERSION: &str = "1";
const SNAPSHOT_HEADER: &str = "RGSTORAGE-SNAPSHOT-V2";
const LEGACY_SNAPSHOT_HEADER: &str = "RGSTORAGE-SNAPSHOT-V1";

pub trait GraphStore {
    fn append(&mut self, event: GraphEvent) -> Result<(), StorageError>;
    fn events(&self) -> &[GraphEvent];
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InMemoryStorage {
    events: Vec<GraphEvent>,
    state: GraphState,
    indexes: StorageIndexes,
}

pub type InMemoryStore = InMemoryStorage;

impl InMemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn replay(events: &[GraphEvent]) -> Result<Self, StorageError> {
        let state = GraphState::replay(events).map_err(StorageError::Replay)?;
        Ok(Self {
            events: events.to_vec(),
            indexes: StorageIndexes::from_state(&state),
            state,
        })
    }

    pub fn append_event(&mut self, event: GraphEvent) -> Result<(), StorageError> {
        self.state
            .apply_event(&event)
            .map_err(StorageError::Replay)?;
        self.indexes.apply_event(&event, &self.state);
        self.events.push(event);
        Ok(())
    }

    pub fn entity(&self, id: &EntityId) -> Option<&Entity> {
        self.state.entities.get(id)
    }

    pub fn assertion(&self, id: &AssertionId) -> Option<&Assertion> {
        self.state.assertions.get(id)
    }

    pub fn source(&self, id: &SourceId) -> Option<&Source> {
        self.state.sources.get(id)
    }

    pub fn assertions_by_subject(&self, subject: &EntityId) -> Vec<&Assertion> {
        self.assertions_for_ids(self.indexes.by_subject.get(subject))
    }

    pub fn assertions_by_predicate(&self, predicate: &PredicateId) -> Vec<&Assertion> {
        self.assertions_for_ids(self.indexes.by_predicate.get(predicate))
    }

    pub fn assertions_by_object(&self, object: &GraphValue) -> Vec<&Assertion> {
        let key = ObjectIndexKey::from(object);
        self.assertions_for_ids(self.indexes.by_object.get(&key))
    }

    pub fn adjacent_edges(&self, entity: &EntityId) -> Vec<&Assertion> {
        self.assertions_for_ids(self.indexes.by_entity.get(entity))
    }

    pub fn assertions_valid_at(&self, instant: ValidTime) -> Vec<&Assertion> {
        self.indexes
            .by_valid_time
            .values()
            .flatten()
            .filter_map(|id| self.state.assertions.get(id))
            .filter(|assertion| assertion.valid_time.contains(instant))
            .collect()
    }

    pub fn assertions_tx_at(&self, instant: TxTime) -> Vec<&Assertion> {
        self.indexes
            .by_tx_time
            .values()
            .flatten()
            .filter_map(|id| self.state.assertions.get(id))
            .filter(|assertion| assertion.transaction_time.contains(instant))
            .collect()
    }

    pub fn assertions_by_source(&self, source: &SourceId) -> Vec<&Assertion> {
        self.assertions_for_ids(self.indexes.by_source.get(source))
    }

    pub fn events(&self) -> &[GraphEvent] {
        &self.events
    }

    pub fn graph_state(&self) -> &GraphState {
        &self.state
    }

    fn assertions_for_ids(&self, ids: Option<&Vec<AssertionId>>) -> Vec<&Assertion> {
        ids.into_iter()
            .flatten()
            .filter_map(|id| self.state.assertions.get(id))
            .collect()
    }
}

impl GraphStore for InMemoryStorage {
    fn append(&mut self, event: GraphEvent) -> Result<(), StorageError> {
        self.append_event(event)
    }

    fn events(&self) -> &[GraphEvent] {
        &self.events
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct StorageIndexes {
    by_subject: BTreeMap<EntityId, Vec<AssertionId>>,
    by_predicate: BTreeMap<PredicateId, Vec<AssertionId>>,
    by_object: BTreeMap<ObjectIndexKey, Vec<AssertionId>>,
    by_entity: BTreeMap<EntityId, Vec<AssertionId>>,
    by_valid_time: BTreeMap<i64, Vec<AssertionId>>,
    by_tx_time: BTreeMap<i64, Vec<AssertionId>>,
    by_source: BTreeMap<SourceId, Vec<AssertionId>>,
}

impl StorageIndexes {
    fn from_state(state: &GraphState) -> Self {
        let mut indexes = Self::default();
        for assertion in state.assertions.values() {
            indexes.index_assertion(assertion);
        }
        indexes
    }

    fn apply_event(&mut self, event: &GraphEvent, state: &GraphState) {
        match event {
            GraphEvent::AssertionAdded(event) => self.index_assertion(&event.assertion),
            GraphEvent::EvidenceLinked(event)
                if state.assertions.contains_key(&event.assertion_id) =>
            {
                push_index(
                    &mut self.by_source,
                    event.source_id.clone(),
                    event.assertion_id.clone(),
                );
            }
            _ => {}
        }
    }

    fn index_assertion(&mut self, assertion: &Assertion) {
        push_index(
            &mut self.by_subject,
            assertion.subject.clone(),
            assertion.id.clone(),
        );
        push_index(
            &mut self.by_predicate,
            assertion.predicate.clone(),
            assertion.id.clone(),
        );
        push_index(
            &mut self.by_object,
            ObjectIndexKey::from(&assertion.object),
            assertion.id.clone(),
        );
        push_index(
            &mut self.by_entity,
            assertion.subject.clone(),
            assertion.id.clone(),
        );
        if let GraphValue::Entity(entity_id) = &assertion.object {
            push_index(&mut self.by_entity, entity_id.clone(), assertion.id.clone());
        }
        push_index(
            &mut self.by_valid_time,
            assertion.valid_time.start.as_i64(),
            assertion.id.clone(),
        );
        push_index(
            &mut self.by_tx_time,
            assertion.transaction_time.start.as_i64(),
            assertion.id.clone(),
        );
        for source_id in &assertion.source_ids {
            push_index(&mut self.by_source, source_id.clone(), assertion.id.clone());
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum ObjectIndexKey {
    Entity(EntityId),
    Text(String),
    Integer(i64),
    DecimalBits(u64),
    Boolean(bool),
    Time(ValidTime),
    Null,
}

impl From<&GraphValue> for ObjectIndexKey {
    fn from(value: &GraphValue) -> Self {
        match value {
            GraphValue::Entity(id) => Self::Entity(id.clone()),
            GraphValue::Text(value) => Self::Text(value.clone()),
            GraphValue::Integer(value) => Self::Integer(*value),
            GraphValue::Decimal(value) => Self::DecimalBits(value.to_bits()),
            GraphValue::Boolean(value) => Self::Boolean(*value),
            GraphValue::Time(value) => Self::Time(*value),
            GraphValue::Null => Self::Null,
        }
    }
}

fn push_index<K>(index: &mut BTreeMap<K, Vec<AssertionId>>, key: K, assertion_id: AssertionId)
where
    K: Ord,
{
    let ids = index.entry(key).or_default();
    ids.push(assertion_id);
    ids.sort();
    ids.dedup();
}

#[derive(Clone, Debug)]
pub struct FileEventLog {
    path: PathBuf,
}

impl FileEventLog {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| StorageError::Io(error.to_string()))?;
        Ok(Self { path })
    }

    pub fn append(&mut self, event: &GraphEvent) -> Result<(), StorageError> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| StorageError::Io(error.to_string()))?;
        file.write_all(encode_event_record(event).as_bytes())
            .map_err(|error| StorageError::Io(error.to_string()))?;
        file.write_all(b"\n")
            .map_err(|error| StorageError::Io(error.to_string()))?;
        file.sync_data()
            .map_err(|error| StorageError::Io(error.to_string()))?;
        Ok(())
    }

    pub fn read_all(&self) -> Result<Vec<GraphEvent>, StorageError> {
        let file = File::open(&self.path).map_err(|error| StorageError::Io(error.to_string()))?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();
        for line in reader.lines() {
            let line = line.map_err(|error| StorageError::Io(error.to_string()))?;
            if line.trim().is_empty() {
                continue;
            }
            events.push(decode_event_record(&line)?);
        }
        Ok(events)
    }
}

pub struct SnapshotWriter;

impl SnapshotWriter {
    pub fn write(path: impl AsRef<Path>, storage: &InMemoryStorage) -> Result<(), StorageError> {
        let mut file = File::create(path).map_err(|error| StorageError::Io(error.to_string()))?;
        file.write_all(SNAPSHOT_HEADER.as_bytes())
            .map_err(|error| StorageError::Io(error.to_string()))?;
        file.write_all(b"\n")
            .map_err(|error| StorageError::Io(error.to_string()))?;
        file.write_all(
            encode_snapshot_manifest(&SnapshotManifest::from_storage(storage)).as_bytes(),
        )
        .map_err(|error| StorageError::Io(error.to_string()))?;
        file.write_all(b"\n")
            .map_err(|error| StorageError::Io(error.to_string()))?;
        for event in storage.events() {
            file.write_all(encode_event_record(event).as_bytes())
                .map_err(|error| StorageError::Io(error.to_string()))?;
            file.write_all(b"\n")
                .map_err(|error| StorageError::Io(error.to_string()))?;
        }
        file.sync_data()
            .map_err(|error| StorageError::Io(error.to_string()))
    }
}

pub struct SnapshotReader;

impl SnapshotReader {
    pub fn read(path: impl AsRef<Path>) -> Result<InMemoryStorage, StorageError> {
        let file = File::open(path).map_err(|error| StorageError::Io(error.to_string()))?;
        let mut lines = BufReader::new(file).lines();
        let header = lines
            .next()
            .ok_or_else(|| StorageError::Codec("snapshot is empty".to_owned()))?
            .map_err(|error| StorageError::Io(error.to_string()))?;
        if header == LEGACY_SNAPSHOT_HEADER {
            let mut events = Vec::new();
            for line in lines {
                let line = line.map_err(|error| StorageError::Io(error.to_string()))?;
                if line.trim().is_empty() {
                    continue;
                }
                events.push(decode_event_record(&line)?);
            }
            return InMemoryStorage::replay(&events);
        }
        if header != SNAPSHOT_HEADER {
            return Err(StorageError::Codec("invalid snapshot header".to_owned()));
        }
        let manifest_line = lines
            .next()
            .ok_or_else(|| StorageError::Codec("snapshot manifest is missing".to_owned()))?
            .map_err(|error| StorageError::Io(error.to_string()))?;
        let manifest = decode_snapshot_manifest(&manifest_line)?;
        let mut events = Vec::new();
        for line in lines {
            let line = line.map_err(|error| StorageError::Io(error.to_string()))?;
            if line.trim().is_empty() {
                continue;
            }
            events.push(decode_event_record(&line)?);
        }
        let storage = InMemoryStorage::replay(&events)?;
        if SnapshotManifest::from_storage(&storage) != manifest {
            return Err(StorageError::SnapshotMismatch);
        }
        Ok(storage)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotManifest {
    pub schema_version: u32,
    pub event_count: usize,
    pub last_event_id: Option<String>,
    pub event_checksum: String,
}

impl SnapshotManifest {
    fn from_storage(storage: &InMemoryStorage) -> Self {
        Self {
            schema_version: 2,
            event_count: storage.events().len(),
            last_event_id: storage
                .events()
                .last()
                .map(|event| event.event_id().as_str().to_owned()),
            event_checksum: checksum_events(storage.events()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupManifest {
    pub event_count: usize,
    pub entity_count: usize,
    pub assertion_count: usize,
    pub source_count: usize,
    pub last_event_id: Option<String>,
}

impl BackupManifest {
    fn from_storage(storage: &InMemoryStorage) -> Self {
        Self {
            event_count: storage.events().len(),
            entity_count: storage.graph_state().entities.len(),
            assertion_count: storage.graph_state().assertions.len(),
            source_count: storage.graph_state().sources.len(),
            last_event_id: storage
                .events()
                .last()
                .map(|event| event.event_id().as_str().to_owned()),
        }
    }
}

pub struct BackupWriter;

impl BackupWriter {
    pub fn write(
        path: impl AsRef<Path>,
        storage: &InMemoryStorage,
    ) -> Result<BackupManifest, StorageError> {
        let manifest = BackupManifest::from_storage(storage);
        let mut file = File::create(path).map_err(|error| StorageError::Io(error.to_string()))?;
        file.write_all(b"RGSTORAGE-BACKUP-V1\n")
            .map_err(|error| StorageError::Io(error.to_string()))?;
        file.write_all(encode_backup_manifest(&manifest).as_bytes())
            .map_err(|error| StorageError::Io(error.to_string()))?;
        file.write_all(b"\n")
            .map_err(|error| StorageError::Io(error.to_string()))?;
        for event in storage.events() {
            file.write_all(encode_event_record(event).as_bytes())
                .map_err(|error| StorageError::Io(error.to_string()))?;
            file.write_all(b"\n")
                .map_err(|error| StorageError::Io(error.to_string()))?;
        }
        file.sync_data()
            .map_err(|error| StorageError::Io(error.to_string()))?;
        Ok(manifest)
    }
}

pub struct BackupReader;

impl BackupReader {
    pub fn manifest(path: impl AsRef<Path>) -> Result<BackupManifest, StorageError> {
        read_backup(path).map(|(manifest, _)| manifest)
    }

    pub fn restore(path: impl AsRef<Path>) -> Result<InMemoryStorage, StorageError> {
        let (manifest, events) = read_backup(path)?;
        let storage = InMemoryStorage::replay(&events)?;
        if BackupManifest::from_storage(&storage) != manifest {
            return Err(StorageError::SnapshotMismatch);
        }
        Ok(storage)
    }
}

fn read_backup(path: impl AsRef<Path>) -> Result<(BackupManifest, Vec<GraphEvent>), StorageError> {
    let file = File::open(path).map_err(|error| StorageError::Io(error.to_string()))?;
    let mut lines = BufReader::new(file).lines();
    let header = lines
        .next()
        .ok_or_else(|| StorageError::Codec("backup is empty".to_owned()))?
        .map_err(|error| StorageError::Io(error.to_string()))?;
    if header != "RGSTORAGE-BACKUP-V1" {
        return Err(StorageError::Codec("invalid backup header".to_owned()));
    }
    let manifest_line = lines
        .next()
        .ok_or_else(|| StorageError::Codec("backup manifest is missing".to_owned()))?
        .map_err(|error| StorageError::Io(error.to_string()))?;
    let manifest = decode_backup_manifest(&manifest_line)?;
    let mut events = Vec::new();
    for line in lines {
        let line = line.map_err(|error| StorageError::Io(error.to_string()))?;
        if line.trim().is_empty() {
            continue;
        }
        events.push(decode_event_record(&line)?);
    }
    Ok((manifest, events))
}

fn encode_backup_manifest(manifest: &BackupManifest) -> String {
    encode_parts(&[
        manifest.event_count.to_string(),
        manifest.entity_count.to_string(),
        manifest.assertion_count.to_string(),
        manifest.source_count.to_string(),
        manifest.last_event_id.clone().unwrap_or_default(),
    ])
}

fn encode_snapshot_manifest(manifest: &SnapshotManifest) -> String {
    encode_parts(&[
        "snapshot_manifest".to_owned(),
        "schema_version".to_owned(),
        manifest.schema_version.to_string(),
        "event_count".to_owned(),
        manifest.event_count.to_string(),
        "last_event_id".to_owned(),
        manifest.last_event_id.clone().unwrap_or_default(),
        "event_checksum".to_owned(),
        manifest.event_checksum.clone(),
    ])
}

fn decode_snapshot_manifest(record: &str) -> Result<SnapshotManifest, StorageError> {
    let parts = decode_parts(record)?;
    if required(&parts, 0, "snapshot manifest kind")? != "snapshot_manifest" {
        return Err(StorageError::Codec(
            "invalid snapshot manifest kind".to_owned(),
        ));
    }
    if required(&parts, 1, "schema version key")? != "schema_version"
        || required(&parts, 3, "event count key")? != "event_count"
        || required(&parts, 5, "last event id key")? != "last_event_id"
        || required(&parts, 7, "event checksum key")? != "event_checksum"
    {
        return Err(StorageError::Codec(
            "invalid snapshot manifest fields".to_owned(),
        ));
    }
    let last_event_id = match required(&parts, 6, "last event id")? {
        "" => None,
        value => Some(value.to_owned()),
    };
    Ok(SnapshotManifest {
        schema_version: parse_u32(required(&parts, 2, "schema version")?)?,
        event_count: parse_usize(required(&parts, 4, "event count")?)?,
        last_event_id,
        event_checksum: required(&parts, 8, "event checksum")?.to_owned(),
    })
}

fn decode_backup_manifest(record: &str) -> Result<BackupManifest, StorageError> {
    let parts = decode_parts(record)?;
    let last_event_id = match required(&parts, 4, "last event id")? {
        "" => None,
        value => Some(value.to_owned()),
    };
    Ok(BackupManifest {
        event_count: parse_usize(required(&parts, 0, "event count")?)?,
        entity_count: parse_usize(required(&parts, 1, "entity count")?)?,
        assertion_count: parse_usize(required(&parts, 2, "assertion count")?)?,
        source_count: parse_usize(required(&parts, 3, "source count")?)?,
        last_event_id,
    })
}

fn encode_event_record(event: &GraphEvent) -> String {
    let payload = encode_event(event);
    encode_parts(&[
        EVENT_RECORD_KIND.to_owned(),
        EVENT_RECORD_VERSION.to_owned(),
        checksum_hex(payload.as_bytes()),
        payload,
    ])
}

fn decode_event_record(record: &str) -> Result<GraphEvent, StorageError> {
    let parts = decode_parts(record)?;
    if parts
        .first()
        .is_some_and(|kind| kind.as_str() == EVENT_RECORD_KIND)
    {
        if required(&parts, 1, "event record version")? != EVENT_RECORD_VERSION {
            return Err(StorageError::Codec(
                "unsupported event record version".to_owned(),
            ));
        }
        let checksum = required(&parts, 2, "event checksum")?;
        let payload = required(&parts, 3, "event payload")?;
        let actual = checksum_hex(payload.as_bytes());
        if checksum != actual {
            return Err(StorageError::Codec(format!(
                "event checksum mismatch: expected {checksum}, got {actual}"
            )));
        }
        return decode_event(payload);
    }
    decode_event(record)
}

fn checksum_events(events: &[GraphEvent]) -> String {
    let mut bytes = Vec::new();
    for event in events {
        bytes.extend_from_slice(encode_event(event).as_bytes());
        bytes.push(b'\n');
    }
    checksum_hex(&bytes)
}

fn checksum_hex(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn encode_event(event: &GraphEvent) -> String {
    match event {
        GraphEvent::EntityCreated(event) => encode_parts(&[
            "EntityCreated".to_owned(),
            event.event_id.as_str().to_owned(),
            event.transaction_time.as_i64().to_string(),
            encode_entity(&event.entity),
        ]),
        GraphEvent::AssertionAdded(event) => encode_parts(&[
            "AssertionAdded".to_owned(),
            event.event_id.as_str().to_owned(),
            event.transaction_time.as_i64().to_string(),
            encode_assertion(&event.assertion),
        ]),
        GraphEvent::AssertionRetracted(event) => encode_parts(&[
            "AssertionRetracted".to_owned(),
            event.event_id.as_str().to_owned(),
            event.transaction_time.as_i64().to_string(),
            event.assertion_id.as_str().to_owned(),
        ]),
        GraphEvent::SourceAdded(event) => encode_parts(&[
            "SourceAdded".to_owned(),
            event.event_id.as_str().to_owned(),
            event.transaction_time.as_i64().to_string(),
            encode_source(&event.source),
        ]),
        GraphEvent::EvidenceLinked(event) => encode_parts(&[
            "EvidenceLinked".to_owned(),
            event.event_id.as_str().to_owned(),
            event.transaction_time.as_i64().to_string(),
            event.assertion_id.as_str().to_owned(),
            event.source_id.as_str().to_owned(),
        ]),
        GraphEvent::EntityMerged(event) => encode_parts(&[
            "EntityMerged".to_owned(),
            event.event_id.as_str().to_owned(),
            event.transaction_time.as_i64().to_string(),
            event.from.as_str().to_owned(),
            event.into.as_str().to_owned(),
        ]),
        GraphEvent::ConfidenceUpdated(event) => encode_parts(&[
            "ConfidenceUpdated".to_owned(),
            event.event_id.as_str().to_owned(),
            event.transaction_time.as_i64().to_string(),
            event.assertion_id.as_str().to_owned(),
            encode_confidence(event.confidence),
            encode_source_ids(&event.source_ids),
        ]),
        GraphEvent::CausalLinkAdded(event) => encode_parts(&[
            "CausalLinkAdded".to_owned(),
            event.event_id.as_str().to_owned(),
            event.transaction_time.as_i64().to_string(),
            encode_causal_link(&event.causal_link),
        ]),
        GraphEvent::AgentMemoryRecorded(event) => encode_parts(&[
            "AgentMemoryRecorded".to_owned(),
            event.event_id.as_str().to_owned(),
            event.transaction_time.as_i64().to_string(),
            encode_agent_memory(&event.memory),
        ]),
    }
}

fn decode_event(record: &str) -> Result<GraphEvent, StorageError> {
    let parts = decode_parts(record)?;
    let event_kind = required(&parts, 0, "event kind")?;
    let event_id = EventId::new(required(&parts, 1, "event id")?);
    let transaction_time = TxTime::new(parse_i64(required(&parts, 2, "transaction time")?)?);
    match event_kind {
        "EntityCreated" => Ok(GraphEvent::EntityCreated(EntityCreated {
            event_id,
            transaction_time,
            entity: decode_entity(required(&parts, 3, "entity")?)?,
        })),
        "AssertionAdded" => Ok(GraphEvent::AssertionAdded(AssertionAdded {
            event_id,
            transaction_time,
            assertion: decode_assertion(required(&parts, 3, "assertion")?)?,
        })),
        "AssertionRetracted" => Ok(GraphEvent::AssertionRetracted(AssertionRetracted {
            event_id,
            transaction_time,
            assertion_id: AssertionId::new(required(&parts, 3, "assertion id")?),
        })),
        "SourceAdded" => Ok(GraphEvent::SourceAdded(SourceAdded {
            event_id,
            transaction_time,
            source: decode_source(required(&parts, 3, "source")?)?,
        })),
        "EvidenceLinked" => Ok(GraphEvent::EvidenceLinked(EvidenceLinked {
            event_id,
            transaction_time,
            assertion_id: AssertionId::new(required(&parts, 3, "assertion id")?),
            source_id: SourceId::new(required(&parts, 4, "source id")?),
        })),
        "EntityMerged" => Ok(GraphEvent::EntityMerged(EntityMerged {
            event_id,
            transaction_time,
            from: EntityId::new(required(&parts, 3, "from entity")?),
            into: EntityId::new(required(&parts, 4, "into entity")?),
        })),
        "ConfidenceUpdated" => Ok(GraphEvent::ConfidenceUpdated(ConfidenceUpdated {
            event_id,
            transaction_time,
            assertion_id: AssertionId::new(required(&parts, 3, "assertion id")?),
            confidence: decode_confidence(required(&parts, 4, "confidence")?)?,
            source_ids: decode_source_ids(required(&parts, 5, "source ids")?)?,
        })),
        "CausalLinkAdded" => Ok(GraphEvent::CausalLinkAdded(CausalLinkAdded {
            event_id,
            transaction_time,
            causal_link: decode_causal_link(required(&parts, 3, "causal link")?)?,
        })),
        "AgentMemoryRecorded" => Ok(GraphEvent::AgentMemoryRecorded(AgentMemoryRecorded {
            event_id,
            transaction_time,
            memory: decode_agent_memory(required(&parts, 3, "agent memory")?)?,
        })),
        other => Err(StorageError::Codec(format!("unknown event kind {other}"))),
    }
}

fn encode_entity(entity: &Entity) -> String {
    encode_parts(&[
        entity.id.as_str().to_owned(),
        encode_entity_type(&entity.entity_type),
        encode_option_string(entity.canonical_name.as_deref()),
        encode_property_map(&entity.properties),
        entity.created_tx.as_i64().to_string(),
    ])
}

fn decode_entity(record: &str) -> Result<Entity, StorageError> {
    let parts = decode_parts(record)?;
    Ok(Entity {
        id: EntityId::new(required(&parts, 0, "entity id")?),
        entity_type: decode_entity_type(required(&parts, 1, "entity type")?)?,
        canonical_name: decode_option_string(required(&parts, 2, "canonical name")?)?,
        properties: decode_property_map(required(&parts, 3, "properties")?)?,
        created_tx: TxTime::new(parse_i64(required(&parts, 4, "created tx")?)?),
    })
}

fn encode_assertion(assertion: &Assertion) -> String {
    encode_parts(&[
        assertion.id.as_str().to_owned(),
        assertion.subject.as_str().to_owned(),
        assertion.predicate.as_str().to_owned(),
        encode_graph_value(&assertion.object),
        encode_valid_interval(&assertion.valid_time),
        encode_tx_interval(&assertion.transaction_time),
        encode_confidence(assertion.confidence),
        encode_source_ids(&assertion.source_ids),
        encode_context_scope(&assertion.context),
        encode_assertion_status(&assertion.status),
    ])
}

fn decode_assertion(record: &str) -> Result<Assertion, StorageError> {
    let parts = decode_parts(record)?;
    Ok(Assertion {
        id: AssertionId::new(required(&parts, 0, "assertion id")?),
        subject: EntityId::new(required(&parts, 1, "subject")?),
        predicate: PredicateId::new(required(&parts, 2, "predicate")?),
        object: decode_graph_value(required(&parts, 3, "object")?)?,
        valid_time: decode_valid_interval(required(&parts, 4, "valid time")?)?,
        transaction_time: decode_tx_interval(required(&parts, 5, "tx time")?)?,
        confidence: decode_confidence(required(&parts, 6, "confidence")?)?,
        source_ids: decode_source_ids(required(&parts, 7, "source ids")?)?,
        context: decode_context_scope(required(&parts, 8, "context")?)?,
        status: decode_assertion_status(required(&parts, 9, "status")?)?,
    })
}

fn encode_source(source: &Source) -> String {
    encode_parts(&[
        source.id.as_str().to_owned(),
        encode_source_type(&source.source_type),
        encode_option_string(source.uri.as_deref()),
        source.content_hash.as_str().to_owned(),
        source.observed_at.as_i64().to_string(),
        source
            .trust_score
            .map(|score| score.to_bits().to_string())
            .unwrap_or_default(),
    ])
}

fn decode_source(record: &str) -> Result<Source, StorageError> {
    let parts = decode_parts(record)?;
    let trust_score = match required(&parts, 5, "trust score")? {
        "" => None,
        value => Some(f32::from_bits(parse_u32(value)?)),
    };
    Ok(Source {
        id: SourceId::new(required(&parts, 0, "source id")?),
        source_type: decode_source_type(required(&parts, 1, "source type")?)?,
        uri: decode_option_string(required(&parts, 2, "uri")?)?,
        content_hash: ContentHash::new(required(&parts, 3, "content hash")?),
        observed_at: TxTime::new(parse_i64(required(&parts, 4, "observed at")?)?),
        trust_score,
    })
}

fn encode_causal_link(link: &CausalLink) -> String {
    encode_parts(&[
        link.id.as_str().to_owned(),
        link.cause_event.as_str().to_owned(),
        link.effect_event.as_str().to_owned(),
        encode_confidence(link.confidence),
        encode_option_string(link.mechanism.as_deref()),
        encode_source_ids(&link.source_ids),
    ])
}

fn decode_causal_link(record: &str) -> Result<CausalLink, StorageError> {
    let parts = decode_parts(record)?;
    Ok(CausalLink {
        id: CausalLinkId::new(required(&parts, 0, "causal link id")?),
        cause_event: EventId::new(required(&parts, 1, "cause event")?),
        effect_event: EventId::new(required(&parts, 2, "effect event")?),
        confidence: decode_confidence(required(&parts, 3, "confidence")?)?,
        mechanism: decode_option_string(required(&parts, 4, "mechanism")?)?,
        source_ids: decode_source_ids(required(&parts, 5, "source ids")?)?,
    })
}

fn encode_agent_memory(memory: &AgentMemory) -> String {
    encode_parts(&[
        memory.id.as_str().to_owned(),
        memory.agent_id.as_str().to_owned(),
        encode_memory_type(&memory.memory_type),
        memory.content.clone(),
        encode_valid_interval(&memory.valid_time),
        encode_confidence(memory.confidence),
        encode_source_ids(&memory.source_ids),
        encode_entity_ids(&memory.related_entities),
        encode_memory_ids(&memory.supersedes),
        encode_memory_status(&memory.status),
    ])
}

fn decode_agent_memory(record: &str) -> Result<AgentMemory, StorageError> {
    let parts = decode_parts(record)?;
    Ok(AgentMemory {
        id: MemoryId::new(required(&parts, 0, "memory id")?),
        agent_id: AgentId::new(required(&parts, 1, "agent id")?),
        memory_type: decode_memory_type(required(&parts, 2, "memory type")?)?,
        content: required(&parts, 3, "content")?.to_owned(),
        valid_time: decode_valid_interval(required(&parts, 4, "valid time")?)?,
        confidence: decode_confidence(required(&parts, 5, "confidence")?)?,
        source_ids: decode_source_ids(required(&parts, 6, "source ids")?)?,
        related_entities: decode_entity_ids(required(&parts, 7, "related entities")?)?,
        supersedes: decode_memory_ids(required(&parts, 8, "supersedes")?)?,
        status: decode_memory_status(required(&parts, 9, "memory status")?)?,
    })
}

fn encode_property_map(properties: &PropertyMap) -> String {
    let mut parts = vec![properties.0.len().to_string()];
    for (key, value) in &properties.0 {
        parts.push(key.as_str().to_owned());
        parts.push(encode_graph_value(value));
    }
    encode_parts(&parts)
}

fn decode_property_map(record: &str) -> Result<PropertyMap, StorageError> {
    let parts = decode_parts(record)?;
    let count = parse_usize(required(&parts, 0, "property count")?)?;
    let expected_len = 1 + count * 2;
    if parts.len() != expected_len {
        return Err(StorageError::Codec(
            "invalid property map length".to_owned(),
        ));
    }
    let mut properties = BTreeMap::new();
    for index in 0..count {
        let key = PropertyKey::new(parts[1 + index * 2].as_str());
        let value = decode_graph_value(&parts[2 + index * 2])?;
        properties.insert(key, value);
    }
    Ok(PropertyMap(properties))
}

fn encode_graph_value(value: &GraphValue) -> String {
    match value {
        GraphValue::Entity(id) => encode_parts(&["Entity".to_owned(), id.as_str().to_owned()]),
        GraphValue::Text(value) => encode_parts(&["Text".to_owned(), value.clone()]),
        GraphValue::Integer(value) => encode_parts(&["Integer".to_owned(), value.to_string()]),
        GraphValue::Decimal(value) => {
            encode_parts(&["Decimal".to_owned(), value.to_bits().to_string()])
        }
        GraphValue::Boolean(value) => encode_parts(&["Boolean".to_owned(), value.to_string()]),
        GraphValue::Time(value) => encode_parts(&["Time".to_owned(), value.as_i64().to_string()]),
        GraphValue::Null => encode_parts(&["Null".to_owned()]),
    }
}

fn decode_graph_value(record: &str) -> Result<GraphValue, StorageError> {
    let parts = decode_parts(record)?;
    match required(&parts, 0, "graph value kind")? {
        "Entity" => Ok(GraphValue::Entity(EntityId::new(required(
            &parts,
            1,
            "entity value",
        )?))),
        "Text" => Ok(GraphValue::Text(
            required(&parts, 1, "text value")?.to_owned(),
        )),
        "Integer" => Ok(GraphValue::Integer(parse_i64(required(
            &parts,
            1,
            "integer value",
        )?)?)),
        "Decimal" => Ok(GraphValue::Decimal(f64::from_bits(parse_u64(required(
            &parts,
            1,
            "decimal value",
        )?)?))),
        "Boolean" => Ok(GraphValue::Boolean(parse_bool(required(
            &parts,
            1,
            "boolean value",
        )?)?)),
        "Time" => Ok(GraphValue::Time(ValidTime::new(parse_i64(required(
            &parts,
            1,
            "time value",
        )?)?))),
        "Null" => Ok(GraphValue::Null),
        other => Err(StorageError::Codec(format!("unknown graph value {other}"))),
    }
}

fn encode_valid_interval(interval: &TimeInterval<ValidTime>) -> String {
    encode_parts(&[
        interval.start.as_i64().to_string(),
        interval
            .end
            .map(|end| end.as_i64().to_string())
            .unwrap_or_default(),
    ])
}

fn decode_valid_interval(record: &str) -> Result<TimeInterval<ValidTime>, StorageError> {
    let parts = decode_parts(record)?;
    let start = ValidTime::new(parse_i64(required(&parts, 0, "valid start")?)?);
    let end = match required(&parts, 1, "valid end")? {
        "" => None,
        value => Some(ValidTime::new(parse_i64(value)?)),
    };
    TimeInterval::new(start, end).map_err(|error| StorageError::Codec(format!("{error:?}")))
}

fn encode_tx_interval(interval: &TimeInterval<TxTime>) -> String {
    encode_parts(&[
        interval.start.as_i64().to_string(),
        interval
            .end
            .map(|end| end.as_i64().to_string())
            .unwrap_or_default(),
    ])
}

fn decode_tx_interval(record: &str) -> Result<TimeInterval<TxTime>, StorageError> {
    let parts = decode_parts(record)?;
    let start = TxTime::new(parse_i64(required(&parts, 0, "tx start")?)?);
    let end = match required(&parts, 1, "tx end")? {
        "" => None,
        value => Some(TxTime::new(parse_i64(value)?)),
    };
    TimeInterval::new(start, end).map_err(|error| StorageError::Codec(format!("{error:?}")))
}

fn encode_confidence(confidence: Confidence) -> String {
    confidence.as_f32().to_bits().to_string()
}

fn decode_confidence(record: &str) -> Result<Confidence, StorageError> {
    Confidence::new(f32::from_bits(parse_u32(record)?))
        .map_err(|error| StorageError::Codec(format!("{error:?}")))
}

fn encode_source_ids(source_ids: &[SourceId]) -> String {
    encode_parts(
        &source_ids
            .iter()
            .map(|source_id| source_id.as_str().to_owned())
            .collect::<Vec<_>>(),
    )
}

fn decode_source_ids(record: &str) -> Result<Vec<SourceId>, StorageError> {
    Ok(decode_parts(record)?
        .into_iter()
        .map(SourceId::new)
        .collect())
}

fn encode_entity_ids(entity_ids: &[EntityId]) -> String {
    encode_parts(
        &entity_ids
            .iter()
            .map(|entity_id| entity_id.as_str().to_owned())
            .collect::<Vec<_>>(),
    )
}

fn decode_entity_ids(record: &str) -> Result<Vec<EntityId>, StorageError> {
    Ok(decode_parts(record)?
        .into_iter()
        .map(EntityId::new)
        .collect())
}

fn encode_memory_ids(memory_ids: &[MemoryId]) -> String {
    encode_parts(
        &memory_ids
            .iter()
            .map(|memory_id| memory_id.as_str().to_owned())
            .collect::<Vec<_>>(),
    )
}

fn decode_memory_ids(record: &str) -> Result<Vec<MemoryId>, StorageError> {
    Ok(decode_parts(record)?
        .into_iter()
        .map(MemoryId::new)
        .collect())
}

fn encode_context_scope(context: &ContextScope) -> String {
    match context {
        ContextScope::Global => encode_parts(&["Global".to_owned()]),
        ContextScope::Named(value) => encode_parts(&["Named".to_owned(), value.clone()]),
    }
}

fn decode_context_scope(record: &str) -> Result<ContextScope, StorageError> {
    let parts = decode_parts(record)?;
    match required(&parts, 0, "context kind")? {
        "Global" => Ok(ContextScope::Global),
        "Named" => Ok(ContextScope::Named(
            required(&parts, 1, "context name")?.to_owned(),
        )),
        other => Err(StorageError::Codec(format!("unknown context {other}"))),
    }
}

fn encode_assertion_status(status: &AssertionStatus) -> String {
    match status {
        AssertionStatus::Active => "Active",
        AssertionStatus::Retracted => "Retracted",
        AssertionStatus::Superseded => "Superseded",
        AssertionStatus::Disputed => "Disputed",
    }
    .to_owned()
}

fn decode_assertion_status(record: &str) -> Result<AssertionStatus, StorageError> {
    match record {
        "Active" => Ok(AssertionStatus::Active),
        "Retracted" => Ok(AssertionStatus::Retracted),
        "Superseded" => Ok(AssertionStatus::Superseded),
        "Disputed" => Ok(AssertionStatus::Disputed),
        other => Err(StorageError::Codec(format!(
            "unknown assertion status {other}"
        ))),
    }
}

fn encode_memory_type(memory_type: &MemoryType) -> String {
    match memory_type {
        MemoryType::Episodic => "Episodic",
        MemoryType::Semantic => "Semantic",
        MemoryType::Procedural => "Procedural",
        MemoryType::Observation => "Observation",
        MemoryType::Decision => "Decision",
        MemoryType::Action => "Action",
        MemoryType::ToolCall => "ToolCall",
        MemoryType::Outcome => "Outcome",
        MemoryType::Preference => "Preference",
        MemoryType::Goal => "Goal",
        MemoryType::Plan => "Plan",
        MemoryType::Reflection => "Reflection",
        MemoryType::Correction => "Correction",
        MemoryType::Relationship => "Relationship",
        MemoryType::WorldState => "WorldState",
    }
    .to_owned()
}

fn decode_memory_type(record: &str) -> Result<MemoryType, StorageError> {
    match record {
        "Episodic" => Ok(MemoryType::Episodic),
        "Semantic" => Ok(MemoryType::Semantic),
        "Procedural" => Ok(MemoryType::Procedural),
        "Observation" => Ok(MemoryType::Observation),
        "Decision" => Ok(MemoryType::Decision),
        "Action" => Ok(MemoryType::Action),
        "ToolCall" => Ok(MemoryType::ToolCall),
        "Outcome" => Ok(MemoryType::Outcome),
        "Preference" => Ok(MemoryType::Preference),
        "Goal" => Ok(MemoryType::Goal),
        "Plan" => Ok(MemoryType::Plan),
        "Reflection" => Ok(MemoryType::Reflection),
        "Correction" => Ok(MemoryType::Correction),
        "Relationship" => Ok(MemoryType::Relationship),
        "WorldState" => Ok(MemoryType::WorldState),
        other => Err(StorageError::Codec(format!("unknown memory type {other}"))),
    }
}

fn encode_memory_status(status: &MemoryStatus) -> String {
    match status {
        MemoryStatus::Candidate => "Candidate",
        MemoryStatus::Active => "Active",
        MemoryStatus::Reinforced => "Reinforced",
        MemoryStatus::Superseded => "Superseded",
        MemoryStatus::Contradicted => "Contradicted",
        MemoryStatus::Archived => "Archived",
    }
    .to_owned()
}

fn decode_memory_status(record: &str) -> Result<MemoryStatus, StorageError> {
    match record {
        "Candidate" => Ok(MemoryStatus::Candidate),
        "Active" => Ok(MemoryStatus::Active),
        "Reinforced" => Ok(MemoryStatus::Reinforced),
        "Superseded" => Ok(MemoryStatus::Superseded),
        "Contradicted" => Ok(MemoryStatus::Contradicted),
        "Archived" => Ok(MemoryStatus::Archived),
        other => Err(StorageError::Codec(format!(
            "unknown memory status {other}"
        ))),
    }
}

fn encode_entity_type(entity_type: &EntityType) -> String {
    match entity_type {
        EntityType::Person => encode_parts(&["Person".to_owned()]),
        EntityType::Organization => encode_parts(&["Organization".to_owned()]),
        EntityType::Place => encode_parts(&["Place".to_owned()]),
        EntityType::Event => encode_parts(&["Event".to_owned()]),
        EntityType::Document => encode_parts(&["Document".to_owned()]),
        EntityType::Concept => encode_parts(&["Concept".to_owned()]),
        EntityType::Custom(value) => encode_parts(&["Custom".to_owned(), value.clone()]),
    }
}

fn decode_entity_type(record: &str) -> Result<EntityType, StorageError> {
    let parts = decode_parts(record)?;
    match required(&parts, 0, "entity type")? {
        "Person" => Ok(EntityType::Person),
        "Organization" => Ok(EntityType::Organization),
        "Place" => Ok(EntityType::Place),
        "Event" => Ok(EntityType::Event),
        "Document" => Ok(EntityType::Document),
        "Concept" => Ok(EntityType::Concept),
        "Custom" => Ok(EntityType::Custom(
            required(&parts, 1, "custom entity type")?.to_owned(),
        )),
        other => Err(StorageError::Codec(format!("unknown entity type {other}"))),
    }
}

fn encode_source_type(source_type: &SourceType) -> String {
    match source_type {
        SourceType::Document => encode_parts(&["Document".to_owned()]),
        SourceType::WebPage => encode_parts(&["WebPage".to_owned()]),
        SourceType::DatabaseRecord => encode_parts(&["DatabaseRecord".to_owned()]),
        SourceType::ApiResponse => encode_parts(&["ApiResponse".to_owned()]),
        SourceType::HumanReport => encode_parts(&["HumanReport".to_owned()]),
        SourceType::SensorReading => encode_parts(&["SensorReading".to_owned()]),
        SourceType::Custom(value) => encode_parts(&["Custom".to_owned(), value.clone()]),
    }
}

fn decode_source_type(record: &str) -> Result<SourceType, StorageError> {
    let parts = decode_parts(record)?;
    match required(&parts, 0, "source type")? {
        "Document" => Ok(SourceType::Document),
        "WebPage" => Ok(SourceType::WebPage),
        "DatabaseRecord" => Ok(SourceType::DatabaseRecord),
        "ApiResponse" => Ok(SourceType::ApiResponse),
        "HumanReport" => Ok(SourceType::HumanReport),
        "SensorReading" => Ok(SourceType::SensorReading),
        "Custom" => Ok(SourceType::Custom(
            required(&parts, 1, "custom source type")?.to_owned(),
        )),
        other => Err(StorageError::Codec(format!("unknown source type {other}"))),
    }
}

fn encode_option_string(value: Option<&str>) -> String {
    match value {
        Some(value) => encode_parts(&["Some".to_owned(), value.to_owned()]),
        None => encode_parts(&["None".to_owned()]),
    }
}

fn decode_option_string(record: &str) -> Result<Option<String>, StorageError> {
    let parts = decode_parts(record)?;
    match required(&parts, 0, "option kind")? {
        "Some" => Ok(Some(required(&parts, 1, "option value")?.to_owned())),
        "None" => Ok(None),
        other => Err(StorageError::Codec(format!("unknown option kind {other}"))),
    }
}

fn encode_parts(parts: &[String]) -> String {
    let mut encoded = String::new();
    for part in parts {
        let escaped = escape_part(part);
        encoded.push_str(&escaped.len().to_string());
        encoded.push(':');
        encoded.push_str(&escaped);
    }
    encoded
}

fn decode_parts(record: &str) -> Result<Vec<String>, StorageError> {
    let mut parts = Vec::new();
    let mut index = 0;
    while index < record.len() {
        let colon = record[index..]
            .find(':')
            .ok_or_else(|| StorageError::Codec("missing length delimiter".to_owned()))?
            + index;
        let len = record[index..colon]
            .parse::<usize>()
            .map_err(|error| StorageError::Codec(error.to_string()))?;
        let start = colon + 1;
        let end = start + len;
        if end > record.len() {
            return Err(StorageError::Codec("part length exceeds record".to_owned()));
        }
        parts.push(unescape_part(&record[start..end])?);
        index = end;
    }
    Ok(parts)
}

fn escape_part(part: &str) -> String {
    let mut escaped = String::new();
    for character in part.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn unescape_part(part: &str) -> Result<String, StorageError> {
    let mut unescaped = String::new();
    let mut chars = part.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            unescaped.push(character);
            continue;
        }
        match chars.next() {
            Some('\\') => unescaped.push('\\'),
            Some('n') => unescaped.push('\n'),
            Some('r') => unescaped.push('\r'),
            Some('t') => unescaped.push('\t'),
            Some(other) => {
                return Err(StorageError::Codec(format!(
                    "invalid escape sequence \\{other}"
                )));
            }
            None => return Err(StorageError::Codec("dangling escape sequence".to_owned())),
        }
    }
    Ok(unescaped)
}

fn required<'a>(
    parts: &'a [String],
    index: usize,
    field_name: &str,
) -> Result<&'a str, StorageError> {
    parts
        .get(index)
        .map(String::as_str)
        .ok_or_else(|| StorageError::Codec(format!("missing {field_name}")))
}

fn parse_i64(value: &str) -> Result<i64, StorageError> {
    value
        .parse::<i64>()
        .map_err(|error| StorageError::Codec(error.to_string()))
}

fn parse_u64(value: &str) -> Result<u64, StorageError> {
    value
        .parse::<u64>()
        .map_err(|error| StorageError::Codec(error.to_string()))
}

fn parse_u32(value: &str) -> Result<u32, StorageError> {
    value
        .parse::<u32>()
        .map_err(|error| StorageError::Codec(error.to_string()))
}

fn parse_usize(value: &str) -> Result<usize, StorageError> {
    value
        .parse::<usize>()
        .map_err(|error| StorageError::Codec(error.to_string()))
}

fn parse_bool(value: &str) -> Result<bool, StorageError> {
    value
        .parse::<bool>()
        .map_err(|error| StorageError::Codec(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rg_events::{
        AddAssertion, AddSource, AssertionId, Confidence, ContentHash, ContextScope, CreateEntity,
        EntityId, EntityType, EventLog, GraphCommand, GraphState, GraphValue, PredicateId,
        PropertyMap, SourceId, SourceType, TimeInterval, TxTime, ValidTime,
    };
    use std::fs;
    use std::path::PathBuf;

    fn temp_file(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "reality-graph-storage-{name}-{}-{}.jsonl",
            std::process::id(),
            TxTime::new(0).as_i64()
        ));
        let _ = fs::remove_file(&path);
        path
    }

    fn sample_events() -> Vec<GraphEvent> {
        let mut log = EventLog::new(TxTime::new(0));
        log.execute(GraphCommand::AddSource(AddSource {
            id: SourceId::new("source-1"),
            source_type: SourceType::Document,
            uri: Some("file://source.md".to_owned()),
            content_hash: ContentHash::new("sha256:source"),
            trust_score: Some(0.9),
        }))
        .expect("source command valid");
        log.execute(GraphCommand::CreateEntity(CreateEntity {
            id: EntityId::new("person-a"),
            entity_type: EntityType::Person,
            canonical_name: Some("Person A".to_owned()),
            properties: PropertyMap::default(),
        }))
        .expect("subject command valid");
        log.execute(GraphCommand::CreateEntity(CreateEntity {
            id: EntityId::new("company-b"),
            entity_type: EntityType::Organization,
            canonical_name: Some("Company B".to_owned()),
            properties: PropertyMap::default(),
        }))
        .expect("object command valid");
        log.execute(GraphCommand::AddAssertion(AddAssertion {
            id: AssertionId::new("assertion-1"),
            subject: EntityId::new("person-a"),
            predicate: PredicateId::new("works_at"),
            object: GraphValue::Entity(EntityId::new("company-b")),
            valid_time: TimeInterval::new(ValidTime::new(10), Some(ValidTime::new(20)))
                .expect("valid interval"),
            confidence: Confidence::new(0.92).expect("valid confidence"),
            source_ids: vec![SourceId::new("source-1")],
            context: ContextScope::Global,
        }))
        .expect("assertion command valid");
        log.events().to_vec()
    }

    #[test]
    fn in_memory_storage_materializes_stores_and_indexes() {
        let events = sample_events();
        let storage = InMemoryStorage::replay(&events).expect("replay succeeds");

        assert!(storage.entity(&EntityId::new("person-a")).is_some());
        assert!(storage
            .assertion(&AssertionId::new("assertion-1"))
            .is_some());
        assert!(storage.source(&SourceId::new("source-1")).is_some());
        assert_eq!(
            storage
                .assertions_by_subject(&EntityId::new("person-a"))
                .len(),
            1
        );
        assert_eq!(
            storage
                .assertions_by_predicate(&PredicateId::new("works_at"))
                .len(),
            1
        );
        assert_eq!(
            storage
                .assertions_by_object(&GraphValue::Entity(EntityId::new("company-b")))
                .len(),
            1
        );
        assert_eq!(storage.adjacent_edges(&EntityId::new("company-b")).len(), 1);
        assert_eq!(storage.assertions_valid_at(ValidTime::new(15)).len(), 1);
        assert_eq!(storage.assertions_tx_at(TxTime::new(4)).len(), 1);
        assert_eq!(
            storage
                .assertions_by_source(&SourceId::new("source-1"))
                .len(),
            1
        );
    }

    #[test]
    fn file_event_log_recovers_by_replaying_append_only_records() {
        let path = temp_file("event-log");
        let events = sample_events();
        {
            let mut file_log = FileEventLog::open(&path).expect("open log");
            for event in &events {
                file_log.append(event).expect("append event");
            }
        }

        let reloaded = FileEventLog::open(&path).expect("reopen log");
        let recovered_events = reloaded.read_all().expect("read events");
        let recovered = InMemoryStorage::replay(&recovered_events).expect("replay recovered");
        let expected = InMemoryStorage::replay(&events).expect("replay expected");

        assert_eq!(recovered.graph_state(), expected.graph_state());
        assert_eq!(
            GraphState::replay(&recovered_events).expect("graph replay"),
            GraphState::replay(&events).expect("expected replay")
        );

        fs::remove_file(path).expect("cleanup");
    }

    #[test]
    fn file_event_log_rejects_corrupted_event_records() {
        let path = temp_file("event-log-corrupt");
        let events = sample_events();
        {
            let mut file_log = FileEventLog::open(&path).expect("open log");
            file_log.append(&events[0]).expect("append event");
        }

        let contents = fs::read_to_string(&path).expect("read log");
        let mut parts = decode_parts(contents.trim_end()).expect("event record parts");
        assert_eq!(parts[0], EVENT_RECORD_KIND);
        parts[2] = "0000000000000000".to_owned();
        fs::write(&path, format!("{}\n", encode_parts(&parts))).expect("corrupt log");

        let reloaded = FileEventLog::open(&path).expect("reopen log");
        assert!(matches!(
            reloaded.read_all(),
            Err(StorageError::Codec(message)) if message.contains("checksum")
        ));

        fs::remove_file(path).expect("cleanup");
    }

    #[test]
    fn snapshots_include_manifest_and_detect_corruption() {
        let path = temp_file("snapshot-manifest");
        let events = sample_events();
        let storage = InMemoryStorage::replay(&events).expect("replay succeeds");

        SnapshotWriter::write(&path, &storage).expect("write snapshot");
        let mut contents = fs::read_to_string(&path).expect("read snapshot");
        contents = contents.replacen("event_count", "event_count_corrupt", 1);
        fs::write(&path, contents).expect("corrupt snapshot");

        let result = SnapshotReader::read(&path);
        assert!(result.is_err());

        fs::remove_file(path).expect("cleanup");
    }

    #[test]
    fn snapshots_round_trip_deterministic_graph_state() {
        let path = temp_file("snapshot");
        let events = sample_events();
        let storage = InMemoryStorage::replay(&events).expect("replay succeeds");

        SnapshotWriter::write(&path, &storage).expect("write snapshot");
        let restored = SnapshotReader::read(&path).expect("read snapshot");

        assert_eq!(restored.graph_state(), storage.graph_state());
        assert_eq!(restored.events(), storage.events());

        fs::remove_file(path).expect("cleanup");
    }

    #[test]
    fn backup_restore_preserves_events_manifest_and_graph_state() {
        let path = temp_file("backup");
        let events = sample_events();
        let storage = InMemoryStorage::replay(&events).expect("replay succeeds");

        let manifest = BackupWriter::write(&path, &storage).expect("write backup");
        let restored = BackupReader::restore(&path).expect("restore backup");
        let restored_manifest = BackupReader::manifest(&path).expect("read backup manifest");

        assert_eq!(manifest, restored_manifest);
        assert_eq!(manifest.event_count, events.len());
        assert_eq!(manifest.entity_count, 2);
        assert_eq!(manifest.assertion_count, 1);
        assert_eq!(manifest.source_count, 1);
        assert_eq!(restored.events(), storage.events());
        assert_eq!(restored.graph_state(), storage.graph_state());

        fs::remove_file(path).expect("cleanup");
    }
}
