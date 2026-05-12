use rg_agent_memory::{
    AgentMemoryKind, AgentMemoryService, ConsolidateMemory, CorrectionMemory, EpisodicMemory,
    GoalMemory, MemoryPermissions, MemoryQuery, MemoryRecord, MemoryRetrievalMode, PlanMemory,
    PreferenceMemory, ProceduralMemory, ReflectionMemory, RelationshipMemory, SemanticMemory,
    SupersedeMemory, WorldStateMemory, WriteMemory,
};
use rg_core::{
    AgentId, Confidence, EntityId, MemoryId, MemoryStatus, SourceId, TimeInterval, TxTime,
    ValidTime,
};

#[test]
fn typed_memories_survive_replay_and_preserve_provenance() {
    let mut service = AgentMemoryService::new(TxTime::new(100));

    service
        .write_memory(write(
            "episode-1",
            AgentMemoryKind::Episodic,
            "During the onboarding session, the user connected Project Atlas to the API gateway.",
            MemoryStatus::Candidate,
        ))
        .expect("episode candidate is written");

    service
        .consolidate_memory(ConsolidateMemory {
            new_id: MemoryId::new("semantic-1"),
            agent_id: agent(),
            memory_type: AgentMemoryKind::Semantic,
            content: "Project Atlas depends on the API gateway.".to_owned(),
            valid_time: interval(100, None),
            confidence: Confidence::new(0.91).expect("confidence"),
            source_ids: vec![source()],
            related_entities: vec![entity("project-atlas"), entity("api-gateway")],
            source_memory_ids: vec![MemoryId::new("episode-1")],
            permissions: MemoryPermissions::private(agent()),
        })
        .expect("candidate is consolidated into semantic memory");

    let replayed = AgentMemoryService::replay(service.journal()).expect("journal replays");
    let semantic = replayed
        .memory(&MemoryId::new("semantic-1"))
        .expect("semantic memory exists after replay");

    assert_eq!(semantic.memory_type, AgentMemoryKind::Semantic);
    assert_eq!(semantic.lifecycle, MemoryStatus::Active);
    assert_eq!(semantic.compressed_from, vec![MemoryId::new("episode-1")]);
    assert_eq!(semantic.source_ids, vec![source()]);
    assert_eq!(
        semantic.to_core_memory().memory_type,
        rg_core::MemoryType::Semantic
    );
}

#[test]
fn supersede_memory_revises_beliefs_without_deleting_history() {
    let mut service = AgentMemoryService::new(TxTime::new(200));

    service
        .write_memory(write(
            "memory-manual",
            AgentMemoryKind::Preference,
            "The user prefers manual API deployments.",
            MemoryStatus::Active,
        ))
        .expect("old preference is written");

    service
        .supersede_memory(SupersedeMemory {
            old_id: MemoryId::new("memory-manual"),
            new_memory: write(
                "memory-blue-green",
                AgentMemoryKind::Correction,
                "The user corrected this: use blue-green API deployments.",
                MemoryStatus::Active,
            ),
            reason: "User explicitly corrected the deployment preference.".to_owned(),
        })
        .expect("belief is superseded");

    let old = service
        .memory(&MemoryId::new("memory-manual"))
        .expect("old belief retained");
    let new = service
        .memory(&MemoryId::new("memory-blue-green"))
        .expect("new belief retained");

    assert_eq!(old.lifecycle, MemoryStatus::Superseded);
    assert_eq!(new.supersedes, vec![MemoryId::new("memory-manual")]);

    let current = service.retrieve_memory(MemoryQuery {
        agent_id: agent(),
        query: "What deployment preference is current?".to_owned(),
        valid_at: Some(ValidTime::new(250)),
        related_entities: vec![entity("api-gateway")],
        include_history: false,
        mode: MemoryRetrievalMode::GraphTemporal,
        limit: Some(5),
    });

    assert_eq!(
        current.memories[0].record.id,
        MemoryId::new("memory-blue-green")
    );
    assert!(!current
        .memories
        .iter()
        .any(|item| { item.record.id == MemoryId::new("memory-manual") && item.current_truth }));

    let explanation = service
        .explain_memory(&MemoryId::new("memory-manual"))
        .expect("old memory can be explained");
    assert!(!explanation.current_truth);
    assert!(explanation
        .reason
        .contains("superseded by memory-blue-green"));
}

#[test]
fn contradicted_memory_is_explainable_but_not_current_truth() {
    let mut service = AgentMemoryService::new(TxTime::new(300));

    let mut wrong = write(
        "memory-wrong-owner",
        AgentMemoryKind::Relationship,
        "Company A owns Company B.",
        MemoryStatus::Active,
    );
    wrong.related_entities = vec![entity("company-a"), entity("company-b")];
    service.write_memory(wrong).expect("wrong memory written");

    let mut correction = write(
        "memory-owner-correction",
        AgentMemoryKind::Correction,
        "Company A does not own Company B; it is only a supplier.",
        MemoryStatus::Active,
    );
    correction.contradicts = vec![MemoryId::new("memory-wrong-owner")];
    correction.related_entities = vec![entity("company-a"), entity("company-b")];
    service
        .write_memory(correction)
        .expect("contradiction is written");

    let contradicted = service
        .memory(&MemoryId::new("memory-wrong-owner"))
        .expect("contradicted memory retained");
    assert_eq!(contradicted.lifecycle, MemoryStatus::Contradicted);

    let current = service.retrieve_memory(MemoryQuery {
        agent_id: agent(),
        query: "Who owns Company B?".to_owned(),
        valid_at: Some(ValidTime::new(350)),
        related_entities: vec![entity("company-b")],
        include_history: false,
        mode: MemoryRetrievalMode::GraphTemporal,
        limit: Some(10),
    });

    assert!(!current
        .memories
        .iter()
        .any(|item| item.record.id == MemoryId::new("memory-wrong-owner")));
    assert!(service
        .explain_memory(&MemoryId::new("memory-wrong-owner"))
        .expect("explanation exists")
        .reason
        .contains("contradicted by memory-owner-correction"));
}

#[test]
fn permissions_filter_memory_retrieval() {
    let mut service = AgentMemoryService::new(TxTime::new(400));
    service
        .write_memory(write(
            "private-memory",
            AgentMemoryKind::Goal,
            "The agent is pursuing the private launch-risk review.",
            MemoryStatus::Active,
        ))
        .expect("private memory written");

    let blocked = service.retrieve_memory(MemoryQuery {
        agent_id: AgentId::new("other-agent"),
        query: "launch risk".to_owned(),
        valid_at: None,
        related_entities: Vec::new(),
        include_history: false,
        mode: MemoryRetrievalMode::GraphTemporal,
        limit: None,
    });

    assert!(blocked.memories.is_empty());
}

#[test]
fn graph_temporal_retrieval_beats_transcript_vector_on_long_term_temporal_memory() {
    let mut service = AgentMemoryService::new(TxTime::new(500));

    service
        .write_memory(write(
            "memory-manual",
            AgentMemoryKind::Preference,
            "Manual deployment is the API deployment approach.",
            MemoryStatus::Active,
        ))
        .expect("old memory written");

    service
        .supersede_memory(SupersedeMemory {
            old_id: MemoryId::new("memory-manual"),
            new_memory: write(
                "memory-blue-green",
                AgentMemoryKind::Correction,
                "After the outage, the current API rollout strategy is blue-green.",
                MemoryStatus::Active,
            ),
            reason: "Later correction changed the preference.".to_owned(),
        })
        .expect("memory superseded");

    let temporal = service.retrieve_memory(MemoryQuery {
        agent_id: agent(),
        query: "What is the current API deployment approach?".to_owned(),
        valid_at: Some(ValidTime::new(550)),
        related_entities: vec![entity("api-gateway")],
        include_history: false,
        mode: MemoryRetrievalMode::GraphTemporal,
        limit: Some(1),
    });

    let transcript = service.retrieve_memory(MemoryQuery {
        agent_id: agent(),
        query: "What is the current API deployment approach?".to_owned(),
        valid_at: Some(ValidTime::new(550)),
        related_entities: vec![entity("api-gateway")],
        include_history: true,
        mode: MemoryRetrievalMode::TranscriptVector,
        limit: Some(1),
    });

    assert_eq!(
        temporal.memories[0].record.id,
        MemoryId::new("memory-blue-green")
    );
    assert_eq!(
        transcript.memories[0].record.id,
        MemoryId::new("memory-manual")
    );
    assert!(temporal.quality_score > transcript.quality_score);
}

#[test]
fn non_transcript_memory_type_wrappers_are_available() {
    let record = MemoryRecord::from_write(write(
        "memory-type",
        AgentMemoryKind::WorldState,
        "The modeled environment has one active incident.",
        MemoryStatus::Active,
    ));

    let _episodic = EpisodicMemory(record.clone());
    let _semantic = SemanticMemory(record.clone());
    let _procedural = ProceduralMemory(record.clone());
    let _preference = PreferenceMemory(record.clone());
    let _goal = GoalMemory(record.clone());
    let _plan = PlanMemory(record.clone());
    let _reflection = ReflectionMemory(record.clone());
    let _correction = CorrectionMemory(record.clone());
    let _relationship = RelationshipMemory(record.clone());
    let _world_state = WorldStateMemory(record);
}

fn write(
    id: &str,
    memory_type: AgentMemoryKind,
    content: &str,
    lifecycle: MemoryStatus,
) -> WriteMemory {
    WriteMemory {
        id: MemoryId::new(id),
        agent_id: agent(),
        memory_type,
        content: content.to_owned(),
        valid_time: interval(100, None),
        confidence: Confidence::new(0.88).expect("confidence"),
        source_ids: vec![source()],
        related_entities: vec![entity("api-gateway")],
        supersedes: Vec::new(),
        contradicts: Vec::new(),
        lifecycle,
        permissions: MemoryPermissions::private(agent()),
    }
}

fn interval(start: i64, end: Option<i64>) -> TimeInterval<ValidTime> {
    TimeInterval::new(ValidTime::new(start), end.map(ValidTime::new)).expect("valid interval")
}

fn agent() -> AgentId {
    AgentId::new("agent-main")
}

fn source() -> SourceId {
    SourceId::new("source-memory")
}

fn entity(id: &str) -> EntityId {
    EntityId::new(id)
}
