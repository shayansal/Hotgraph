//! Single-node storage primitives for Reality Graph.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use redb::{Database, ReadableTable, TableDefinition};
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
    Redb(String),
    Codec(String),
    Replay(GraphReplayError),
    SnapshotMismatch,
}

const EVENT_RECORD_KIND: &str = "RGEVENT";
const LEGACY_EVENT_RECORD_VERSION: &str = "1";
const EVENT_RECORD_VERSION: &str = "2";
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

const REDB_SCHEMA_VERSION: u32 = 1;
const REDB_METADATA: TableDefinition<'static, &str, &[u8]> =
    TableDefinition::new("hotgraph_metadata");
const REDB_EVENTS: TableDefinition<'static, u64, &[u8]> = TableDefinition::new("events_by_lsn");
const REDB_IDEMPOTENCY: TableDefinition<'static, &str, &[u8]> =
    TableDefinition::new("idempotency_by_key");
const REDB_ENTITIES: TableDefinition<'static, &str, &[u8]> = TableDefinition::new("entities_by_id");
const REDB_ASSERTIONS: TableDefinition<'static, &str, &[u8]> =
    TableDefinition::new("assertions_by_id");
const REDB_SOURCES: TableDefinition<'static, &str, &[u8]> = TableDefinition::new("sources_by_id");
const REDB_MEMORIES: TableDefinition<'static, &str, &[u8]> = TableDefinition::new("memories_by_id");
const REDB_CAUSAL_LINKS: TableDefinition<'static, &str, &[u8]> =
    TableDefinition::new("causal_links_by_id");
const REDB_INDEX_SUBJECT: TableDefinition<'static, &str, &[u8]> =
    TableDefinition::new("idx_subject_to_assertions");
const REDB_INDEX_PREDICATE: TableDefinition<'static, &str, &[u8]> =
    TableDefinition::new("idx_predicate_to_assertions");
const REDB_INDEX_OBJECT: TableDefinition<'static, &str, &[u8]> =
    TableDefinition::new("idx_object_to_assertions");
const REDB_INDEX_SOURCE: TableDefinition<'static, &str, &[u8]> =
    TableDefinition::new("idx_source_to_assertions");
const REDB_INDEX_VALID_TIME: TableDefinition<'static, &str, &[u8]> =
    TableDefinition::new("idx_valid_time_to_assertions");
const REDB_INDEX_TX_TIME: TableDefinition<'static, &str, &[u8]> =
    TableDefinition::new("idx_tx_time_to_assertions");
const REDB_INDEX_CONTEXT: TableDefinition<'static, &str, &[u8]> =
    TableDefinition::new("idx_context_to_assertions");

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableAppend {
    pub lsn: u64,
    pub event_id: EventId,
    pub transaction_time: TxTime,
    pub idempotency_replayed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableHealth {
    pub schema_version: u32,
    pub last_lsn: u64,
    pub applied_lsn: u64,
    pub replay_lag: u64,
    pub writer_lease: Option<WriterLease>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationRecord {
    pub version: u32,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WriterLease {
    pub holder_id: String,
    pub fencing_token: u64,
    pub expires_at_ms: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WriterLeaseAttempt {
    Acquired(WriterLease),
    Rejected(WriterLease),
}

pub trait DurableGraphStore {
    fn append_event(
        &mut self,
        event: &GraphEvent,
        idempotency_key: Option<&str>,
    ) -> Result<DurableAppend, StorageError>;

    fn events_by_lsn(
        &self,
        start_lsn: u64,
        end_lsn: u64,
    ) -> Result<Vec<(u64, GraphEvent)>, StorageError>;
    fn materialized_storage(&self) -> Result<InMemoryStorage, StorageError>;
    fn health(&self) -> Result<DurableHealth, StorageError>;
}

pub struct RedbGraphStore {
    database: Database,
}

impl std::fmt::Debug for RedbGraphStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RedbGraphStore")
    }
}

impl RedbGraphStore {
    pub fn create(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let database = Database::create(path).map_err(redb_error)?;
        let store = Self { database };
        store.initialize_schema()?;
        Ok(store)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let database = Database::open(path).map_err(redb_error)?;
        let store = Self { database };
        store.initialize_schema()?;
        Ok(store)
    }

    pub fn append_event(
        &mut self,
        event: &GraphEvent,
        idempotency_key: Option<&str>,
    ) -> Result<DurableAppend, StorageError> {
        if let Some(key) = idempotency_key {
            if let Some(lsn) = self.idempotency_lsn(key)? {
                let event = self.event_at_lsn(lsn)?.ok_or_else(|| {
                    StorageError::Codec(format!(
                        "idempotency key {key} points at missing event LSN {lsn}"
                    ))
                })?;
                return Ok(DurableAppend {
                    lsn,
                    event_id: event.event_id().clone(),
                    transaction_time: event.transaction_time(),
                    idempotency_replayed: true,
                });
            }
        }

        let lsn = self.last_lsn()? + 1;
        let payload = encode_event(event);
        let checksum = checksum_hex(
            wal_checksum_input(
                lsn,
                event.event_id().as_str(),
                event.transaction_time(),
                idempotency_key,
                &payload,
            )
            .as_bytes(),
        );
        let record = WalRecord {
            sequence: lsn,
            event_id: event.event_id().as_str().to_owned(),
            transaction_time: event.transaction_time(),
            idempotency_key: idempotency_key.map(str::to_owned),
            checksum,
            event: event.clone(),
        };
        let encoded_record = encode_event_record(&record);
        let lsn_string = lsn.to_string();

        let write_txn = self.database.begin_write().map_err(redb_error)?;
        {
            let mut events = write_txn.open_table(REDB_EVENTS).map_err(redb_error)?;
            events
                .insert(lsn, encoded_record.as_bytes())
                .map_err(redb_error)?;
        }
        if let Some(key) = idempotency_key {
            let mut idempotency = write_txn.open_table(REDB_IDEMPOTENCY).map_err(redb_error)?;
            idempotency
                .insert(key, lsn_string.as_bytes())
                .map_err(redb_error)?;
        }
        materialize_event_in_redb(&write_txn, event)?;
        write_metadata_in_tx(&write_txn, "last_lsn", &lsn_string)?;
        write_metadata_in_tx(&write_txn, "applied_lsn", &lsn_string)?;
        write_txn.commit().map_err(redb_error)?;

        Ok(DurableAppend {
            lsn,
            event_id: event.event_id().clone(),
            transaction_time: event.transaction_time(),
            idempotency_replayed: false,
        })
    }

    pub fn events_by_lsn(
        &self,
        start_lsn: u64,
        end_lsn: u64,
    ) -> Result<Vec<(u64, GraphEvent)>, StorageError> {
        if start_lsn > end_lsn {
            return Ok(Vec::new());
        }

        let read_txn = self.database.begin_read().map_err(redb_error)?;
        let events = read_txn.open_table(REDB_EVENTS).map_err(redb_error)?;
        let mut output = Vec::new();
        for record in events.range(start_lsn..=end_lsn).map_err(redb_error)? {
            let (lsn, value) = record.map_err(redb_error)?;
            let lsn = lsn.value();
            let record = bytes_to_string(value.value())?;
            let decoded = decode_event_record(&record, lsn)?;
            output.push((lsn, decoded.event));
        }
        Ok(output)
    }

    pub fn materialized_storage(&self) -> Result<InMemoryStorage, StorageError> {
        let last_lsn = self.last_lsn()?;
        let events = self
            .events_by_lsn(1, last_lsn)?
            .into_iter()
            .map(|(_, event)| event)
            .collect::<Vec<_>>();
        InMemoryStorage::replay(&events)
    }

    pub fn entity(&self, id: &EntityId) -> Result<Option<Entity>, StorageError> {
        self.read_string_table(REDB_ENTITIES, id.as_str(), decode_entity)
    }

    pub fn assertion(&self, id: &AssertionId) -> Result<Option<Assertion>, StorageError> {
        self.read_string_table(REDB_ASSERTIONS, id.as_str(), decode_assertion)
    }

    pub fn source(&self, id: &SourceId) -> Result<Option<Source>, StorageError> {
        self.read_string_table(REDB_SOURCES, id.as_str(), decode_source)
    }

    pub fn idempotency_lsn(&self, key: &str) -> Result<Option<u64>, StorageError> {
        self.read_string_table(REDB_IDEMPOTENCY, key, parse_u64)
    }

    pub fn schema_version(&self) -> Result<u32, StorageError> {
        self.read_metadata_u32("schema_version")
    }

    pub fn record_schema_migration(
        &mut self,
        version: u32,
        name: &str,
    ) -> Result<(), StorageError> {
        let mut history = self.migration_history()?;
        history.push(MigrationRecord {
            version,
            name: name.to_owned(),
        });
        let history_record = encode_migration_history(&history);
        let version_record = version.to_string();
        let write_txn = self.database.begin_write().map_err(redb_error)?;
        write_metadata_in_tx(&write_txn, "schema_version", &version_record)?;
        write_metadata_in_tx(&write_txn, "migration_history", &history_record)?;
        write_txn.commit().map_err(redb_error)?;
        Ok(())
    }

    pub fn migration_history(&self) -> Result<Vec<MigrationRecord>, StorageError> {
        self.read_metadata_string("migration_history")?.map_or_else(
            || Ok(Vec::new()),
            |record| decode_migration_history(&record),
        )
    }

    pub fn acquire_writer_lease(
        &mut self,
        holder_id: &str,
        now_ms: i64,
        ttl_ms: i64,
    ) -> Result<WriterLease, StorageError> {
        match self.try_acquire_writer_lease(holder_id, now_ms, ttl_ms)? {
            WriterLeaseAttempt::Acquired(lease) => Ok(lease),
            WriterLeaseAttempt::Rejected(existing) => Err(StorageError::Codec(format!(
                "writer lease held by {} until {} with fencing token {}",
                existing.holder_id, existing.expires_at_ms, existing.fencing_token
            ))),
        }
    }

    pub fn try_acquire_writer_lease(
        &mut self,
        holder_id: &str,
        now_ms: i64,
        ttl_ms: i64,
    ) -> Result<WriterLeaseAttempt, StorageError> {
        if let Some(existing) = self.writer_lease()? {
            if existing.expires_at_ms > now_ms && existing.holder_id != holder_id {
                return Ok(WriterLeaseAttempt::Rejected(existing));
            }
        }

        let next_token = self
            .writer_lease()?
            .map(|lease| lease.fencing_token.saturating_add(1))
            .unwrap_or(1);
        let lease = WriterLease {
            holder_id: holder_id.to_owned(),
            fencing_token: next_token,
            expires_at_ms: now_ms.saturating_add(ttl_ms),
        };
        let encoded = encode_writer_lease(&lease);
        let write_txn = self.database.begin_write().map_err(redb_error)?;
        write_metadata_in_tx(&write_txn, "writer_lease", &encoded)?;
        write_txn.commit().map_err(redb_error)?;
        Ok(WriterLeaseAttempt::Acquired(lease))
    }

    pub fn writer_lease(&self) -> Result<Option<WriterLease>, StorageError> {
        self.read_metadata_string("writer_lease")?
            .map_or_else(|| Ok(None), |record| decode_writer_lease(&record).map(Some))
    }

    pub fn health(&self) -> Result<DurableHealth, StorageError> {
        let last_lsn = self.last_lsn()?;
        let applied_lsn = self.read_metadata_u64("applied_lsn")?;
        Ok(DurableHealth {
            schema_version: self.schema_version()?,
            last_lsn,
            applied_lsn,
            replay_lag: last_lsn.saturating_sub(applied_lsn),
            writer_lease: self.writer_lease()?,
        })
    }

    fn initialize_schema(&self) -> Result<(), StorageError> {
        let write_txn = self.database.begin_write().map_err(redb_error)?;
        {
            let mut metadata = write_txn.open_table(REDB_METADATA).map_err(redb_error)?;
            if metadata
                .get("schema_version")
                .map_err(redb_error)?
                .is_none()
            {
                metadata
                    .insert("schema_version", REDB_SCHEMA_VERSION.to_string().as_bytes())
                    .map_err(redb_error)?;
            }
            if metadata.get("last_lsn").map_err(redb_error)?.is_none() {
                metadata
                    .insert("last_lsn", b"0".as_slice())
                    .map_err(redb_error)?;
            }
            if metadata.get("applied_lsn").map_err(redb_error)?.is_none() {
                metadata
                    .insert("applied_lsn", b"0".as_slice())
                    .map_err(redb_error)?;
            }
        }
        {
            write_txn.open_table(REDB_EVENTS).map_err(redb_error)?;
            write_txn.open_table(REDB_IDEMPOTENCY).map_err(redb_error)?;
            write_txn.open_table(REDB_ENTITIES).map_err(redb_error)?;
            write_txn.open_table(REDB_ASSERTIONS).map_err(redb_error)?;
            write_txn.open_table(REDB_SOURCES).map_err(redb_error)?;
            write_txn.open_table(REDB_MEMORIES).map_err(redb_error)?;
            write_txn
                .open_table(REDB_CAUSAL_LINKS)
                .map_err(redb_error)?;
            write_txn
                .open_table(REDB_INDEX_SUBJECT)
                .map_err(redb_error)?;
            write_txn
                .open_table(REDB_INDEX_PREDICATE)
                .map_err(redb_error)?;
            write_txn
                .open_table(REDB_INDEX_OBJECT)
                .map_err(redb_error)?;
            write_txn
                .open_table(REDB_INDEX_SOURCE)
                .map_err(redb_error)?;
            write_txn
                .open_table(REDB_INDEX_VALID_TIME)
                .map_err(redb_error)?;
            write_txn
                .open_table(REDB_INDEX_TX_TIME)
                .map_err(redb_error)?;
            write_txn
                .open_table(REDB_INDEX_CONTEXT)
                .map_err(redb_error)?;
        }
        write_txn.commit().map_err(redb_error)
    }

    fn last_lsn(&self) -> Result<u64, StorageError> {
        self.read_metadata_u64("last_lsn")
    }

    fn event_at_lsn(&self, lsn: u64) -> Result<Option<GraphEvent>, StorageError> {
        let read_txn = self.database.begin_read().map_err(redb_error)?;
        let events = read_txn.open_table(REDB_EVENTS).map_err(redb_error)?;
        let Some(value) = events.get(lsn).map_err(redb_error)? else {
            return Ok(None);
        };
        let record = bytes_to_string(value.value())?;
        let decoded = decode_event_record(&record, lsn)?;
        Ok(Some(decoded.event))
    }

    fn read_metadata_string(&self, key: &str) -> Result<Option<String>, StorageError> {
        self.read_string_table(REDB_METADATA, key, |record| Ok(record.to_owned()))
    }

    fn read_metadata_u64(&self, key: &str) -> Result<u64, StorageError> {
        self.read_metadata_string(key)?
            .ok_or_else(|| StorageError::Codec(format!("missing metadata key {key}")))?
            .parse::<u64>()
            .map_err(|error| StorageError::Codec(error.to_string()))
    }

    fn read_metadata_u32(&self, key: &str) -> Result<u32, StorageError> {
        self.read_metadata_string(key)?
            .ok_or_else(|| StorageError::Codec(format!("missing metadata key {key}")))?
            .parse::<u32>()
            .map_err(|error| StorageError::Codec(error.to_string()))
    }

    fn read_string_table<T>(
        &self,
        table: TableDefinition<'static, &str, &[u8]>,
        key: &str,
        decode: impl FnOnce(&str) -> Result<T, StorageError>,
    ) -> Result<Option<T>, StorageError> {
        let read_txn = self.database.begin_read().map_err(redb_error)?;
        let table = read_txn.open_table(table).map_err(redb_error)?;
        let Some(value) = table.get(key).map_err(redb_error)? else {
            return Ok(None);
        };
        let record = bytes_to_string(value.value())?;
        decode(&record).map(Some)
    }
}

impl DurableGraphStore for RedbGraphStore {
    fn append_event(
        &mut self,
        event: &GraphEvent,
        idempotency_key: Option<&str>,
    ) -> Result<DurableAppend, StorageError> {
        Self::append_event(self, event, idempotency_key)
    }

    fn events_by_lsn(
        &self,
        start_lsn: u64,
        end_lsn: u64,
    ) -> Result<Vec<(u64, GraphEvent)>, StorageError> {
        Self::events_by_lsn(self, start_lsn, end_lsn)
    }

    fn materialized_storage(&self) -> Result<InMemoryStorage, StorageError> {
        Self::materialized_storage(self)
    }

    fn health(&self) -> Result<DurableHealth, StorageError> {
        Self::health(self)
    }
}

fn materialize_event_in_redb(
    write_txn: &redb::WriteTransaction,
    event: &GraphEvent,
) -> Result<(), StorageError> {
    match event {
        GraphEvent::EntityCreated(event) => {
            let mut entities = write_txn.open_table(REDB_ENTITIES).map_err(redb_error)?;
            let encoded = encode_entity(&event.entity);
            entities
                .insert(event.entity.id.as_str(), encoded.as_bytes())
                .map_err(redb_error)?;
        }
        GraphEvent::AssertionAdded(event) => {
            let mut assertions = write_txn.open_table(REDB_ASSERTIONS).map_err(redb_error)?;
            let encoded = encode_assertion(&event.assertion);
            assertions
                .insert(event.assertion.id.as_str(), encoded.as_bytes())
                .map_err(redb_error)?;
            index_assertion_in_redb(write_txn, &event.assertion)?;
        }
        GraphEvent::AssertionRetracted(event) => {
            let mut assertions = write_txn.open_table(REDB_ASSERTIONS).map_err(redb_error)?;
            let existing = read_write_table_string(&assertions, event.assertion_id.as_str())?;
            let mut assertion = existing
                .as_deref()
                .map(decode_assertion)
                .transpose()?
                .ok_or_else(|| {
                    StorageError::Codec(format!(
                        "cannot retract unknown assertion {}",
                        event.assertion_id.as_str()
                    ))
                })?;
            assertion.status = AssertionStatus::Retracted;
            assertion.transaction_time.end = Some(event.transaction_time);
            let encoded = encode_assertion(&assertion);
            assertions
                .insert(event.assertion_id.as_str(), encoded.as_bytes())
                .map_err(redb_error)?;
        }
        GraphEvent::SourceAdded(event) => {
            let mut sources = write_txn.open_table(REDB_SOURCES).map_err(redb_error)?;
            let encoded = encode_source(&event.source);
            sources
                .insert(event.source.id.as_str(), encoded.as_bytes())
                .map_err(redb_error)?;
        }
        GraphEvent::EvidenceLinked(event) => {
            let mut assertions = write_txn.open_table(REDB_ASSERTIONS).map_err(redb_error)?;
            let existing = read_write_table_string(&assertions, event.assertion_id.as_str())?;
            let mut assertion = existing
                .as_deref()
                .map(decode_assertion)
                .transpose()?
                .ok_or_else(|| {
                    StorageError::Codec(format!(
                        "cannot link evidence to unknown assertion {}",
                        event.assertion_id.as_str()
                    ))
                })?;
            if !assertion.source_ids.contains(&event.source_id) {
                assertion.source_ids.push(event.source_id.clone());
                assertion.source_ids.sort();
                assertion.source_ids.dedup();
            }
            let encoded = encode_assertion(&assertion);
            assertions
                .insert(event.assertion_id.as_str(), encoded.as_bytes())
                .map_err(redb_error)?;
            append_durable_index_entry(
                write_txn,
                REDB_INDEX_SOURCE,
                event.source_id.as_str(),
                event.assertion_id.as_str(),
            )?;
        }
        GraphEvent::EntityMerged(_) => {}
        GraphEvent::ConfidenceUpdated(event) => {
            let mut assertions = write_txn.open_table(REDB_ASSERTIONS).map_err(redb_error)?;
            let existing = read_write_table_string(&assertions, event.assertion_id.as_str())?;
            let mut assertion = existing
                .as_deref()
                .map(decode_assertion)
                .transpose()?
                .ok_or_else(|| {
                    StorageError::Codec(format!(
                        "cannot update confidence for unknown assertion {}",
                        event.assertion_id.as_str()
                    ))
                })?;
            assertion.confidence = event.confidence;
            for source_id in &event.source_ids {
                if !assertion.source_ids.contains(source_id) {
                    assertion.source_ids.push(source_id.clone());
                }
                append_durable_index_entry(
                    write_txn,
                    REDB_INDEX_SOURCE,
                    source_id.as_str(),
                    event.assertion_id.as_str(),
                )?;
            }
            assertion.source_ids.sort();
            assertion.source_ids.dedup();
            let encoded = encode_assertion(&assertion);
            assertions
                .insert(event.assertion_id.as_str(), encoded.as_bytes())
                .map_err(redb_error)?;
        }
        GraphEvent::CausalLinkAdded(event) => {
            let mut causal_links = write_txn
                .open_table(REDB_CAUSAL_LINKS)
                .map_err(redb_error)?;
            let encoded = encode_causal_link(&event.causal_link);
            causal_links
                .insert(event.causal_link.id.as_str(), encoded.as_bytes())
                .map_err(redb_error)?;
        }
        GraphEvent::AgentMemoryRecorded(event) => {
            let mut memories = write_txn.open_table(REDB_MEMORIES).map_err(redb_error)?;
            for superseded_id in &event.memory.supersedes {
                let existing = read_write_table_string(&memories, superseded_id.as_str())?;
                if let Some(existing) = existing {
                    let mut memory = decode_agent_memory(&existing)?;
                    memory.status = MemoryStatus::Superseded;
                    let encoded = encode_agent_memory(&memory);
                    memories
                        .insert(superseded_id.as_str(), encoded.as_bytes())
                        .map_err(redb_error)?;
                }
            }
            let encoded = encode_agent_memory(&event.memory);
            memories
                .insert(event.memory.id.as_str(), encoded.as_bytes())
                .map_err(redb_error)?;
        }
    }
    Ok(())
}

fn index_assertion_in_redb(
    write_txn: &redb::WriteTransaction,
    assertion: &Assertion,
) -> Result<(), StorageError> {
    append_durable_index_entry(
        write_txn,
        REDB_INDEX_SUBJECT,
        assertion.subject.as_str(),
        assertion.id.as_str(),
    )?;
    append_durable_index_entry(
        write_txn,
        REDB_INDEX_PREDICATE,
        assertion.predicate.as_str(),
        assertion.id.as_str(),
    )?;
    append_durable_index_entry(
        write_txn,
        REDB_INDEX_OBJECT,
        &encode_graph_value(&assertion.object),
        assertion.id.as_str(),
    )?;
    append_durable_index_entry(
        write_txn,
        REDB_INDEX_VALID_TIME,
        &assertion.valid_time.start.as_i64().to_string(),
        assertion.id.as_str(),
    )?;
    append_durable_index_entry(
        write_txn,
        REDB_INDEX_TX_TIME,
        &assertion.transaction_time.start.as_i64().to_string(),
        assertion.id.as_str(),
    )?;
    append_durable_index_entry(
        write_txn,
        REDB_INDEX_CONTEXT,
        &encode_context_scope(&assertion.context),
        assertion.id.as_str(),
    )?;
    for source_id in &assertion.source_ids {
        append_durable_index_entry(
            write_txn,
            REDB_INDEX_SOURCE,
            source_id.as_str(),
            assertion.id.as_str(),
        )?;
    }
    Ok(())
}

fn append_durable_index_entry(
    write_txn: &redb::WriteTransaction,
    table: TableDefinition<'static, &str, &[u8]>,
    key: &str,
    value: &str,
) -> Result<(), StorageError> {
    let mut table = write_txn.open_table(table).map_err(redb_error)?;
    let mut values = read_write_table_string(&table, key)?
        .map(|record| decode_parts(&record))
        .transpose()?
        .unwrap_or_default();
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_owned());
        values.sort();
    }
    let encoded = encode_parts(&values);
    table.insert(key, encoded.as_bytes()).map_err(redb_error)?;
    Ok(())
}

fn read_write_table_string(
    table: &redb::Table<'_, &str, &[u8]>,
    key: &str,
) -> Result<Option<String>, StorageError> {
    table
        .get(key)
        .map_err(redb_error)?
        .map(|value| bytes_to_string(value.value()))
        .transpose()
}

fn write_metadata_in_tx(
    write_txn: &redb::WriteTransaction,
    key: &str,
    value: &str,
) -> Result<(), StorageError> {
    let mut metadata = write_txn.open_table(REDB_METADATA).map_err(redb_error)?;
    metadata.insert(key, value.as_bytes()).map_err(redb_error)?;
    Ok(())
}

fn encode_migration_history(records: &[MigrationRecord]) -> String {
    records
        .iter()
        .map(|record| encode_parts(&[record.version.to_string(), record.name.clone()]))
        .collect::<Vec<_>>()
        .join("\n")
}

fn decode_migration_history(record: &str) -> Result<Vec<MigrationRecord>, StorageError> {
    if record.is_empty() {
        return Ok(Vec::new());
    }
    record
        .lines()
        .map(|line| {
            let parts = decode_parts(line)?;
            Ok(MigrationRecord {
                version: parse_u32(required(&parts, 0, "migration version")?)?,
                name: required(&parts, 1, "migration name")?.to_owned(),
            })
        })
        .collect()
}

fn encode_writer_lease(lease: &WriterLease) -> String {
    encode_parts(&[
        lease.holder_id.clone(),
        lease.fencing_token.to_string(),
        lease.expires_at_ms.to_string(),
    ])
}

fn decode_writer_lease(record: &str) -> Result<WriterLease, StorageError> {
    let parts = decode_parts(record)?;
    Ok(WriterLease {
        holder_id: required(&parts, 0, "writer lease holder")?.to_owned(),
        fencing_token: parse_u64(required(&parts, 1, "writer lease fencing token")?)?,
        expires_at_ms: parse_i64(required(&parts, 2, "writer lease expiry")?)?,
    })
}

fn bytes_to_string(bytes: &[u8]) -> Result<String, StorageError> {
    String::from_utf8(bytes.to_vec()).map_err(|error| StorageError::Codec(error.to_string()))
}

fn redb_error(error: impl std::fmt::Display) -> StorageError {
    StorageError::Redb(error.to_string())
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
    options: WalOptions,
}

impl FileEventLog {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        Self::open_with_options(path, WalOptions::new(FsyncPolicy::EveryWrite))
    }

    pub fn open_with_options(
        path: impl AsRef<Path>,
        options: WalOptions,
    ) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| StorageError::Io(error.to_string()))?;
        Ok(Self { path, options })
    }

    pub fn append(&mut self, event: &GraphEvent) -> Result<(), StorageError> {
        self.append_with_metadata(event, WalAppendMetadata::new())
    }

    pub fn append_with_metadata(
        &mut self,
        event: &GraphEvent,
        metadata: WalAppendMetadata,
    ) -> Result<(), StorageError> {
        let sequence = self.next_sequence()?;
        let record = WalRecord::new(sequence, event.clone(), metadata.idempotency_key);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| StorageError::Io(error.to_string()))?;
        file.write_all(encode_event_record(&record).as_bytes())
            .map_err(|error| StorageError::Io(error.to_string()))?;
        file.write_all(b"\n")
            .map_err(|error| StorageError::Io(error.to_string()))?;
        if self.options.fsync_policy.should_sync(sequence) {
            file.sync_data()
                .map_err(|error| StorageError::Io(error.to_string()))?;
        }
        Ok(())
    }

    pub fn read_all(&self) -> Result<Vec<GraphEvent>, StorageError> {
        self.read_records()
            .map(|records| records.into_iter().map(|record| record.event).collect())
    }

    pub fn read_records(&self) -> Result<Vec<WalRecord>, StorageError> {
        read_wal_records_from_path(&self.path, 1)
    }

    pub fn recover_truncate_to_last_good(&mut self) -> Result<WalRecoveryReport, StorageError> {
        recover_wal_path_truncate_to_last_good(&self.path, 1)
    }

    fn next_sequence(&self) -> Result<u64, StorageError> {
        Ok(self.read_records()?.len() as u64 + 1)
    }
}

fn read_wal_records_from_path(
    path: impl AsRef<Path>,
    first_expected_sequence: u64,
) -> Result<Vec<WalRecord>, StorageError> {
    let file = File::open(path).map_err(|error| StorageError::Io(error.to_string()))?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|error| StorageError::Io(error.to_string()))?;
        if line.trim().is_empty() {
            continue;
        }
        let expected_sequence = first_expected_sequence + records.len() as u64;
        let record = decode_event_record(&line, expected_sequence)?;
        records.push(record);
    }
    Ok(records)
}

fn recover_wal_path_truncate_to_last_good(
    path: impl AsRef<Path>,
    first_expected_sequence: u64,
) -> Result<WalRecoveryReport, StorageError> {
    let path = path.as_ref();
    let bytes = fs::read(path).map_err(|error| StorageError::Io(error.to_string()))?;
    let mut start = 0_usize;
    let mut last_good_end = 0_usize;
    let mut records_recovered = 0_u64;
    let mut corruption_reason = None;

    while start < bytes.len() {
        let Some(relative_end) = bytes[start..].iter().position(|byte| *byte == b'\n') else {
            corruption_reason = Some("partial WAL record tail".to_owned());
            break;
        };
        let end = start + relative_end + 1;
        let line = String::from_utf8(bytes[start..end - 1].to_vec())
            .map_err(|error| StorageError::Codec(error.to_string()))?;
        if line.trim().is_empty() {
            last_good_end = end;
            start = end;
            continue;
        }
        match decode_event_record(&line, first_expected_sequence + records_recovered) {
            Ok(_) => {
                records_recovered += 1;
                last_good_end = end;
                start = end;
            }
            Err(error) => {
                corruption_reason = Some(format!("{error:?}"));
                break;
            }
        }
    }

    let bytes_quarantined = bytes.len().saturating_sub(last_good_end) as u64;
    if bytes_quarantined > 0 {
        let file = OpenOptions::new()
            .write(true)
            .open(path)
            .map_err(|error| StorageError::Io(error.to_string()))?;
        file.set_len(last_good_end as u64)
            .map_err(|error| StorageError::Io(error.to_string()))?;
        file.sync_data()
            .map_err(|error| StorageError::Io(error.to_string()))?;
    }

    Ok(WalRecoveryReport {
        records_recovered,
        last_good_sequence: (records_recovered > 0)
            .then_some(first_expected_sequence + records_recovered - 1),
        bytes_quarantined,
        corruption_reason,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SegmentedWalOptions {
    pub max_segment_events: usize,
    pub fsync_policy: FsyncPolicy,
    pub archive_dir: Option<PathBuf>,
    pub quarantine_dir: Option<PathBuf>,
}

impl SegmentedWalOptions {
    pub fn new(max_segment_events: usize) -> Self {
        Self {
            max_segment_events: max_segment_events.max(1),
            fsync_policy: FsyncPolicy::EveryWrite,
            archive_dir: None,
            quarantine_dir: None,
        }
    }

    pub fn with_fsync_policy(mut self, policy: FsyncPolicy) -> Self {
        self.fsync_policy = policy;
        self
    }

    pub fn with_archive_dir(mut self, path: impl AsRef<Path>) -> Self {
        self.archive_dir = Some(path.as_ref().to_path_buf());
        self
    }

    pub fn with_quarantine_dir(mut self, path: impl AsRef<Path>) -> Self {
        self.quarantine_dir = Some(path.as_ref().to_path_buf());
        self
    }
}

#[derive(Clone, Debug)]
pub struct SegmentedWal {
    dir: PathBuf,
    options: SegmentedWalOptions,
}

impl SegmentedWal {
    pub fn open(dir: impl AsRef<Path>, options: SegmentedWalOptions) -> Result<Self, StorageError> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir).map_err(|error| StorageError::Io(error.to_string()))?;
        if let Some(archive_dir) = &options.archive_dir {
            fs::create_dir_all(archive_dir).map_err(|error| StorageError::Io(error.to_string()))?;
        }
        if let Some(quarantine_dir) = &options.quarantine_dir {
            fs::create_dir_all(quarantine_dir)
                .map_err(|error| StorageError::Io(error.to_string()))?;
        }
        Ok(Self { dir, options })
    }

    pub fn append(&mut self, event: &GraphEvent) -> Result<(), StorageError> {
        self.append_with_metadata(event, WalAppendMetadata::new())
    }

    pub fn append_with_metadata(
        &mut self,
        event: &GraphEvent,
        metadata: WalAppendMetadata,
    ) -> Result<(), StorageError> {
        let sequence = self.next_sequence()?;
        let segment_id = self.active_segment_id()?;
        let path = self.segment_path(segment_id);
        let record = WalRecord::new(sequence, event.clone(), metadata.idempotency_key);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| StorageError::Io(error.to_string()))?;
        file.write_all(encode_event_record(&record).as_bytes())
            .map_err(|error| StorageError::Io(error.to_string()))?;
        file.write_all(b"\n")
            .map_err(|error| StorageError::Io(error.to_string()))?;
        if self.options.fsync_policy.should_sync(sequence) {
            file.sync_data()
                .map_err(|error| StorageError::Io(error.to_string()))?;
        }
        self.rewrite_segment_manifest(segment_id)?;
        Ok(())
    }

    pub fn read_all(&self) -> Result<Vec<GraphEvent>, StorageError> {
        self.read_records()
            .map(|records| records.into_iter().map(|record| record.event).collect())
    }

    pub fn read_records(&self) -> Result<Vec<WalRecord>, StorageError> {
        let mut out = Vec::new();
        let mut expected_next = None;
        for segment_id in self.segment_ids()? {
            let manifest = self.read_segment_manifest(segment_id)?;
            if let Some(expected) = expected_next {
                if manifest.first_sequence != expected {
                    return Err(StorageError::Codec(format!(
                        "WAL segment sequence gap: expected first sequence {expected}, got {}",
                        manifest.first_sequence
                    )));
                }
            }
            let records = self.read_segment_records_with_manifest(&manifest)?;
            expected_next = Some(manifest.last_sequence + 1);
            out.extend(records);
        }
        Ok(out)
    }

    pub fn read_tail_after(&self, sequence: u64) -> Result<Vec<WalRecord>, StorageError> {
        Ok(self
            .read_records()?
            .into_iter()
            .filter(|record| record.sequence > sequence)
            .collect())
    }

    pub fn manifests(&self) -> Result<Vec<SegmentManifest>, StorageError> {
        let mut manifests = self
            .segment_ids()?
            .into_iter()
            .map(|segment_id| self.read_segment_manifest(segment_id))
            .collect::<Result<Vec<_>, _>>()?;
        manifests.sort_by_key(|manifest| manifest.segment_id);
        Ok(manifests)
    }

    pub fn archive_compacted_segments(&self, up_to_sequence: u64) -> Result<usize, StorageError> {
        let Some(archive_dir) = &self.options.archive_dir else {
            return Ok(0);
        };
        fs::create_dir_all(archive_dir).map_err(|error| StorageError::Io(error.to_string()))?;
        let mut archived = 0_usize;
        for manifest in self.manifests()? {
            if manifest.last_sequence > up_to_sequence {
                continue;
            }
            let wal_path = self.segment_path(manifest.segment_id);
            let manifest_path = self.segment_manifest_path(manifest.segment_id);
            fs::rename(
                &wal_path,
                archive_dir.join(segment_wal_file_name(manifest.segment_id)),
            )
            .map_err(|error| StorageError::Io(error.to_string()))?;
            fs::rename(
                &manifest_path,
                archive_dir.join(segment_manifest_file_name(manifest.segment_id)),
            )
            .map_err(|error| StorageError::Io(error.to_string()))?;
            archived += 1;
        }
        Ok(archived)
    }

    pub fn recover_quarantine_corrupt_segments(
        &self,
    ) -> Result<SegmentedWalRecoveryReport, StorageError> {
        let Some(quarantine_dir) = &self.options.quarantine_dir else {
            return Ok(SegmentedWalRecoveryReport {
                records_recovered: self.read_records()?.len() as u64,
                last_good_sequence: self.last_sequence()?,
                segments_quarantined: 0,
                bytes_quarantined: 0,
                corruption_reason: None,
            });
        };
        fs::create_dir_all(quarantine_dir).map_err(|error| StorageError::Io(error.to_string()))?;
        let mut segments_quarantined = 0_usize;
        let mut bytes_quarantined = 0_u64;
        let mut corruption_reason = None;
        for segment_id in self.segment_ids()? {
            let manifest = match self.read_segment_manifest(segment_id) {
                Ok(manifest) => manifest,
                Err(error) => {
                    corruption_reason = Some(format!("{error:?}"));
                    let (segments, bytes) = self.quarantine_segment(segment_id, quarantine_dir)?;
                    segments_quarantined += segments;
                    bytes_quarantined += bytes;
                    continue;
                }
            };
            if let Err(error) = self.read_segment_records_with_manifest(&manifest) {
                corruption_reason = Some(format!("{error:?}"));
                let (segments, bytes) = self.quarantine_segment(segment_id, quarantine_dir)?;
                segments_quarantined += segments;
                bytes_quarantined += bytes;
            }
        }
        let records = self.read_records().unwrap_or_default();
        Ok(SegmentedWalRecoveryReport {
            records_recovered: records.len() as u64,
            last_good_sequence: records.last().map(|record| record.sequence),
            segments_quarantined,
            bytes_quarantined,
            corruption_reason,
        })
    }

    pub fn restore_snapshot_and_tail(
        snapshot_path: impl AsRef<Path>,
        wal: &SegmentedWal,
    ) -> Result<InMemoryStorage, StorageError> {
        let manifest = SnapshotReader::manifest(&snapshot_path)?;
        let mut storage = SnapshotReader::read(snapshot_path)?;
        let boundary = manifest
            .wal_lsn_boundary
            .unwrap_or(manifest.event_count as u64);
        for record in wal.read_tail_after(boundary)? {
            storage.append_event(record.event)?;
        }
        Ok(storage)
    }

    fn active_segment_id(&self) -> Result<u64, StorageError> {
        let manifests = self.manifests()?;
        let Some(last) = manifests.last() else {
            return Ok(1);
        };
        if last.event_count >= self.options.max_segment_events {
            Ok(last.segment_id + 1)
        } else {
            Ok(last.segment_id)
        }
    }

    fn next_sequence(&self) -> Result<u64, StorageError> {
        Ok(self.last_sequence()?.unwrap_or(0) + 1)
    }

    fn last_sequence(&self) -> Result<Option<u64>, StorageError> {
        Ok(self
            .manifests()?
            .last()
            .map(|manifest| manifest.last_sequence))
    }

    fn segment_path(&self, segment_id: u64) -> PathBuf {
        self.dir.join(segment_wal_file_name(segment_id))
    }

    fn segment_manifest_path(&self, segment_id: u64) -> PathBuf {
        self.dir.join(segment_manifest_file_name(segment_id))
    }

    fn segment_ids(&self) -> Result<Vec<u64>, StorageError> {
        let mut ids = Vec::new();
        for entry in fs::read_dir(&self.dir).map_err(|error| StorageError::Io(error.to_string()))? {
            let entry = entry.map_err(|error| StorageError::Io(error.to_string()))?;
            if let Some(segment_id) = parse_segment_wal_file_name(&entry.path()) {
                ids.push(segment_id);
            }
        }
        ids.sort_unstable();
        Ok(ids)
    }

    fn read_segment_manifest(&self, segment_id: u64) -> Result<SegmentManifest, StorageError> {
        let encoded = fs::read_to_string(self.segment_manifest_path(segment_id))
            .map_err(|error| StorageError::Io(error.to_string()))?;
        decode_segment_manifest(encoded.trim_end())
    }

    fn read_segment_records_with_manifest(
        &self,
        manifest: &SegmentManifest,
    ) -> Result<Vec<WalRecord>, StorageError> {
        let records = read_wal_records_from_path(
            self.segment_path(manifest.segment_id),
            manifest.first_sequence,
        )?;
        let actual = SegmentManifest::from_records(manifest.segment_id, &records)?;
        if &actual != manifest {
            return Err(StorageError::Codec(format!(
                "segment manifest mismatch for segment {}",
                manifest.segment_id
            )));
        }
        Ok(records)
    }

    fn rewrite_segment_manifest(&self, segment_id: u64) -> Result<(), StorageError> {
        let first_sequence = self
            .read_segment_manifest(segment_id)
            .ok()
            .map(|manifest| manifest.first_sequence)
            .or_else(|| {
                self.previous_segment_manifest(segment_id)
                    .map(|manifest| manifest.last_sequence + 1)
            })
            .unwrap_or(1);
        let records = read_wal_records_from_path(self.segment_path(segment_id), first_sequence)?;
        let manifest = SegmentManifest::from_records(segment_id, &records)?;
        let path = self.segment_manifest_path(segment_id);
        let tmp_path = path.with_extension("manifest.tmp");
        fs::write(
            &tmp_path,
            format!("{}\n", encode_segment_manifest(&manifest)),
        )
        .map_err(|error| StorageError::Io(error.to_string()))?;
        fs::rename(&tmp_path, &path).map_err(|error| StorageError::Io(error.to_string()))?;
        Ok(())
    }

    fn quarantine_segment(
        &self,
        segment_id: u64,
        quarantine_dir: &Path,
    ) -> Result<(usize, u64), StorageError> {
        let mut moved = 0_usize;
        let mut bytes = 0_u64;
        for path in [
            self.segment_path(segment_id),
            self.segment_manifest_path(segment_id),
        ] {
            if !path.exists() {
                continue;
            }
            bytes += fs::metadata(&path)
                .map_err(|error| StorageError::Io(error.to_string()))?
                .len();
            let target = quarantine_dir.join(
                path.file_name()
                    .ok_or_else(|| StorageError::Codec("missing segment filename".to_owned()))?,
            );
            if target.exists() {
                fs::remove_file(&target).map_err(|error| StorageError::Io(error.to_string()))?;
            }
            fs::rename(&path, target).map_err(|error| StorageError::Io(error.to_string()))?;
            moved += 1;
        }
        Ok(((moved > 0) as usize, bytes))
    }

    fn previous_segment_manifest(&self, segment_id: u64) -> Option<SegmentManifest> {
        self.segment_ids()
            .ok()?
            .into_iter()
            .filter(|candidate| *candidate < segment_id)
            .filter_map(|candidate| self.read_segment_manifest(candidate).ok())
            .max_by_key(|manifest| manifest.segment_id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SegmentManifest {
    pub schema_version: u32,
    pub segment_id: u64,
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub event_count: usize,
    pub event_checksum: String,
}

impl SegmentManifest {
    fn from_records(segment_id: u64, records: &[WalRecord]) -> Result<Self, StorageError> {
        let first_sequence = records
            .first()
            .ok_or_else(|| StorageError::Codec("empty WAL segment".to_owned()))?
            .sequence;
        let last_sequence = records
            .last()
            .ok_or_else(|| StorageError::Codec("empty WAL segment".to_owned()))?
            .sequence;
        Ok(Self {
            schema_version: 1,
            segment_id,
            first_sequence,
            last_sequence,
            event_count: records.len(),
            event_checksum: checksum_wal_records(records),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SegmentedWalRecoveryReport {
    pub records_recovered: u64,
    pub last_good_sequence: Option<u64>,
    pub segments_quarantined: usize,
    pub bytes_quarantined: u64,
    pub corruption_reason: Option<String>,
}

fn segment_wal_file_name(segment_id: u64) -> String {
    format!("segment-{segment_id:020}.wal")
}

fn segment_manifest_file_name(segment_id: u64) -> String {
    format!("segment-{segment_id:020}.manifest")
}

fn parse_segment_wal_file_name(path: &Path) -> Option<u64> {
    let filename = path.file_name()?.to_str()?;
    let id = filename.strip_prefix("segment-")?.strip_suffix(".wal")?;
    id.parse::<u64>().ok()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WalOptions {
    pub fsync_policy: FsyncPolicy,
}

impl WalOptions {
    pub fn new(fsync_policy: FsyncPolicy) -> Self {
        Self { fsync_policy }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FsyncPolicy {
    EveryWrite,
    EveryNWrites(u64),
    Never,
}

impl FsyncPolicy {
    fn should_sync(&self, sequence: u64) -> bool {
        match self {
            Self::EveryWrite => true,
            Self::EveryNWrites(interval) => *interval > 0 && sequence % interval == 0,
            Self::Never => false,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WalAppendMetadata {
    pub idempotency_key: Option<String>,
}

impl WalAppendMetadata {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WalRecord {
    pub sequence: u64,
    pub event_id: String,
    pub transaction_time: TxTime,
    pub idempotency_key: Option<String>,
    pub checksum: String,
    pub event: GraphEvent,
}

impl WalRecord {
    fn new(sequence: u64, event: GraphEvent, idempotency_key: Option<String>) -> Self {
        let event_id = event.event_id().as_str().to_owned();
        let transaction_time = event.transaction_time();
        let payload = encode_event(&event);
        let checksum = checksum_hex(
            wal_checksum_input(
                sequence,
                &event_id,
                transaction_time,
                idempotency_key.as_deref(),
                &payload,
            )
            .as_bytes(),
        );
        Self {
            sequence,
            event_id,
            transaction_time,
            idempotency_key,
            checksum,
            event,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WalRecoveryReport {
    pub records_recovered: u64,
    pub last_good_sequence: Option<u64>,
    pub bytes_quarantined: u64,
    pub corruption_reason: Option<String>,
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
        for (index, event) in storage.events().iter().enumerate() {
            let record = WalRecord::new(index as u64 + 1, event.clone(), None);
            file.write_all(encode_event_record(&record).as_bytes())
                .map_err(|error| StorageError::Io(error.to_string()))?;
            file.write_all(b"\n")
                .map_err(|error| StorageError::Io(error.to_string()))?;
        }
        file.sync_data()
            .map_err(|error| StorageError::Io(error.to_string()))
    }

    pub fn write_atomic(
        path: impl AsRef<Path>,
        storage: &InMemoryStorage,
    ) -> Result<(), StorageError> {
        let path = path.as_ref();
        let temp_path = path.with_extension("tmp");
        Self::write(&temp_path, storage)?;
        fs::rename(&temp_path, path).map_err(|error| StorageError::Io(error.to_string()))?;
        if let Some(parent) = path.parent() {
            if let Ok(directory) = File::open(parent) {
                let _ = directory.sync_data();
            }
        }
        Ok(())
    }
}

pub struct SnapshotReader;

impl SnapshotReader {
    pub fn manifest(path: impl AsRef<Path>) -> Result<SnapshotManifest, StorageError> {
        read_snapshot(path).map(|(manifest, _)| manifest)
    }

    pub fn read(path: impl AsRef<Path>) -> Result<InMemoryStorage, StorageError> {
        let (manifest, events) = read_snapshot(path)?;
        let storage = InMemoryStorage::replay(&events)?;
        let actual = SnapshotManifest::from_storage(&storage);
        if !snapshot_manifests_match(&actual, &manifest) {
            return Err(StorageError::SnapshotMismatch);
        }
        Ok(storage)
    }
}

fn read_snapshot(
    path: impl AsRef<Path>,
) -> Result<(SnapshotManifest, Vec<GraphEvent>), StorageError> {
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
            events.push(decode_event_record(&line, events.len() as u64 + 1)?.event);
        }
        let storage = InMemoryStorage::replay(&events)?;
        return Ok((SnapshotManifest::from_storage(&storage), events));
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
        events.push(decode_event_record(&line, events.len() as u64 + 1)?.event);
    }
    Ok((manifest, events))
}

fn snapshot_manifests_match(actual: &SnapshotManifest, expected: &SnapshotManifest) -> bool {
    actual.schema_version == expected.schema_version
        && actual.event_count == expected.event_count
        && actual.last_event_id == expected.last_event_id
        && actual.event_checksum == expected.event_checksum
        && match expected.wal_lsn_boundary {
            Some(boundary) => actual.wal_lsn_boundary == Some(boundary),
            None => true,
        }
        && (expected.graph_state_hash.is_empty()
            || actual.graph_state_hash == expected.graph_state_hash)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotManifest {
    pub schema_version: u32,
    pub event_count: usize,
    pub last_event_id: Option<String>,
    pub event_checksum: String,
    pub wal_lsn_boundary: Option<u64>,
    pub graph_state_hash: String,
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
            wal_lsn_boundary: Some(storage.events().len() as u64),
            graph_state_hash: deterministic_state_hash(storage),
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
    pub event_checksum: String,
    pub graph_state_hash: String,
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
            event_checksum: checksum_events(storage.events()),
            graph_state_hash: deterministic_state_hash(storage),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestoreReport {
    pub manifest: BackupManifest,
    pub restored_state_hash: String,
    pub event_checksum: String,
    pub query_parity_checked: bool,
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
        for (index, event) in storage.events().iter().enumerate() {
            let record = WalRecord::new(index as u64 + 1, event.clone(), None);
            file.write_all(encode_event_record(&record).as_bytes())
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
        if !backup_manifests_match(&BackupManifest::from_storage(&storage), &manifest) {
            return Err(StorageError::SnapshotMismatch);
        }
        Ok(storage)
    }

    pub fn restore_report(path: impl AsRef<Path>) -> Result<RestoreReport, StorageError> {
        let (manifest, events) = read_backup(path)?;
        let storage = InMemoryStorage::replay(&events)?;
        let actual = BackupManifest::from_storage(&storage);
        if !backup_manifests_match(&actual, &manifest) {
            return Err(StorageError::SnapshotMismatch);
        }
        Ok(RestoreReport {
            manifest: actual,
            restored_state_hash: deterministic_state_hash(&storage),
            event_checksum: checksum_events(storage.events()),
            query_parity_checked: backup_query_parity(&storage),
        })
    }
}

fn backup_manifests_match(actual: &BackupManifest, expected: &BackupManifest) -> bool {
    actual.event_count == expected.event_count
        && actual.entity_count == expected.entity_count
        && actual.assertion_count == expected.assertion_count
        && actual.source_count == expected.source_count
        && actual.last_event_id == expected.last_event_id
        && (expected.event_checksum.is_empty() || actual.event_checksum == expected.event_checksum)
        && (expected.graph_state_hash.is_empty()
            || actual.graph_state_hash == expected.graph_state_hash)
}

fn backup_query_parity(storage: &InMemoryStorage) -> bool {
    storage
        .graph_state()
        .assertions
        .values()
        .all(|assertion| storage.assertion(&assertion.id).is_some())
        && storage
            .graph_state()
            .entities
            .values()
            .all(|entity| storage.entity(&entity.id).is_some())
        && storage
            .graph_state()
            .sources
            .values()
            .all(|source| storage.source(&source.id).is_some())
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
        events.push(decode_event_record(&line, events.len() as u64 + 1)?.event);
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
        manifest.event_checksum.clone(),
        manifest.graph_state_hash.clone(),
    ])
}

fn encode_segment_manifest(manifest: &SegmentManifest) -> String {
    encode_parts(&[
        "segment_manifest".to_owned(),
        "schema_version".to_owned(),
        manifest.schema_version.to_string(),
        "segment_id".to_owned(),
        manifest.segment_id.to_string(),
        "first_sequence".to_owned(),
        manifest.first_sequence.to_string(),
        "last_sequence".to_owned(),
        manifest.last_sequence.to_string(),
        "event_count".to_owned(),
        manifest.event_count.to_string(),
        "event_checksum".to_owned(),
        manifest.event_checksum.clone(),
    ])
}

fn decode_segment_manifest(record: &str) -> Result<SegmentManifest, StorageError> {
    let parts = decode_parts(record)?;
    if required(&parts, 0, "segment manifest kind")? != "segment_manifest"
        || required(&parts, 1, "schema version key")? != "schema_version"
        || required(&parts, 3, "segment id key")? != "segment_id"
        || required(&parts, 5, "first sequence key")? != "first_sequence"
        || required(&parts, 7, "last sequence key")? != "last_sequence"
        || required(&parts, 9, "event count key")? != "event_count"
        || required(&parts, 11, "event checksum key")? != "event_checksum"
    {
        return Err(StorageError::Codec(
            "invalid segment manifest fields".to_owned(),
        ));
    }
    Ok(SegmentManifest {
        schema_version: parse_u32(required(&parts, 2, "schema version")?)?,
        segment_id: parse_u64(required(&parts, 4, "segment id")?)?,
        first_sequence: parse_u64(required(&parts, 6, "first sequence")?)?,
        last_sequence: parse_u64(required(&parts, 8, "last sequence")?)?,
        event_count: parse_usize(required(&parts, 10, "event count")?)?,
        event_checksum: required(&parts, 12, "event checksum")?.to_owned(),
    })
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
        "wal_lsn_boundary".to_owned(),
        manifest
            .wal_lsn_boundary
            .map(|sequence| sequence.to_string())
            .unwrap_or_default(),
        "graph_state_hash".to_owned(),
        manifest.graph_state_hash.clone(),
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
    let wal_lsn_boundary = if parts.len() > 10 {
        if required(&parts, 9, "wal lsn boundary key")? != "wal_lsn_boundary"
            || required(&parts, 11, "graph state hash key")? != "graph_state_hash"
        {
            return Err(StorageError::Codec(
                "invalid snapshot manifest fields".to_owned(),
            ));
        }
        match required(&parts, 10, "wal lsn boundary")? {
            "" => None,
            value => Some(parse_u64(value)?),
        }
    } else {
        None
    };
    let graph_state_hash = if parts.len() > 12 {
        required(&parts, 12, "graph state hash")?.to_owned()
    } else {
        String::new()
    };
    Ok(SnapshotManifest {
        schema_version: parse_u32(required(&parts, 2, "schema version")?)?,
        event_count: parse_usize(required(&parts, 4, "event count")?)?,
        last_event_id,
        event_checksum: required(&parts, 8, "event checksum")?.to_owned(),
        wal_lsn_boundary,
        graph_state_hash,
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
        event_checksum: parts.get(5).cloned().unwrap_or_default(),
        graph_state_hash: parts.get(6).cloned().unwrap_or_default(),
    })
}

fn encode_event_record(record: &WalRecord) -> String {
    let payload = encode_event(&record.event);
    encode_parts(&[
        EVENT_RECORD_KIND.to_owned(),
        record.sequence.to_string(),
        record.checksum.clone(),
        EVENT_RECORD_VERSION.to_owned(),
        record.event_id.clone(),
        record.transaction_time.as_i64().to_string(),
        record.idempotency_key.clone().unwrap_or_default(),
        payload,
    ])
}

fn decode_event_record(record: &str, expected_sequence: u64) -> Result<WalRecord, StorageError> {
    let parts = decode_parts(record)?;
    if parts
        .first()
        .is_some_and(|kind| kind.as_str() == EVENT_RECORD_KIND)
    {
        if parts.len() == 4
            && required(&parts, 1, "event record version")? == LEGACY_EVENT_RECORD_VERSION
        {
            let checksum = required(&parts, 2, "event checksum")?;
            let payload = required(&parts, 3, "event payload")?;
            let actual = checksum_hex(payload.as_bytes());
            if checksum != actual {
                return Err(StorageError::Codec(format!(
                    "event checksum mismatch: expected {checksum}, got {actual}"
                )));
            }
            let event = decode_event(payload)?;
            return Ok(WalRecord {
                sequence: expected_sequence,
                event_id: event.event_id().as_str().to_owned(),
                transaction_time: event.transaction_time(),
                idempotency_key: None,
                checksum: checksum.to_owned(),
                event,
            });
        }
        let sequence = parse_u64(required(&parts, 1, "event sequence")?)?;
        if sequence != expected_sequence {
            return Err(StorageError::Codec(format!(
                "event sequence mismatch: expected {expected_sequence}, got {sequence}"
            )));
        }
        let checksum = required(&parts, 2, "event checksum")?;
        if required(&parts, 3, "event record version")? != EVENT_RECORD_VERSION {
            return Err(StorageError::Codec(
                "unsupported event record version".to_owned(),
            ));
        }
        let event_id = required(&parts, 4, "event id")?.to_owned();
        let transaction_time = TxTime::new(parse_i64(required(&parts, 5, "transaction time")?)?);
        let idempotency_key = match required(&parts, 6, "idempotency key")? {
            "" => None,
            value => Some(value.to_owned()),
        };
        let payload = required(&parts, 7, "event payload")?;
        let actual = checksum_hex(
            wal_checksum_input(
                sequence,
                &event_id,
                transaction_time,
                idempotency_key.as_deref(),
                payload,
            )
            .as_bytes(),
        );
        if checksum != actual {
            return Err(StorageError::Codec(format!(
                "event checksum mismatch: expected {checksum}, got {actual}"
            )));
        }
        let event = decode_event(payload)?;
        if event.event_id().as_str() != event_id {
            return Err(StorageError::Codec("event id metadata mismatch".to_owned()));
        }
        if event.transaction_time() != transaction_time {
            return Err(StorageError::Codec(
                "transaction time metadata mismatch".to_owned(),
            ));
        }
        return Ok(WalRecord {
            sequence,
            event_id,
            transaction_time,
            idempotency_key,
            checksum: checksum.to_owned(),
            event,
        });
    }
    let event = decode_event(record)?;
    Ok(WalRecord {
        sequence: expected_sequence,
        event_id: event.event_id().as_str().to_owned(),
        transaction_time: event.transaction_time(),
        idempotency_key: None,
        checksum: checksum_hex(record.as_bytes()),
        event,
    })
}

fn wal_checksum_input(
    sequence: u64,
    event_id: &str,
    transaction_time: TxTime,
    idempotency_key: Option<&str>,
    payload: &str,
) -> String {
    encode_parts(&[
        sequence.to_string(),
        EVENT_RECORD_VERSION.to_owned(),
        event_id.to_owned(),
        transaction_time.as_i64().to_string(),
        idempotency_key.unwrap_or_default().to_owned(),
        payload.to_owned(),
    ])
}

fn checksum_events(events: &[GraphEvent]) -> String {
    let mut bytes = Vec::new();
    for event in events {
        bytes.extend_from_slice(encode_event(event).as_bytes());
        bytes.push(b'\n');
    }
    checksum_hex(&bytes)
}

fn checksum_wal_records(records: &[WalRecord]) -> String {
    let mut bytes = Vec::new();
    for record in records {
        bytes.extend_from_slice(record.sequence.to_string().as_bytes());
        bytes.push(b':');
        bytes.extend_from_slice(record.checksum.as_bytes());
        bytes.push(b'\n');
    }
    checksum_hex(&bytes)
}

pub fn deterministic_state_hash(storage: &InMemoryStorage) -> String {
    let state = storage.graph_state();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"events\n");
    for event in storage.events() {
        bytes.extend_from_slice(encode_event(event).as_bytes());
        bytes.push(b'\n');
    }
    bytes.extend_from_slice(b"entities\n");
    for (id, entity) in &state.entities {
        bytes.extend_from_slice(id.as_str().as_bytes());
        bytes.push(b'=');
        bytes.extend_from_slice(encode_entity(entity).as_bytes());
        bytes.push(b'\n');
    }
    bytes.extend_from_slice(b"assertions\n");
    for (id, assertion) in &state.assertions {
        bytes.extend_from_slice(id.as_str().as_bytes());
        bytes.push(b'=');
        bytes.extend_from_slice(encode_assertion(assertion).as_bytes());
        bytes.push(b'\n');
    }
    bytes.extend_from_slice(b"sources\n");
    for (id, source) in &state.sources {
        bytes.extend_from_slice(id.as_str().as_bytes());
        bytes.push(b'=');
        bytes.extend_from_slice(encode_source(source).as_bytes());
        bytes.push(b'\n');
    }
    bytes.extend_from_slice(b"memories\n");
    for (id, memory) in &state.agent_memories {
        bytes.extend_from_slice(id.as_str().as_bytes());
        bytes.push(b'=');
        bytes.extend_from_slice(encode_agent_memory(memory).as_bytes());
        bytes.push(b'\n');
    }
    bytes.extend_from_slice(b"causal_links\n");
    for (id, causal_link) in &state.causal_links {
        bytes.extend_from_slice(id.as_str().as_bytes());
        bytes.push(b'=');
        bytes.extend_from_slice(encode_causal_link(causal_link).as_bytes());
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
    use std::time::{SystemTime, UNIX_EPOCH};

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

    fn temp_dir(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        path.push(format!(
            "reality-graph-storage-{name}-{}-{nanos}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp dir");
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
    fn wal_records_include_sequence_transaction_time_idempotency_and_checksum() {
        let path = temp_file("wal-metadata");
        let events = sample_events();
        {
            let mut file_log =
                FileEventLog::open_with_options(&path, WalOptions::new(FsyncPolicy::EveryWrite))
                    .expect("open log");
            file_log
                .append_with_metadata(
                    &events[0],
                    WalAppendMetadata::new().with_idempotency_key("source-1-once"),
                )
                .expect("append source");
            file_log
                .append_with_metadata(&events[1], WalAppendMetadata::new())
                .expect("append entity");
        }

        let reloaded = FileEventLog::open(&path).expect("reopen log");
        let records = reloaded.read_records().expect("read records");

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].sequence, 1);
        assert_eq!(records[1].sequence, 2);
        assert_eq!(records[0].event_id, events[0].event_id().as_str());
        assert_eq!(records[0].transaction_time, events[0].transaction_time());
        assert_eq!(records[0].idempotency_key.as_deref(), Some("source-1-once"));
        assert_eq!(records[0].event, events[0]);
        assert_eq!(records[1].idempotency_key, None);
        assert!(!records[0].checksum.is_empty());

        fs::remove_file(path).expect("cleanup");
    }

    #[test]
    fn wal_recovery_truncates_partial_tail_and_reports_quarantined_bytes() {
        let path = temp_file("wal-truncate-tail");
        let events = sample_events();
        {
            let mut file_log = FileEventLog::open(&path).expect("open log");
            file_log.append(&events[0]).expect("append source");
            file_log.append(&events[1]).expect("append entity");
        }
        let original_len = fs::metadata(&path).expect("metadata").len();
        {
            let mut file = OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("open append");
            file.write_all(b"RGEVENT|3|partial")
                .expect("write torn record");
            file.sync_data().expect("sync torn record");
        }

        let mut file_log = FileEventLog::open(&path).expect("reopen log");
        assert!(file_log.read_all().is_err());

        let report = file_log
            .recover_truncate_to_last_good()
            .expect("recover last good");
        assert_eq!(report.records_recovered, 2);
        assert_eq!(report.last_good_sequence, Some(2));
        assert!(report.bytes_quarantined > 0);
        assert_eq!(fs::metadata(&path).expect("metadata").len(), original_len);
        assert_eq!(
            file_log.read_all().expect("read after recovery"),
            events[..2]
        );

        fs::remove_file(path).expect("cleanup");
    }

    #[test]
    fn wal_rejects_out_of_order_sequence_numbers() {
        let path = temp_file("wal-out-of-order");
        let events = sample_events();
        {
            let mut file_log = FileEventLog::open(&path).expect("open log");
            file_log.append(&events[0]).expect("append source");
            file_log.append(&events[1]).expect("append entity");
        }
        let contents = fs::read_to_string(&path).expect("read log");
        let mut lines = contents.lines().map(str::to_owned).collect::<Vec<_>>();
        let mut second_record = decode_parts(&lines[1]).expect("decode record parts");
        second_record[1] = "4".to_owned();
        lines[1] = encode_parts(&second_record);
        fs::write(&path, format!("{}\n", lines.join("\n"))).expect("write out of order log");

        let reloaded = FileEventLog::open(&path).expect("reopen log");
        assert!(matches!(
            reloaded.read_all(),
            Err(StorageError::Codec(message)) if message.contains("sequence")
        ));

        fs::remove_file(path).expect("cleanup");
    }

    #[test]
    fn segmented_wal_rotates_segments_and_writes_manifests() {
        let dir = temp_dir("segmented-rotate");
        let events = sample_events();
        let mut wal = SegmentedWal::open(&dir, SegmentedWalOptions::new(2)).expect("open wal");

        for event in &events {
            wal.append(event).expect("append event");
        }

        let records = wal.read_records().expect("read segmented records");
        let manifests = wal.manifests().expect("read manifests");

        assert_eq!(records.len(), events.len());
        assert_eq!(records[0].sequence, 1);
        assert_eq!(records[3].sequence, 4);
        assert_eq!(manifests.len(), 2);
        assert_eq!(manifests[0].segment_id, 1);
        assert_eq!(manifests[0].first_sequence, 1);
        assert_eq!(manifests[0].last_sequence, 2);
        assert_eq!(manifests[0].event_count, 2);
        assert_eq!(manifests[1].first_sequence, 3);
        assert_eq!(manifests[1].last_sequence, 4);
        assert!(dir.join(segment_manifest_file_name(1)).exists());
        assert!(dir.join(segment_manifest_file_name(2)).exists());

        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn segmented_wal_rejects_reordered_segments() {
        let dir = temp_dir("segmented-reordered");
        let events = sample_events();
        let mut wal = SegmentedWal::open(&dir, SegmentedWalOptions::new(2)).expect("open wal");
        for event in &events {
            wal.append(event).expect("append event");
        }

        let first_wal = dir.join(segment_wal_file_name(1));
        let second_wal = dir.join(segment_wal_file_name(2));
        let first_manifest = dir.join(segment_manifest_file_name(1));
        let second_manifest = dir.join(segment_manifest_file_name(2));
        let tmp_wal = dir.join("segment-swap.wal");
        let tmp_manifest = dir.join("segment-swap.manifest");
        fs::rename(&first_wal, &tmp_wal).expect("move first wal");
        fs::rename(&second_wal, &first_wal).expect("move second wal");
        fs::rename(&tmp_wal, &second_wal).expect("move first wal into second slot");
        fs::rename(&first_manifest, &tmp_manifest).expect("move first manifest");
        fs::rename(&second_manifest, &first_manifest).expect("move second manifest");
        fs::rename(&tmp_manifest, &second_manifest).expect("move first manifest into second slot");

        let reloaded = SegmentedWal::open(&dir, SegmentedWalOptions::new(2)).expect("reopen wal");
        assert!(matches!(
            reloaded.read_records(),
            Err(StorageError::Codec(message))
                if message.contains("manifest mismatch") || message.contains("sequence")
        ));

        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn segmented_wal_archives_compacted_segments_and_reads_tail() {
        let dir = temp_dir("segmented-archive");
        let archive_dir = dir.join("archive");
        let events = sample_events();
        let mut wal = SegmentedWal::open(
            &dir,
            SegmentedWalOptions::new(2).with_archive_dir(&archive_dir),
        )
        .expect("open wal");
        for event in &events {
            wal.append(event).expect("append event");
        }

        let archived = wal
            .archive_compacted_segments(2)
            .expect("archive compacted segment");
        let tail = wal.read_tail_after(2).expect("read tail");

        assert_eq!(archived, 1);
        assert!(!dir.join(segment_wal_file_name(1)).exists());
        assert!(archive_dir.join(segment_wal_file_name(1)).exists());
        assert_eq!(tail.len(), 2);
        assert_eq!(tail[0].sequence, 3);
        assert_eq!(tail[1].sequence, 4);

        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn segmented_wal_restores_snapshot_plus_wal_tail() {
        let dir = temp_dir("segmented-tail-restore");
        let snapshot_path = dir.join("snapshot.rgsnap");
        let events = sample_events();
        let mut wal = SegmentedWal::open(&dir, SegmentedWalOptions::new(2)).expect("open wal");
        for event in &events {
            wal.append(event).expect("append event");
        }
        let snapshot_storage = InMemoryStorage::replay(&events[..2]).expect("snapshot replay");
        SnapshotWriter::write_atomic(&snapshot_path, &snapshot_storage).expect("write snapshot");
        let expected = InMemoryStorage::replay(&events).expect("full replay");

        let restored = SegmentedWal::restore_snapshot_and_tail(&snapshot_path, &wal)
            .expect("restore snapshot plus tail");

        assert_eq!(restored.events(), expected.events());
        assert_eq!(restored.graph_state(), expected.graph_state());
        assert_eq!(
            deterministic_state_hash(&restored),
            deterministic_state_hash(&expected)
        );

        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn segmented_wal_quarantines_corrupt_segment() {
        let dir = temp_dir("segmented-quarantine");
        let quarantine_dir = dir.join("quarantine");
        let events = sample_events();
        let mut wal = SegmentedWal::open(
            &dir,
            SegmentedWalOptions::new(2).with_quarantine_dir(&quarantine_dir),
        )
        .expect("open wal");
        for event in &events {
            wal.append(event).expect("append event");
        }
        let second_segment = dir.join(segment_wal_file_name(2));
        let mut contents = fs::read_to_string(&second_segment).expect("read segment");
        contents.push_str("RGEVENT|5|partial");
        fs::write(&second_segment, contents).expect("corrupt segment");

        let report = wal
            .recover_quarantine_corrupt_segments()
            .expect("recover corrupt segment");

        assert_eq!(report.segments_quarantined, 1);
        assert!(report.bytes_quarantined > 0);
        assert!(report.corruption_reason.is_some());
        assert!(quarantine_dir.join(segment_wal_file_name(2)).exists());
        assert_eq!(wal.read_records().expect("read remaining records").len(), 2);

        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn deterministic_state_hash_changes_when_materialized_content_changes() {
        let first_events = sample_events();
        let mut second_events = sample_events();
        if let GraphEvent::EntityCreated(event) = &mut second_events[1] {
            event.entity.canonical_name = Some("Person A Renamed".to_owned());
        }
        let first = InMemoryStorage::replay(&first_events).expect("first replay");
        let second = InMemoryStorage::replay(&second_events).expect("second replay");

        assert_ne!(
            deterministic_state_hash(&first),
            deterministic_state_hash(&second)
        );
    }

    #[test]
    fn redb_graph_store_appends_events_and_reopens_materialized_state() {
        let path = temp_file("redb-store");
        let events = sample_events();
        let expected = InMemoryStorage::replay(&events).expect("expected replay");

        {
            let mut store = RedbGraphStore::create(&path).expect("create redb store");
            for event in &events {
                store
                    .append_event(event, None)
                    .expect("append durable event");
            }

            assert_eq!(
                store.health().expect("health").last_lsn,
                events.len() as u64
            );
            assert!(store
                .entity(&EntityId::new("person-a"))
                .expect("read entity")
                .is_some());
            assert!(store
                .assertion(&AssertionId::new("assertion-1"))
                .expect("read assertion")
                .is_some());
            assert!(store
                .source(&SourceId::new("source-1"))
                .expect("read source")
                .is_some());
        }

        let reopened = RedbGraphStore::open(&path).expect("reopen redb store");
        let restored = reopened
            .materialized_storage()
            .expect("restore materialized storage");

        assert_eq!(restored.events(), expected.events());
        assert_eq!(restored.graph_state(), expected.graph_state());
        assert_eq!(
            deterministic_state_hash(&restored),
            deterministic_state_hash(&expected)
        );

        fs::remove_file(path).expect("cleanup");
    }

    #[test]
    fn redb_graph_store_persists_idempotency_records() {
        let path = temp_file("redb-idempotency");
        let events = sample_events();

        {
            let mut store = RedbGraphStore::create(&path).expect("create redb store");
            let first = store
                .append_event(&events[0], Some("source-1-once"))
                .expect("first append");
            let replayed = store
                .append_event(&events[0], Some("source-1-once"))
                .expect("idempotent append");

            assert_eq!(first.lsn, 1);
            assert_eq!(replayed.lsn, 1);
            assert!(replayed.idempotency_replayed);
            assert_eq!(store.health().expect("health").last_lsn, 1);
        }

        let reopened = RedbGraphStore::open(&path).expect("reopen redb store");
        assert_eq!(
            reopened
                .idempotency_lsn("source-1-once")
                .expect("read idempotency lsn"),
            Some(1)
        );
        assert_eq!(reopened.events_by_lsn(1, 10).expect("read events").len(), 1);

        fs::remove_file(path).expect("cleanup");
    }

    #[test]
    fn redb_graph_store_reads_events_by_lsn_range() {
        let path = temp_file("redb-lsn-range");
        let events = sample_events();

        {
            let mut store = RedbGraphStore::create(&path).expect("create redb store");
            for event in &events {
                store.append_event(event, None).expect("append event");
            }
        }

        let reopened = RedbGraphStore::open(&path).expect("reopen redb store");
        let range = reopened.events_by_lsn(2, 3).expect("read lsn range");

        assert_eq!(range.len(), 2);
        assert_eq!(range[0].0, 2);
        assert_eq!(range[0].1, events[1]);
        assert_eq!(range[1].0, 3);
        assert_eq!(range[1].1, events[2]);

        fs::remove_file(path).expect("cleanup");
    }

    #[test]
    fn redb_graph_store_tracks_schema_migration_version() {
        let path = temp_file("redb-schema");

        {
            let mut store = RedbGraphStore::create(&path).expect("create redb store");
            assert_eq!(store.schema_version().expect("schema version"), 1);
            store
                .record_schema_migration(2, "tenant-partition-index")
                .expect("record migration");
        }

        let reopened = RedbGraphStore::open(&path).expect("reopen redb store");
        assert_eq!(reopened.schema_version().expect("schema version"), 2);
        assert_eq!(
            reopened
                .migration_history()
                .expect("migration history")
                .last()
                .map(|entry| entry.name.as_str()),
            Some("tenant-partition-index")
        );

        fs::remove_file(path).expect("cleanup");
    }

    #[test]
    fn redb_graph_store_enforces_writer_lease_fencing() {
        let path = temp_file("redb-lease");
        let mut store = RedbGraphStore::create(&path).expect("create redb store");

        let first = store
            .acquire_writer_lease("writer-a", 1_000, 5_000)
            .expect("acquire first lease");
        assert_eq!(first.holder_id, "writer-a");
        assert_eq!(first.fencing_token, 1);

        let rejected = store
            .try_acquire_writer_lease("writer-b", 2_000, 5_000)
            .expect("try acquire competing lease");
        assert!(matches!(
            rejected,
            WriterLeaseAttempt::Rejected(existing)
                if existing.holder_id == "writer-a" && existing.fencing_token == 1
        ));

        let second = store
            .acquire_writer_lease("writer-b", 7_000, 5_000)
            .expect("acquire expired lease");
        assert_eq!(second.holder_id, "writer-b");
        assert_eq!(second.fencing_token, 2);

        fs::remove_file(path).expect("cleanup");
    }

    #[test]
    fn snapshots_include_manifest_and_detect_corruption() {
        let path = temp_file("snapshot-manifest");
        let events = sample_events();
        let storage = InMemoryStorage::replay(&events).expect("replay succeeds");

        SnapshotWriter::write(&path, &storage).expect("write snapshot");
        let manifest = SnapshotReader::manifest(&path).expect("read manifest");
        assert_eq!(manifest.wal_lsn_boundary, Some(events.len() as u64));
        assert_eq!(
            manifest.graph_state_hash,
            deterministic_state_hash(&storage)
        );

        let mut contents = fs::read_to_string(&path).expect("read snapshot");
        contents = contents.replacen("event_count", "event_count_corrupt", 1);
        fs::write(&path, contents).expect("corrupt snapshot");

        let result = SnapshotReader::read(&path);
        assert!(result.is_err());

        fs::remove_file(path).expect("cleanup");
    }

    #[test]
    fn snapshot_atomic_publish_never_leaves_missing_manifest() {
        let path = temp_file("snapshot-atomic");
        let events = sample_events();
        let storage = InMemoryStorage::replay(&events).expect("replay succeeds");

        SnapshotWriter::write_atomic(&path, &storage).expect("write atomic snapshot");
        let manifest = SnapshotReader::manifest(&path).expect("manifest");
        let restored = SnapshotReader::read(&path).expect("read snapshot");

        assert_eq!(
            manifest.graph_state_hash,
            deterministic_state_hash(&restored)
        );
        assert_eq!(restored.events(), storage.events());
        assert!(!path.with_extension("tmp").exists());

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
        let report = BackupReader::restore_report(&path).expect("restore report");

        assert_eq!(manifest, restored_manifest);
        assert_eq!(report.manifest, manifest);
        assert_eq!(
            report.restored_state_hash,
            deterministic_state_hash(&storage)
        );
        assert_eq!(report.event_checksum, checksum_events(storage.events()));
        assert!(report.query_parity_checked);
        assert_eq!(manifest.event_count, events.len());
        assert_eq!(manifest.entity_count, 2);
        assert_eq!(manifest.assertion_count, 1);
        assert_eq!(manifest.source_count, 1);
        assert_eq!(restored.events(), storage.events());
        assert_eq!(restored.graph_state(), storage.graph_state());

        fs::remove_file(path).expect("cleanup");
    }
}
