use reality_gym::{
    Action, ActionKind, AgentEnvironment, AgentPolicy, EnvironmentConfig, EvaluationOracle,
    GymTaskKind, MemoryWrite, ObservationKind, RewardKind, ScenarioKind, WorldUpdateKind,
};
use rg_core::{AgentId, AssertionId, MemoryId, SourceId};
use rg_worldgen::WorldSchema;

#[test]
fn environment_reset_returns_noisy_observation_without_revealing_hidden_state() {
    let world = WorldSchema::controlled(101)
        .with_agent_tasks(8)
        .generate()
        .expect("world");
    let mut environment = AgentEnvironment::new(EnvironmentConfig::single_agent(
        world,
        ScenarioKind::InvestigateFraud,
        AgentId::new("agent-a"),
    ));

    let observation = environment.reset().expect("reset");

    assert_eq!(observation.step_index, 0);
    assert_eq!(observation.kind, ObservationKind::NoisyEvidence);
    assert!(!observation.visible_assertion_ids.is_empty());
    assert!(!observation.visible_source_ids.is_empty());
    assert!(observation.hidden_truth_assertion_ids.is_empty());
    assert!(observation.prompt.contains("investigate"));
    assert_eq!(environment.current_step(), 0);
}

#[test]
fn observe_retrieve_reason_act_write_memory_world_updates_evaluate_loop_scores_grounded_behavior() {
    let world = WorldSchema::controlled(102)
        .with_agent_tasks(8)
        .generate()
        .expect("world");
    let true_assertion = world.hidden_true_state.assertions[0].id.clone();
    let source = world.documents[0].id.clone();
    let mut environment = AgentEnvironment::new(EnvironmentConfig::single_agent(
        world,
        ScenarioKind::ManageCompany,
        AgentId::new("agent-a"),
    ));
    environment.reset().expect("reset");

    let memory = MemoryWrite {
        id: MemoryId::new("memory-1"),
        agent_id: AgentId::new("agent-a"),
        content: "Verified the source-backed employment assertion.".to_string(),
        source_ids: vec![source.clone()],
        assertion_ids: vec![true_assertion.clone()],
    };
    let action = Action {
        agent_id: AgentId::new("agent-a"),
        kind: ActionKind::AnswerQuestion,
        description: "Answer using verified evidence.".to_string(),
        cited_assertion_ids: vec![true_assertion.clone()],
        cited_source_ids: vec![source],
        memory_write: Some(memory.clone()),
    };

    let transition = environment.step(action).expect("step");

    assert_eq!(transition.observation.kind, ObservationKind::WorldUpdate);
    assert!(transition
        .world_updates
        .iter()
        .any(|update| update.kind == WorldUpdateKind::MemoryCommitted));
    assert!(transition
        .world_updates
        .iter()
        .any(|update| update.kind == WorldUpdateKind::DelayedConsequenceScheduled));
    assert!(transition
        .reward
        .components
        .iter()
        .any(|component| component.kind == RewardKind::CorrectAnswer && component.value > 0.0));
    assert!(transition.reward.total > 0.5);
    assert_eq!(
        environment.memory_writes_for(&AgentId::new("agent-a")),
        &[memory]
    );
}

#[test]
fn single_and_multi_agent_tasks_are_supported_with_agent_scoped_memory() {
    let world = WorldSchema::controlled(103).generate().expect("world");
    let mut environment = AgentEnvironment::new(EnvironmentConfig::multi_agent(
        world,
        ScenarioKind::CoordinateResearchProject,
        vec![AgentId::new("researcher"), AgentId::new("reviewer")],
    ));
    environment.reset().expect("reset");

    let first = environment
        .step(Action::memory_only(
            AgentId::new("researcher"),
            MemoryWrite {
                id: MemoryId::new("memory-researcher"),
                agent_id: AgentId::new("researcher"),
                content: "Need reviewer confirmation.".to_string(),
                source_ids: vec![SourceId::new("source-0000")],
                assertion_ids: vec![AssertionId::new("truth-worked-at-0000")],
            },
        ))
        .expect("first step");
    let second = environment
        .step(Action::memory_only(
            AgentId::new("reviewer"),
            MemoryWrite {
                id: MemoryId::new("memory-reviewer"),
                agent_id: AgentId::new("reviewer"),
                content: "Reviewer confirmed evidence.".to_string(),
                source_ids: vec![SourceId::new("source-0001")],
                assertion_ids: vec![AssertionId::new("truth-worked-at-0001")],
            },
        ))
        .expect("second step");

    assert_eq!(first.actor, AgentId::new("researcher"));
    assert_eq!(second.actor, AgentId::new("reviewer"));
    assert_eq!(
        environment
            .memory_writes_for(&AgentId::new("researcher"))
            .len(),
        1
    );
    assert_eq!(
        environment
            .memory_writes_for(&AgentId::new("reviewer"))
            .len(),
        1
    );
    assert_eq!(environment.agents().len(), 2);
}

#[test]
fn adversarial_source_injection_and_noisy_evidence_penalize_ungrounded_actions() {
    let world = WorldSchema::controlled(104)
        .with_contradictions(2)
        .generate()
        .expect("world");
    let false_assertion = world.noisy_observed_state.contradictions[0]
        .observed_false_assertion
        .clone();
    let source = world.noisy_observed_state.contradictions[0]
        .observed_assertion
        .source_ids[0]
        .clone();
    let mut environment = AgentEnvironment::new(
        EnvironmentConfig::single_agent(
            world,
            ScenarioKind::TrackGeopoliticalCrisis,
            AgentId::new("agent-a"),
        )
        .with_adversarial_source_injection(true),
    );
    environment.reset().expect("reset");

    let transition = environment
        .step(Action {
            agent_id: AgentId::new("agent-a"),
            kind: ActionKind::VerifyClaim,
            description: "Trust the noisy claim.".to_string(),
            cited_assertion_ids: vec![false_assertion],
            cited_source_ids: vec![source],
            memory_write: None,
        })
        .expect("step");

    assert!(transition
        .world_updates
        .iter()
        .any(|update| update.kind == WorldUpdateKind::AdversarialSourceInjected));
    assert!(transition
        .reward
        .components
        .iter()
        .any(
            |component| component.kind == RewardKind::TrustedFalseEvidence && component.value < 0.0
        ));
    assert!(transition.reward.total < 0.0);
}

#[test]
fn hidden_state_and_delayed_consequences_are_revealed_only_through_evaluation_oracle() {
    let world = WorldSchema::controlled(105).generate().expect("world");
    let true_assertion = world.hidden_true_state.assertions[0].id.clone();
    let mut environment = AgentEnvironment::new(EnvironmentConfig::single_agent(
        world,
        ScenarioKind::RunCustomerSuccessAccount,
        AgentId::new("agent-a"),
    ));
    environment.reset().expect("reset");

    let delayed = environment
        .step(Action {
            agent_id: AgentId::new("agent-a"),
            kind: ActionKind::PlanAction,
            description: "Create follow-up plan based on true relationship.".to_string(),
            cited_assertion_ids: vec![true_assertion.clone()],
            cited_source_ids: vec![SourceId::new("source-0000")],
            memory_write: None,
        })
        .expect("step");
    assert!(delayed
        .world_updates
        .iter()
        .any(|update| update.apply_after_steps > 0));

    let observation = environment
        .observe(&AgentId::new("agent-a"))
        .expect("observe");
    assert!(observation.hidden_truth_assertion_ids.is_empty());

    let oracle = EvaluationOracle::from_environment(&environment);
    assert!(oracle.hidden_truth_contains(&true_assertion));
    assert_eq!(oracle.pending_delayed_consequences().len(), 1);
}

#[test]
fn gym_scenario_catalog_covers_frontier_lab_agent_workloads() {
    let scenarios = ScenarioKind::all();
    let task_kinds = GymTaskKind::all();
    let policy = AgentPolicy::memory_first();

    assert!(scenarios.contains(&ScenarioKind::ManageCompany));
    assert!(scenarios.contains(&ScenarioKind::NegotiateContract));
    assert!(scenarios.contains(&ScenarioKind::InvestigateFraud));
    assert!(scenarios.contains(&ScenarioKind::CoordinateResearchProject));
    assert!(scenarios.contains(&ScenarioKind::DebugLongRunningCodebase));
    assert!(scenarios.contains(&ScenarioKind::RunCustomerSuccessAccount));
    assert!(scenarios.contains(&ScenarioKind::TrackGeopoliticalCrisis));
    assert!(scenarios.contains(&ScenarioKind::MaintainPersonalAssistantMemory));
    assert_eq!(task_kinds.len(), 8);
    assert!(policy.requires_memory_retrieval);
    assert!(policy.requires_memory_write);
}
