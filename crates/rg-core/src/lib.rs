//! Assertion-first domain primitives for Reality Graph.

use std::collections::BTreeMap;
use std::fmt;

pub mod ontology;
pub use ontology::{
    EntityTypeSchema, GraphOntology, GraphValueKind, OntologyError, OntologyTypeRef,
    OntologyValidationError, PredicateConstraints, PredicateSchema, PropertyType,
};

macro_rules! string_newtype {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

macro_rules! timestamp_newtype {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
        pub struct $name(i64);

        impl $name {
            pub fn new(value: i64) -> Self {
                Self(value)
            }

            pub fn as_i64(self) -> i64 {
                self.0
            }
        }
    };
}

string_newtype!(EntityId);
string_newtype!(AssertionId);
string_newtype!(AgentId);
string_newtype!(MemoryId);
string_newtype!(TenantId);
string_newtype!(PredicateId);
string_newtype!(SourceId);
string_newtype!(EventId);
string_newtype!(CausalLinkId);
string_newtype!(ContradictionId);
string_newtype!(ContentHash);
string_newtype!(PropertyKey);

timestamp_newtype!(ValidTime);
timestamp_newtype!(TxTime);

#[derive(Clone, Debug, PartialEq)]
pub struct Entity {
    pub id: EntityId,
    pub entity_type: EntityType,
    pub canonical_name: Option<String>,
    pub properties: PropertyMap,
    pub created_tx: TxTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntityType {
    Person,
    Organization,
    Place,
    Event,
    Document,
    Concept,
    Custom(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PropertyMap(pub BTreeMap<PropertyKey, GraphValue>);

#[derive(Clone, Debug, PartialEq)]
pub enum GraphValue {
    Entity(EntityId),
    Text(String),
    Integer(i64),
    Decimal(f64),
    Boolean(bool),
    Time(ValidTime),
    Null,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Assertion {
    pub id: AssertionId,
    pub subject: EntityId,
    pub predicate: PredicateId,
    pub object: GraphValue,
    pub valid_time: TimeInterval<ValidTime>,
    pub transaction_time: TimeInterval<TxTime>,
    pub confidence: Confidence,
    pub source_ids: Vec<SourceId>,
    pub context: ContextScope,
    pub status: AssertionStatus,
}

impl Assertion {
    pub fn is_visible_at(&self, valid_time: ValidTime, transaction_time: TxTime) -> bool {
        self.status == AssertionStatus::Active
            && self.valid_time.contains(valid_time)
            && self.transaction_time.contains(transaction_time)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssertionStatus {
    Active,
    Retracted,
    Superseded,
    Disputed,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ContextScope {
    Global,
    Named(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub id: SourceId,
    pub source_type: SourceType,
    pub uri: Option<String>,
    pub content_hash: ContentHash,
    pub observed_at: TxTime,
    pub trust_score: Option<f32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceType {
    Document,
    WebPage,
    DatabaseRecord,
    ApiResponse,
    HumanReport,
    SensorReading,
    Custom(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub id: EventId,
    pub event_type: EventType,
    pub valid_time: Option<ValidTime>,
    pub transaction_time: TxTime,
    pub source_ids: Vec<SourceId>,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventType {
    SourceObserved,
    EntityObserved,
    AssertionRecorded,
    AssertionCorrected,
    AssertionRetracted,
    CausalLinkRecorded,
    SnapshotCompacted,
    IndexRebuilt,
    WorldEventObserved,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CausalLink {
    pub id: CausalLinkId,
    pub cause_event: EventId,
    pub effect_event: EventId,
    pub confidence: Confidence,
    pub mechanism: Option<String>,
    pub source_ids: Vec<SourceId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentMemory {
    pub id: MemoryId,
    pub agent_id: AgentId,
    pub memory_type: MemoryType,
    pub content: String,
    pub valid_time: TimeInterval<ValidTime>,
    pub confidence: Confidence,
    pub source_ids: Vec<SourceId>,
    pub related_entities: Vec<EntityId>,
    pub supersedes: Vec<MemoryId>,
    pub status: MemoryStatus,
}

impl AgentMemory {
    pub fn is_visible_at(&self, valid_time: ValidTime) -> bool {
        matches!(self.status, MemoryStatus::Active | MemoryStatus::Reinforced)
            && self.valid_time.contains(valid_time)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemoryType {
    Episodic,
    Semantic,
    Procedural,
    Observation,
    Decision,
    Action,
    ToolCall,
    Outcome,
    Preference,
    Goal,
    Plan,
    Reflection,
    Correction,
    Relationship,
    WorldState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemoryStatus {
    Candidate,
    Active,
    Reinforced,
    Superseded,
    Contradicted,
    Archived,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TimeInterval<T> {
    pub start: T,
    pub end: Option<T>,
}

impl<T: Copy + Ord> TimeInterval<T> {
    pub fn new(start: T, end: Option<T>) -> Result<Self, TimeIntervalError> {
        if let Some(end) = end {
            if end < start {
                return Err(TimeIntervalError::EndBeforeStart);
            }
        }

        Ok(Self { start, end })
    }

    pub fn contains(&self, instant: T) -> bool {
        instant >= self.start
            && match self.end {
                Some(end) => instant < end,
                None => true,
            }
    }

    pub fn overlaps(&self, other: &Self) -> bool {
        let self_starts_before_other_ends = match other.end {
            Some(other_end) => self.start < other_end,
            None => true,
        };
        let other_starts_before_self_ends = match self.end {
            Some(self_end) => other.start < self_end,
            None => true,
        };

        self_starts_before_other_ends && other_starts_before_self_ends
    }

    pub fn is_open_ended(&self) -> bool {
        self.end.is_none()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimeIntervalError {
    EndBeforeStart,
}

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Confidence(f32);

impl Confidence {
    pub fn new(value: f32) -> Result<Self, ConfidenceError> {
        if (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(ConfidenceError::OutOfRange)
        }
    }

    pub fn as_f32(self) -> f32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfidenceError {
    OutOfRange,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_and_time_newtypes_preserve_values() {
        assert_eq!(EntityId::new("person-a").as_str(), "person-a");
        assert_eq!(AssertionId::new("assertion-1").as_str(), "assertion-1");
        assert_eq!(AgentId::new("agent-1").as_str(), "agent-1");
        assert_eq!(MemoryId::new("memory-1").as_str(), "memory-1");
        assert_eq!(TenantId::new("tenant-1").as_str(), "tenant-1");
        assert_eq!(PredicateId::new("works_at").as_str(), "works_at");
        assert_eq!(SourceId::new("source-1").as_str(), "source-1");
        assert_eq!(EventId::new("event-1").as_str(), "event-1");
        assert_eq!(CausalLinkId::new("cause-1").as_str(), "cause-1");
        assert_eq!(
            ContradictionId::new("contradiction-1").as_str(),
            "contradiction-1"
        );
        assert_eq!(ContentHash::new("sha256:abc").as_str(), "sha256:abc");
        assert_eq!(PropertyKey::new("title").as_str(), "title");
        assert_eq!(
            ValidTime::new(1_609_459_200_000_000).as_i64(),
            1_609_459_200_000_000
        );
        assert_eq!(
            TxTime::new(1_778_454_000_000_000).as_i64(),
            1_778_454_000_000_000
        );
    }

    #[test]
    fn confidence_accepts_unit_interval_and_rejects_invalid_scores() {
        assert_eq!(
            Confidence::new(0.75).expect("valid confidence").as_f32(),
            0.75
        );
        assert!(Confidence::new(-0.1).is_err());
        assert!(Confidence::new(1.1).is_err());
    }

    #[test]
    fn time_interval_contains_start_and_excludes_end() {
        let interval = TimeInterval::new(ValidTime::new(10), Some(ValidTime::new(20)))
            .expect("valid interval");

        assert!(interval.contains(ValidTime::new(10)));
        assert!(interval.contains(ValidTime::new(19)));
        assert!(!interval.contains(ValidTime::new(20)));
        assert!(!interval.contains(ValidTime::new(9)));
    }

    #[test]
    fn time_interval_detects_overlap_and_adjacency() {
        let first = TimeInterval::new(ValidTime::new(10), Some(ValidTime::new(20)))
            .expect("valid interval");
        let overlapping = TimeInterval::new(ValidTime::new(19), Some(ValidTime::new(30)))
            .expect("valid interval");
        let adjacent = TimeInterval::new(ValidTime::new(20), Some(ValidTime::new(30)))
            .expect("valid interval");

        assert!(first.overlaps(&overlapping));
        assert!(!first.overlaps(&adjacent));
    }

    #[test]
    fn time_interval_supports_open_ended_intervals() {
        let interval = TimeInterval::new(TxTime::new(100), None).expect("valid interval");

        assert!(interval.is_open_ended());
        assert!(interval.contains(TxTime::new(100)));
        assert!(interval.contains(TxTime::new(1_000_000)));
        assert!(!interval.contains(TxTime::new(99)));
    }

    #[test]
    fn time_interval_rejects_end_before_start() {
        let invalid = TimeInterval::new(ValidTime::new(20), Some(ValidTime::new(10)));

        assert_eq!(invalid, Err(TimeIntervalError::EndBeforeStart));
    }

    #[test]
    fn assertion_visibility_requires_valid_and_transaction_time() {
        let assertion = Assertion {
            id: AssertionId::new("employment-1"),
            subject: EntityId::new("person-a"),
            predicate: PredicateId::new("works_at"),
            object: GraphValue::Entity(EntityId::new("company-b")),
            valid_time: TimeInterval::new(
                ValidTime::new(1_609_459_200_000_000),
                Some(ValidTime::new(1_735_689_600_000_000)),
            )
            .expect("valid interval"),
            transaction_time: TimeInterval::new(TxTime::new(1_778_454_000_000_000), None)
                .expect("valid interval"),
            confidence: Confidence::new(0.92).expect("valid confidence"),
            source_ids: vec![SourceId::new("source-1")],
            context: ContextScope::Global,
            status: AssertionStatus::Active,
        };

        assert!(assertion.is_visible_at(
            ValidTime::new(1_672_531_200_000_000),
            TxTime::new(1_778_454_000_000_000),
        ));
        assert!(!assertion.is_visible_at(
            ValidTime::new(1_672_531_200_000_000),
            TxTime::new(1_672_531_200_000_000),
        ));
        assert!(!assertion.is_visible_at(
            ValidTime::new(1_767_225_600_000_000),
            TxTime::new(1_778_454_000_000_000),
        ));
    }

    #[test]
    fn agent_memory_visibility_requires_active_status_and_valid_time() {
        let mut memory = AgentMemory {
            id: MemoryId::new("memory-1"),
            agent_id: AgentId::new("agent-1"),
            memory_type: MemoryType::Observation,
            content: "Company A supplies Company B.".to_owned(),
            valid_time: TimeInterval::new(ValidTime::new(10), Some(ValidTime::new(20)))
                .expect("valid interval"),
            confidence: Confidence::new(0.8).expect("valid confidence"),
            source_ids: vec![SourceId::new("source-1")],
            related_entities: vec![EntityId::new("company-a"), EntityId::new("company-b")],
            supersedes: Vec::new(),
            status: MemoryStatus::Active,
        };

        assert!(memory.is_visible_at(ValidTime::new(15)));
        assert!(!memory.is_visible_at(ValidTime::new(20)));

        memory.status = MemoryStatus::Reinforced;
        assert!(memory.is_visible_at(ValidTime::new(15)));

        memory.status = MemoryStatus::Candidate;
        assert!(!memory.is_visible_at(ValidTime::new(15)));

        memory.status = MemoryStatus::Superseded;
        assert!(!memory.is_visible_at(ValidTime::new(15)));

        memory.status = MemoryStatus::Contradicted;
        assert!(!memory.is_visible_at(ValidTime::new(15)));

        memory.status = MemoryStatus::Archived;
        assert!(!memory.is_visible_at(ValidTime::new(15)));
    }
}
