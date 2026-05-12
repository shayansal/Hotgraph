use std::time::Duration;

use rg_core::{
    AgentId, Confidence, EventId, PredicateId, SourceId, TenantId, TimeInterval, TxTime, ValidTime,
};
use rg_kernel::{
    active_during, current_belief, dispute_atom, known_at, retract_atom, revise_belief,
    supersede_atom, visible_at, AiUsage, AtomId, AtomImpactReport, AtomPattern, BeliefPolicy,
    BeliefState, BitemporalQuestion, BitemporalTruth, CausalAtom, ClaimPattern, ClaimType,
    ConflictSet, ConflictStatus, ConflictType, DependencyEdge, DependencyGraph, DependencyNode,
    DependencyType, EntityRef, EvidenceSpan, ExtractionTrace, ImpactCone, IncrementalComputation,
    IncrementalEventKind, KernelError, KernelEvent, KernelQuery, MaintainedViewName,
    ModelContextCompiler, ModelContextRequest, NativeExecutionStrategy, NativeRealityQuery,
    PermissionLabel, PhysicalGraphStore, PhysicalLayoutKind, RealityAtom, RealityAtomId,
    RealityKernel, RealityOperator, RealityQuery, RealityQueryVm, RealityReturnField,
    RecommendedActionKind, RevisionReason, RiskLevel, SelfRevisionCursor, SelfRevisionEngine,
    SelfRevisionJob, SelfRevisionPolicy, SelfRevisionReviewStatus, SelfRevisionSuggestionKind,
    SourceRef, SupportSet, TaintLabel, TransactionTime, TruthMaintenance, ValueOrEntity,
};

#[test]
fn bitemporal_visibility_requires_valid_time_transaction_time_and_provenance() {
    let atom = worked_at_atom("atom-worked", 2021, Some(2025), 100, None);

    assert!(atom.is_visible_at(ValidTime::new(2024), TxTime::new(100)));
    assert!(!atom.is_visible_at(ValidTime::new(2020), TxTime::new(100)));
    assert!(!atom.is_visible_at(ValidTime::new(2024), TxTime::new(99)));

    let missing_source = RealityAtom::builder(
        AtomId::new("bad-atom"),
        EntityRef::new("person-a"),
        PredicateId::new("WORKED_AT"),
        ValueOrEntity::entity("company-b"),
    )
    .valid_time(TimeInterval::new(ValidTime::new(2021), None).expect("valid"))
    .transaction_time(TimeInterval::new(TxTime::new(100), None).expect("tx"))
    .confidence(Confidence::new(0.9).expect("confidence"));

    assert_eq!(missing_source.build(), Err(KernelError::MissingProvenance));
}

#[test]
fn week_one_kernel_invariants_are_enforced_by_builder() {
    let reality_atom_id = RealityAtomId::new("atom-distinct");
    let atom_id: AtomId = reality_atom_id.clone();
    assert_eq!(atom_id.as_str(), "atom-distinct");
    assert_eq!(TransactionTime::new(42).as_i64(), 42);

    let derived_without_dependencies = RealityAtom::builder(
        AtomId::new("derived-missing-dependencies"),
        EntityRef::new("person-a"),
        PredicateId::new("CURRENT_EMPLOYMENT_BELIEF"),
        ValueOrEntity::entity("company-b"),
    )
    .valid_time(TimeInterval::new(ValidTime::new(2024), None).expect("valid"))
    .transaction_time(TimeInterval::new(TxTime::new(200), None).expect("tx"))
    .claim_type(ClaimType::Derived)
    .belief_state(BeliefState::Accepted)
    .confidence(Confidence::new(0.84).expect("confidence"))
    .source_ref(SourceRef::new(SourceId::new("source-derived")))
    .evidence_span(EvidenceSpan::new(
        SourceId::new("source-derived"),
        0,
        24,
        "derived from source atom",
    ))
    .tenant_id(TenantId::new("tenant-lab"))
    .permissions(PermissionLabel::Internal)
    .taint(TaintLabel::Trusted)
    .ai_usage(AiUsage::SafeForPlanning { caveat: None });

    assert_eq!(
        derived_without_dependencies.build(),
        Err(KernelError::MissingDependencies)
    );

    let memory_without_trace = RealityAtom::builder(
        AtomId::new("memory-missing-trace"),
        EntityRef::new("agent-research"),
        PredicateId::new("HAS_MEMORY"),
        ValueOrEntity::text("remembered without trace"),
    )
    .valid_time(TimeInterval::new(ValidTime::new(2024), None).expect("valid"))
    .transaction_time(TimeInterval::new(TxTime::new(200), None).expect("tx"))
    .claim_type(ClaimType::AgentMemory)
    .belief_state(BeliefState::Accepted)
    .confidence(Confidence::new(0.75).expect("confidence"))
    .source_ref(SourceRef::new(SourceId::new("source-memory")))
    .evidence_span(EvidenceSpan::new(
        SourceId::new("source-memory"),
        0,
        24,
        "memory write accepted",
    ))
    .tenant_id(TenantId::new("tenant-lab"))
    .agent_scope(AgentId::new("agent-research"))
    .permissions(PermissionLabel::Internal)
    .taint(TaintLabel::Trusted)
    .ai_usage(AiUsage::SafeForPlanning { caveat: None });

    assert_eq!(
        memory_without_trace.build(),
        Err(KernelError::MissingMemoryTrace)
    );
}

#[test]
fn week_two_named_bitemporal_and_belief_apis_are_available() {
    let atom = worked_at_atom(
        "atom-week-two",
        2021,
        Some(2025),
        100,
        Some("source-week-two"),
    );
    let active_interval =
        TimeInterval::new(ValidTime::new(2022), Some(ValidTime::new(2024))).expect("valid");

    assert!(visible_at(
        &atom,
        ValidTime::new(2023),
        TransactionTime::new(150)
    ));
    assert!(known_at(&atom, TransactionTime::new(150)));
    assert!(active_during(&atom, &active_interval));

    let view = current_belief(
        std::slice::from_ref(&atom),
        ValidTime::new(2023),
        TransactionTime::new(150),
        BeliefPolicy::IncludeDisputed,
    );
    assert_eq!(view.accepted_atoms, vec![atom.id.clone()]);
    assert!(view.disputed_atoms.is_empty());

    let reason = RevisionReason::new(TransactionTime::new(250), "newer source corrected range");
    assert_eq!(
        revise_belief(
            atom.id.clone(),
            AtomId::new("atom-week-two-replacement"),
            reason.clone()
        )
        .next,
        BeliefState::Superseded
    );
    assert_eq!(
        supersede_atom(
            atom.id.clone(),
            AtomId::new("atom-week-two-replacement"),
            reason.clone()
        )
        .known_at,
        TxTime::new(250)
    );
    assert_eq!(
        dispute_atom(atom.id.clone(), reason.clone()).next,
        BeliefState::Disputed
    );
    assert_eq!(retract_atom(atom.id, reason).next, BeliefState::Retracted);
}

#[test]
fn query_vm_answers_what_is_true_now_as_native_bitemporal_truth() {
    let mut kernel = RealityKernel::new();
    kernel.insert_atom(worked_at_atom(
        "atom-current",
        2021,
        None,
        100,
        Some("source-current"),
    ));
    kernel.insert_atom(worked_at_atom(
        "atom-future",
        2026,
        None,
        100,
        Some("source-future"),
    ));

    let vm = RealityQueryVm::new(&kernel);
    let now = BitemporalTruth::new(ValidTime::new(2024), TxTime::new(250));
    let result = vm.execute(RealityQuery::WhatIsTrueNow {
        entity: EntityRef::new("person-a"),
        now,
        ai_facing: true,
    });

    assert_eq!(result.question, BitemporalQuestion::WhatIsTrueNow);
    assert_eq!(result.truth, Some(now));
    assert_eq!(result.atoms.len(), 1);
    assert_eq!(result.atoms[0].id.as_str(), "atom-current");
}

#[test]
fn query_vm_answers_what_was_true_at_world_time() {
    let mut kernel = RealityKernel::new();
    kernel.insert_atom(worked_at_atom(
        "atom-true-in-2024",
        2021,
        Some(2025),
        100,
        Some("source-2024"),
    ));
    kernel.insert_atom(worked_at_atom(
        "atom-true-later",
        2025,
        None,
        100,
        Some("source-later"),
    ));

    let vm = RealityQueryVm::new(&kernel);
    let result = vm.execute(RealityQuery::WhatWasTrueAt {
        entity: EntityRef::new("person-a"),
        valid_at: ValidTime::new(2024),
        known_at: TxTime::new(300),
        ai_facing: true,
    });

    assert_eq!(result.question, BitemporalQuestion::WhatWasTrueAt);
    assert_eq!(
        result.truth,
        Some(BitemporalTruth::new(ValidTime::new(2024), TxTime::new(300)))
    );
    assert_eq!(result.atoms.len(), 1);
    assert_eq!(result.atoms[0].id.as_str(), "atom-true-in-2024");
}

#[test]
fn query_vm_answers_what_we_believed_at_transaction_time() {
    let mut kernel = RealityKernel::new();
    kernel.insert_atom(worked_at_atom(
        "atom-original-belief",
        2021,
        Some(2025),
        100,
        Some("source-original"),
    ));
    kernel.insert_atom(
        worked_at_atom(
            "atom-revised-belief",
            2021,
            Some(2024),
            300,
            Some("source-revised"),
        )
        .superseding(vec![AtomId::new("atom-original-belief")]),
    );

    let vm = RealityQueryVm::new(&kernel);
    let result = vm.execute(RealityQuery::WhatDidWeBelieveAt {
        entity: EntityRef::new("person-a"),
        valid_at: ValidTime::new(2024),
        believed_at: TxTime::new(150),
        ai_facing: true,
    });

    assert_eq!(result.question, BitemporalQuestion::WhatDidWeBelieveAt);
    assert_eq!(
        result.truth,
        Some(BitemporalTruth::new(ValidTime::new(2024), TxTime::new(150)))
    );
    assert_eq!(result.atoms.len(), 1);
    assert_eq!(result.atoms[0].id.as_str(), "atom-original-belief");
    assert_eq!(result.atoms[0].belief_state, BeliefState::Accepted);
    assert!(result.unsupported_conclusions.is_empty());
}

#[test]
fn query_vm_reports_when_belief_changed() {
    let mut kernel = RealityKernel::new();
    kernel.insert_atom(worked_at_atom(
        "atom-original-belief",
        2021,
        Some(2025),
        100,
        Some("source-original"),
    ));
    kernel.insert_atom(
        worked_at_atom(
            "atom-revised-belief",
            2021,
            Some(2024),
            300,
            Some("source-revised"),
        )
        .superseding(vec![AtomId::new("atom-original-belief")]),
    );

    let vm = RealityQueryVm::new(&kernel);
    let result = vm.execute(RealityQuery::WhenDidBeliefChange {
        atom_id: AtomId::new("atom-original-belief"),
    });

    assert_eq!(result.question, BitemporalQuestion::WhenDidBeliefChange);
    assert_eq!(result.truth, None);
    assert_eq!(result.belief_changes.len(), 1);
    assert_eq!(result.belief_changes[0].known_at, TxTime::new(300));
    assert_eq!(result.belief_changes[0].previous, BeliefState::Accepted);
    assert_eq!(result.belief_changes[0].next, BeliefState::Superseded);
    assert!(result.belief_changes[0]
        .reason
        .contains("atom-revised-belief"));
}

#[test]
fn belief_lifecycle_states_are_not_binary_truth() {
    assert!(BeliefState::Accepted.ai_supported());
    assert!(BeliefState::Disputed.ai_supported());

    for state in [
        BeliefState::Candidate,
        BeliefState::Superseded,
        BeliefState::Retracted,
        BeliefState::Refuted,
        BeliefState::Simulated,
        BeliefState::Unknown,
    ] {
        assert!(
            !state.ai_supported(),
            "{state:?} is not an AI-facing conclusion"
        );
    }

    assert!(BeliefState::Refuted.is_rejected_or_retired());
    assert!(BeliefState::Simulated.is_simulation());
    assert!(BeliefState::Unknown.is_uncertain());
}

#[test]
fn acquisition_conflict_preserves_timeline_sources_and_revision_history() {
    let mut kernel = RealityKernel::new();
    kernel.insert_atom(acquisition_atom(AcquisitionClaim {
        id: "claim-acquired-march",
        predicate: "ACQUIRED_ON",
        quote: "Company X acquired Company Y on March 1.",
        valid_at: 20240301,
        tx_at: 100,
        belief_state: BeliefState::Accepted,
        source: "source-a",
        confidence: 0.72,
    }));
    kernel.insert_atom(acquisition_atom(AcquisitionClaim {
        id: "claim-announced-march",
        predicate: "ANNOUNCED_ON",
        quote: "The acquisition was announced on March 1 and closed June 30.",
        valid_at: 20240301,
        tx_at: 120,
        belief_state: BeliefState::Disputed,
        source: "source-b",
        confidence: 0.91,
    }));
    kernel
        .revise_belief(
            &AtomId::new("claim-acquired-march"),
            BeliefState::Disputed,
            TxTime::new(120),
            "source-b distinguishes announcement date from closing date",
        )
        .expect("claim can be disputed");
    kernel.add_conflict(ConflictSet::new(
        "conflict-acquisition-date",
        vec![
            AtomId::new("claim-acquired-march"),
            AtomId::new("claim-announced-march"),
        ],
        ConflictType::SourceDisagreement,
        ConflictStatus::Unresolved,
        "March 1 may be announcement date, not closing date.",
    ));

    let march_context = RealityQueryVm::new(&kernel).execute(RealityQuery::WhatDidWeBelieveAt {
        entity: EntityRef::new("company-x"),
        valid_at: ValidTime::new(20240301),
        believed_at: TxTime::new(130),
        ai_facing: true,
    });
    let march_atom_ids = march_context
        .atoms
        .iter()
        .map(|atom| atom.id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        march_atom_ids,
        vec!["claim-acquired-march", "claim-announced-march"]
    );
    assert_eq!(march_context.conflicts.len(), 1);
    assert!(march_context
        .atoms
        .iter()
        .any(|atom| atom.source_refs[0].source_id.as_str() == "source-a"));
    assert!(march_context
        .atoms
        .iter()
        .any(|atom| atom.source_refs[0].source_id.as_str() == "source-b"));

    kernel.insert_atom(acquisition_atom(AcquisitionClaim {
        id: "claim-regulators-blocked",
        predicate: "BLOCKED_BY_REGULATORS",
        quote: "Regulators later blocked the acquisition.",
        valid_at: 20240630,
        tx_at: 180,
        belief_state: BeliefState::Accepted,
        source: "source-c",
        confidence: 0.96,
    }));
    kernel
        .revise_belief(
            &AtomId::new("claim-acquired-march"),
            BeliefState::Refuted,
            TxTime::new(180),
            "regulatory blocking evidence refuted the completed-acquisition claim",
        )
        .expect("claim can be revised");

    assert_eq!(
        kernel.belief_at(&AtomId::new("claim-acquired-march"), TxTime::new(110)),
        Some(BeliefState::Accepted)
    );
    assert_eq!(
        kernel.belief_at(&AtomId::new("claim-acquired-march"), TxTime::new(130)),
        Some(BeliefState::Disputed)
    );
    assert_eq!(
        kernel.belief_at(&AtomId::new("claim-acquired-march"), TxTime::new(200)),
        Some(BeliefState::Refuted)
    );

    let current_march_context =
        RealityQueryVm::new(&kernel).execute(RealityQuery::WhatDidWeBelieveAt {
            entity: EntityRef::new("company-x"),
            valid_at: ValidTime::new(20240301),
            believed_at: TxTime::new(200),
            ai_facing: true,
        });

    assert!(!current_march_context
        .atoms
        .iter()
        .any(|atom| atom.id.as_str() == "claim-acquired-march"));
    assert!(current_march_context
        .unsupported_conclusions
        .iter()
        .any(|atom| atom.id.as_str() == "claim-acquired-march"
            && atom.belief_state == BeliefState::Refuted));
    assert!(current_march_context
        .atoms
        .iter()
        .any(|atom| atom.id.as_str() == "claim-announced-march"));
}

#[test]
fn historical_belief_distinguishes_what_was_known_then_from_current_revision() {
    let mut kernel = RealityKernel::new();
    kernel.insert_atom(worked_at_atom(
        "atom-2024-end",
        2021,
        Some(2025),
        100,
        Some("source-1"),
    ));
    kernel.insert_atom(
        worked_at_atom("atom-2023-end", 2021, Some(2024), 200, Some("source-4"))
            .with_belief_state(BeliefState::Accepted)
            .with_confidence(Confidence::new(0.94).expect("confidence"))
            .superseding(vec![AtomId::new("atom-2024-end")]),
    );

    let historical = kernel.belief_at(&AtomId::new("atom-2024-end"), TxTime::new(150));
    let current = kernel.belief_at(&AtomId::new("atom-2024-end"), TxTime::new(250));

    assert_eq!(historical, Some(BeliefState::Accepted));
    assert_eq!(current, Some(BeliefState::Superseded));

    let accepted_then = kernel.entity_state(
        EntityRef::new("person-a"),
        ValidTime::new(2024),
        TxTime::new(150),
    );
    let accepted_now = kernel.entity_state(
        EntityRef::new("person-a"),
        ValidTime::new(2024),
        TxTime::new(250),
    );

    assert_eq!(accepted_then.accepted_atoms.len(), 1);
    assert_eq!(accepted_then.accepted_atoms[0].id.as_str(), "atom-2024-end");
    assert_eq!(accepted_now.accepted_atoms.len(), 0);
    assert_eq!(
        accepted_now.superseded_atoms[0].id.as_str(),
        "atom-2024-end"
    );
}

#[test]
fn supersession_preserves_history_and_records_revision_without_deleting_atom() {
    let mut kernel = RealityKernel::new();
    kernel.insert_atom(worked_at_atom(
        "old-atom",
        2021,
        Some(2025),
        100,
        Some("source-1"),
    ));
    kernel.insert_atom(
        worked_at_atom("new-atom", 2021, Some(2024), 300, Some("source-2"))
            .superseding(vec![AtomId::new("old-atom")]),
    );

    assert!(kernel.atom(&AtomId::new("old-atom")).is_some());
    assert!(kernel.atom(&AtomId::new("new-atom")).is_some());

    let revisions = kernel.belief_revisions(&AtomId::new("old-atom"));
    assert_eq!(revisions.len(), 1);
    assert_eq!(revisions[0].previous, BeliefState::Accepted);
    assert_eq!(revisions[0].next, BeliefState::Superseded);
    assert_eq!(revisions[0].known_at, TxTime::new(300));
}

#[test]
fn contradictions_are_returned_as_conflict_sets_not_silently_collapsed() {
    let mut kernel = RealityKernel::new();
    kernel.insert_atom(worked_at_atom(
        "atom-company-b",
        2021,
        Some(2025),
        100,
        Some("source-1"),
    ));
    kernel.insert_atom(
        RealityAtom::builder(
            AtomId::new("atom-company-c"),
            EntityRef::new("person-a"),
            PredicateId::new("WORKED_AT"),
            ValueOrEntity::entity("company-c"),
        )
        .valid_time(
            TimeInterval::new(ValidTime::new(2022), Some(ValidTime::new(2024))).expect("valid"),
        )
        .transaction_time(TimeInterval::new(TxTime::new(120), None).expect("tx"))
        .confidence(Confidence::new(0.88).expect("confidence"))
        .source_ref(SourceRef::new(SourceId::new("source-4")))
        .evidence_span(EvidenceSpan::new(
            SourceId::new("source-4"),
            8,
            40,
            "source_4 claims A left in 2023",
        ))
        .belief_state(BeliefState::Disputed)
        .build()
        .expect("atom"),
    );
    kernel.add_conflict(ConflictSet::new(
        "conflict-employment-date",
        vec![AtomId::new("atom-company-b"), AtomId::new("atom-company-c")],
        ConflictType::ValidTimeOverlap,
        ConflictStatus::Unresolved,
        "source_4 claims A left in 2023",
    ));

    let state = kernel.entity_state(
        EntityRef::new("person-a"),
        ValidTime::new(2023),
        TxTime::new(200),
    );

    assert_eq!(state.conflicts.len(), 1);
    assert_eq!(state.conflicts[0].atom_ids.len(), 2);
    assert!(state.conflicts[0].explanation.contains("left in 2023"));
}

#[test]
fn dependency_invalidation_marks_downstream_atoms_answers_and_simulations() {
    let mut graph = DependencyGraph::new();
    graph.add_dependency(
        DependencyNode::Atom(AtomId::new("source-atom")),
        DependencyNode::Atom(AtomId::new("derived-plan")),
        "plan relies on employment claim",
    );
    graph.add_dependency(
        DependencyNode::Atom(AtomId::new("derived-plan")),
        DependencyNode::Answer("answer-1".to_owned()),
        "answer used the plan",
    );
    graph.add_dependency(
        DependencyNode::Answer("answer-1".to_owned()),
        DependencyNode::Simulation("simulation-1".to_owned()),
        "simulation reused the answer context",
    );

    let invalidation = TruthMaintenance::new(graph).invalidate(
        DependencyNode::Atom(AtomId::new("source-atom")),
        "source was retracted",
    );

    assert_eq!(
        invalidation.invalidated_nodes,
        vec![
            DependencyNode::Atom(AtomId::new("derived-plan")),
            DependencyNode::Answer("answer-1".to_owned()),
            DependencyNode::Simulation("simulation-1".to_owned())
        ]
    );
    assert_eq!(invalidation.steps.len(), 3);
}

#[test]
fn typed_dependency_edges_track_kind_strength_and_trace() {
    let mut graph = DependencyGraph::new();
    graph
        .add_dependency_edge(DependencyEdge {
            from: AtomId::new("source-employment"),
            to: AtomId::new("claim-worked-at"),
            dependency_type: DependencyType::SupportedBy,
            strength: 0.95,
        })
        .expect("valid dependency");
    graph
        .add_typed_dependency(
            DependencyNode::Atom(AtomId::new("claim-worked-at")),
            DependencyNode::Atom(AtomId::new("belief-current-employment")),
            DependencyType::DerivedFrom,
            0.84,
            "resolved employment belief from source-backed claim",
        )
        .expect("valid typed dependency");

    let trace = graph.trace_from(&DependencyNode::Atom(AtomId::new("source-employment")));

    assert_eq!(trace.len(), 2);
    assert!(trace.iter().any(|step| {
        step.from == DependencyNode::Atom(AtomId::new("source-employment"))
            && step.to == DependencyNode::Atom(AtomId::new("claim-worked-at"))
            && step.dependency_type == DependencyType::SupportedBy
            && step.strength == 0.95
    }));
    assert!(trace.iter().any(|step| {
        step.from == DependencyNode::Atom(AtomId::new("claim-worked-at"))
            && step.to == DependencyNode::Atom(AtomId::new("belief-current-employment"))
            && step.dependency_type == DependencyType::DerivedFrom
            && step.strength == 0.84
    }));
    assert_eq!(
        DependencyEdge {
            from: AtomId::new("bad"),
            to: AtomId::new("edge"),
            dependency_type: DependencyType::Assumes,
            strength: 1.2,
        }
        .validate(),
        Err(KernelError::InvalidDependencyStrength)
    );
}

#[test]
fn source_false_collapse_query_categorizes_beliefs_memories_plans_answers_and_simulations() {
    let mut kernel = RealityKernel::new();
    kernel.insert_atom(source_atom("source-employment"));
    kernel.insert_atom(worked_at_atom(
        "claim-worked-at",
        2021,
        Some(2025),
        100,
        Some("source-employment"),
    ));
    kernel.insert_atom(
        derived_belief_atom("belief-current-employment")
            .depending_on(vec![AtomId::new("claim-worked-at")]),
    );
    kernel.insert_atom(
        memory_atom("memory-employment", "remember current employment")
            .depending_on(vec![AtomId::new("belief-current-employment")]),
    );
    kernel.insert_atom(
        plan_atom("plan-email-company", "email Company B contact")
            .depending_on(vec![AtomId::new("belief-current-employment")]),
    );
    kernel
        .add_dependency_edge(DependencyEdge {
            from: AtomId::new("source-employment"),
            to: AtomId::new("claim-worked-at"),
            dependency_type: DependencyType::SupportedBy,
            strength: 0.97,
        })
        .expect("valid source dependency");
    kernel.add_dependency(
        DependencyNode::Atom(AtomId::new("plan-email-company")),
        DependencyNode::Answer("answer-outreach".to_owned()),
        "answer depends on outreach plan",
    );
    kernel.add_dependency(
        DependencyNode::Answer("answer-outreach".to_owned()),
        DependencyNode::Simulation("simulation-outreach-risk".to_owned()),
        "simulation reused answer context",
    );

    let report = kernel
        .collapse_if_source_false(&AtomId::new("source-employment"))
        .expect("source atom exists");

    assert_eq!(report.root_source.as_str(), "source-employment");
    assert!(report
        .collapsed_atoms
        .contains(&AtomId::new("claim-worked-at")));
    assert_eq!(
        report.collapsed_beliefs,
        vec![AtomId::new("belief-current-employment")]
    );
    assert_eq!(
        report.collapsed_memories,
        vec![AtomId::new("memory-employment")]
    );
    assert_eq!(
        report.collapsed_plans,
        vec![AtomId::new("plan-email-company")]
    );
    assert_eq!(report.collapsed_answers, vec!["answer-outreach".to_owned()]);
    assert_eq!(
        report.collapsed_simulations,
        vec!["simulation-outreach-risk".to_owned()]
    );
    assert!(report
        .dependency_steps
        .iter()
        .any(|step| step.dependency_type == DependencyType::SupportedBy && step.strength == 0.97));
    assert!(report.warning.contains("not a factual conclusion"));

    let vm_result =
        RealityQueryVm::new(&kernel).execute(RealityQuery::IfSourceFalseWhatCollapses {
            source_atom_id: AtomId::new("source-employment"),
        });

    assert_eq!(
        vm_result.question,
        BitemporalQuestion::IfSourceFalseWhatCollapses
    );
    assert_eq!(
        vm_result
            .collapse_report
            .expect("collapse report")
            .collapsed_answers,
        vec!["answer-outreach".to_owned()]
    );
}

#[test]
fn week_three_support_conflict_and_impact_apis_explain_truth_maintenance() {
    let mut kernel = RealityKernel::new();
    kernel.insert_atom(source_atom("source-payroll"));
    kernel.insert_atom(
        worked_at_atom(
            "claim-worked-at",
            2021,
            Some(2025),
            100,
            Some("source-payroll"),
        )
        .depending_on(vec![AtomId::new("source-payroll")]),
    );
    kernel.insert_atom(
        derived_belief_atom("summary-current-employment")
            .depending_on(vec![AtomId::new("claim-worked-at")]),
    );
    kernel.add_dependency(
        DependencyNode::Atom(AtomId::new("summary-current-employment")),
        DependencyNode::Answer("answer-employment".to_owned()),
        "answer quoted summary",
    );
    kernel.insert_atom(
        worked_at_atom(
            "claim-worked-at-conflict",
            2022,
            Some(2023),
            120,
            Some("source-conflict"),
        )
        .with_confidence(Confidence::new(0.52).expect("confidence")),
    );
    kernel.add_conflict(ConflictSet::new(
        "conflict-worked-at",
        vec![
            AtomId::new("claim-worked-at"),
            AtomId::new("claim-worked-at-conflict"),
        ],
        ConflictType::SourceDisagreement,
        ConflictStatus::Unresolved,
        "conflicting employment range",
    ));

    let support: SupportSet = kernel
        .explain_support(&AtomId::new("claim-worked-at"))
        .expect("support exists");
    assert_eq!(support.atom_id.as_str(), "claim-worked-at");
    assert!(support
        .supporting_atoms
        .contains(&AtomId::new("source-payroll")));
    assert!(support
        .source_ids
        .contains(&SourceId::new("source-payroll")));
    assert!(!support.evidence.is_empty());

    let conflicts = kernel.explain_conflict(&AtomId::new("claim-worked-at"));
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].id, "conflict-worked-at");

    let downstream = kernel.compute_downstream_dependencies(&AtomId::new("source-payroll"));
    assert!(downstream.contains(&DependencyNode::Atom(AtomId::new("claim-worked-at"))));
    assert!(downstream.contains(&DependencyNode::Atom(AtomId::new(
        "summary-current-employment"
    ))));
    assert!(downstream.contains(&DependencyNode::Answer("answer-employment".to_owned())));

    let impact: ImpactCone = kernel.compute_impact_if_retracted(&AtomId::new("source-payroll"));
    assert!(impact
        .impacted_atoms
        .contains(&AtomId::new("summary-current-employment")));
    assert!(impact
        .impacted_answers
        .contains(&"answer-employment".to_owned()));
    assert!(!impact.invalidation_trace.steps.is_empty());
}

#[test]
fn summaries_do_not_leak_permissioned_sources_into_ai_context() {
    let mut kernel = RealityKernel::new();
    let mut restricted = worked_at_atom(
        "atom-restricted-source",
        2021,
        None,
        100,
        Some("source-restricted"),
    );
    restricted.permissions = PermissionLabel::Restricted;
    kernel.insert_atom(restricted);
    let mut public_summary = summary_atom(
        "summary-public-over-restricted",
        vec![AtomId::new("atom-restricted-source")],
    );
    public_summary.permissions = PermissionLabel::Public;
    kernel.insert_atom(public_summary);

    assert!(!kernel.summary_is_permission_safe(
        &AtomId::new("summary-public-over-restricted"),
        &[PermissionLabel::Public]
    ));
    assert_eq!(
        kernel.summary_permission_leaks(
            &AtomId::new("summary-public-over-restricted"),
            &[PermissionLabel::Public]
        ),
        vec![AtomId::new("atom-restricted-source")]
    );

    let context = ModelContextCompiler::new(&kernel).compile(
        ModelContextRequest::new(
            "summarize public employment context",
            AgentId::new("agent-web"),
        )
        .valid_at(ValidTime::new(2024))
        .known_at(TxTime::new(200))
        .permission_scope(vec![PermissionLabel::Public])
        .token_budget(512),
    );

    assert!(context
        .permission_filtered_atoms
        .contains(&AtomId::new("summary-public-over-restricted")));
    assert!(!context
        .evidence_pack
        .atoms
        .iter()
        .any(|atom| atom.id.as_str() == "summary-public-over-restricted"));
}

#[test]
fn causal_atoms_are_first_class_and_require_evidence() {
    let mut kernel = RealityKernel::new();
    let atom = causal_atom(
        "sanction-announced",
        "oil-price-increase",
        "supply restriction expectation",
        3,
        0.71,
        vec!["source-energy"],
        vec!["if sanctions do not happen, this price-pressure path weakens"],
    );

    kernel
        .insert_causal_atom(atom.clone())
        .expect("causal atom is valid");

    assert_eq!(
        kernel
            .causal_atoms_from(&EventId::new("sanction-announced"))
            .first(),
        Some(&atom)
    );
    assert_eq!(
        kernel.insert_causal_atom(causal_atom(
            "event-a",
            "event-b",
            "unsupported mechanism",
            1,
            0.5,
            vec![],
            vec![],
        )),
        Err(KernelError::MissingCausalEvidence)
    );
    assert_eq!(
        kernel.insert_causal_atom(causal_atom(
            "same-event",
            "same-event",
            "self-causation",
            1,
            0.5,
            vec!["source-loop"],
            vec![],
        )),
        Err(KernelError::SelfCausation)
    );
}

#[test]
fn causal_queries_answer_causes_next_effects_and_counterfactual_breakage() {
    let kernel = fixture_causal_kernel();

    let causes = kernel.what_caused(&EventId::new("inflation-pressure"), 3);
    let cause_event_ids = causes[0].event_ids();

    assert_eq!(
        cause_event_ids,
        vec![
            EventId::new("sanction-announced"),
            EventId::new("oil-price-increase"),
            EventId::new("inflation-pressure")
        ]
    );
    assert!(causes[0]
        .mechanisms
        .contains(&"supply restriction expectation".to_owned()));
    assert!(causes[0].evidence.contains(&SourceId::new("source-energy")));

    let next = kernel.what_might_happen_next(&EventId::new("sanction-announced"), 1);

    assert_eq!(next.len(), 1);
    assert_eq!(
        next[0].event_ids(),
        vec![
            EventId::new("sanction-announced"),
            EventId::new("oil-price-increase")
        ]
    );

    let impact = kernel.what_breaks_if_event_does_not_occur(&EventId::new("sanction-announced"), 3);

    assert_eq!(
        impact.affected_events,
        vec![
            EventId::new("oil-price-increase"),
            EventId::new("inflation-pressure"),
            EventId::new("contract-risk")
        ]
    );
    assert!(impact
        .downstream_risks
        .contains(&"contract-risk".to_owned()));
    assert!(impact
        .counterfactual_notes
        .iter()
        .any(|note| note.contains("price-pressure path weakens")));
    assert!(impact.warning.contains("not fact"));
}

#[test]
fn causal_query_vm_exposes_strategy_questions_without_labeling_simulation_as_fact() {
    let kernel = fixture_causal_kernel();
    let vm = RealityQueryVm::new(&kernel);

    let caused = vm.execute(RealityQuery::WhatCaused {
        event_id: EventId::new("contract-risk"),
        max_depth: 3,
    });

    assert_eq!(caused.question, BitemporalQuestion::WhatCaused);
    assert!(!caused.causal_paths.is_empty());
    assert_eq!(
        caused.causal_paths[0].event_ids(),
        vec![
            EventId::new("sanction-announced"),
            EventId::new("oil-price-increase"),
            EventId::new("inflation-pressure"),
            EventId::new("contract-risk")
        ]
    );

    let breaks = vm.execute(RealityQuery::WhatBreaksIfEventDoesNotOccur {
        event_id: EventId::new("sanction-announced"),
        max_depth: 3,
    });

    assert_eq!(
        breaks.question,
        BitemporalQuestion::WhatBreaksIfEventDoesNotOccur
    );
    assert!(breaks
        .causal_impact
        .expect("causal impact")
        .warning
        .contains("not fact"));
}

#[test]
fn incremental_engine_appends_event_then_updates_only_touched_views() {
    let mut engine = IncrementalComputation::new();
    let atom = worked_at_atom(
        "atom-source-arrival",
        2024,
        None,
        100,
        Some("source-incremental"),
    );

    let delta = engine
        .apply_event(KernelEvent::AtomInserted(Box::new(atom)))
        .expect("incremental delta");

    assert_eq!(delta.sequence.as_u64(), 1);
    assert_eq!(delta.event_kind, IncrementalEventKind::AtomInserted);
    assert_eq!(engine.event_log().len(), 1);
    assert_eq!(engine.event_log()[0].sequence, delta.sequence);
    assert_eq!(
        delta.touched_atoms,
        vec![AtomId::new("atom-source-arrival")]
    );
    assert!(delta.touched_views.contains(&MaintainedViewName::Graph));
    assert!(delta
        .touched_views
        .contains(&MaintainedViewName::SourceTrust));
    assert!(delta
        .touched_views
        .contains(&MaintainedViewName::AgentMemory));
    assert!(!delta
        .touched_views
        .contains(&MaintainedViewName::Contradictions));
    assert!(!delta.touched_views.contains(&MaintainedViewName::Summaries));

    assert_eq!(
        engine
            .views()
            .atoms_for_subject(&EntityRef::new("person-a")),
        vec![AtomId::new("atom-source-arrival")]
    );
    assert_eq!(
        engine
            .views()
            .atoms_for_source(&SourceId::new("source-incremental")),
        vec![AtomId::new("atom-source-arrival")]
    );
    assert_eq!(
        engine
            .views()
            .atoms_for_agent(&AgentId::new("agent-research")),
        vec![AtomId::new("atom-source-arrival")]
    );
    assert_eq!(engine.views().versions().graph, Some(delta.sequence));
    assert_eq!(engine.views().versions().contradictions, None);
    assert_eq!(engine.views().versions().summaries, None);
}

#[test]
fn incremental_conflict_marks_dependent_summaries_stale_without_rebuilding_graph_view() {
    let mut engine = IncrementalComputation::new();
    engine
        .apply_event(KernelEvent::AtomInserted(Box::new(worked_at_atom(
            "atom-employment",
            2021,
            None,
            100,
            Some("source-employment"),
        ))))
        .expect("employment atom");
    engine
        .apply_event(KernelEvent::AtomInserted(Box::new(summary_atom(
            "summary-employment",
            vec![AtomId::new("atom-employment")],
        ))))
        .expect("summary atom");
    engine
        .apply_event(KernelEvent::AtomInserted(Box::new(
            worked_at_atom(
                "atom-employment-conflict",
                2021,
                None,
                120,
                Some("source-conflict"),
            )
            .with_confidence(Confidence::new(0.74).expect("confidence")),
        )))
        .expect("conflicting atom");
    let graph_version_before_conflict = engine.views().versions().graph;

    let delta = engine
        .apply_event(KernelEvent::ConflictAdded(Box::new(ConflictSet::new(
            "conflict-employment",
            vec![
                AtomId::new("atom-employment"),
                AtomId::new("atom-employment-conflict"),
            ],
            ConflictType::SourceDisagreement,
            ConflictStatus::Unresolved,
            "sources disagree about employment status",
        ))))
        .expect("conflict delta");

    assert_eq!(delta.event_kind, IncrementalEventKind::ConflictAdded);
    assert!(delta
        .touched_views
        .contains(&MaintainedViewName::Contradictions));
    assert!(delta.touched_views.contains(&MaintainedViewName::Summaries));
    assert!(!delta.touched_views.contains(&MaintainedViewName::Graph));
    assert_eq!(
        engine.views().versions().graph,
        graph_version_before_conflict
    );
    assert_eq!(
        delta.stale_summaries,
        vec![AtomId::new("summary-employment")]
    );
    assert!(
        engine
            .views()
            .summary_status(&AtomId::new("summary-employment"))
            .expect("summary status")
            .is_stale
    );
    assert_eq!(
        engine
            .views()
            .conflicts_for_atom(&AtomId::new("atom-employment")),
        vec!["conflict-employment".to_owned()]
    );
}

#[test]
fn incremental_belief_revision_updates_belief_view_and_marks_downstream_plans_risky() {
    let mut engine = IncrementalComputation::new();
    engine
        .apply_event(KernelEvent::AtomInserted(Box::new(worked_at_atom(
            "atom-supplier",
            2024,
            None,
            100,
            Some("source-supplier"),
        ))))
        .expect("supplier atom");
    engine
        .apply_event(KernelEvent::AtomInserted(Box::new(
            plan_atom("plan-renew-contract", "Renew contract with supplier")
                .depending_on(vec![AtomId::new("atom-supplier")]),
        )))
        .expect("plan atom");

    let delta = engine
        .apply_event(KernelEvent::BeliefRevised {
            atom_id: AtomId::new("atom-supplier"),
            next: BeliefState::Refuted,
            known_at: TxTime::new(200),
            reason: "new source refuted supplier relationship".to_owned(),
        })
        .expect("belief revision delta");

    assert_eq!(delta.event_kind, IncrementalEventKind::BeliefRevised);
    assert!(delta.touched_views.contains(&MaintainedViewName::Beliefs));
    assert!(delta.touched_views.contains(&MaintainedViewName::Summaries));
    assert_eq!(delta.risky_plans, vec![AtomId::new("plan-renew-contract")]);
    assert_eq!(
        engine.views().current_belief(&AtomId::new("atom-supplier")),
        Some(BeliefState::Refuted)
    );
}

#[test]
fn native_reasoning_vm_verifies_claim_with_temporal_belief_evidence_contradictions_permissions_and_trace(
) {
    let mut kernel = RealityKernel::new();
    kernel.insert_atom(worked_at_atom(
        "atom-employment-accepted",
        2021,
        Some(2025),
        100,
        Some("source-payroll"),
    ));
    kernel.insert_atom(
        worked_at_atom(
            "atom-employment-conflict",
            2021,
            Some(2024),
            120,
            Some("source-rumor"),
        )
        .with_confidence(Confidence::new(0.42).expect("confidence")),
    );
    kernel.insert_atom(
        memory_atom("memory-downstream", "agent memory relies on employment")
            .depending_on(vec![AtomId::new("atom-employment-accepted")]),
    );
    kernel.add_conflict(ConflictSet::new(
        "conflict-employment-end-date",
        vec![
            AtomId::new("atom-employment-accepted"),
            AtomId::new("atom-employment-conflict"),
        ],
        ConflictType::SourceDisagreement,
        ConflictStatus::Preferred(AtomId::new("atom-employment-accepted")),
        "sources disagree about whether employment ended before 2025",
    ));

    let vm = RealityQueryVm::new(&kernel);
    let query = NativeRealityQuery::verify_claim(
        ClaimPattern::new()
            .subject(EntityRef::new("person-a"))
            .predicate(PredicateId::new("WORKED_AT"))
            .object(ValueOrEntity::entity("company-b")),
    )
    .with_operator(RealityOperator::ValidAt(ValidTime::new(2023)))
    .with_operator(RealityOperator::KnownAt(TxTime::new(260512)))
    .with_operator(RealityOperator::BeliefIn(vec![
        BeliefState::Accepted,
        BeliefState::Disputed,
    ]))
    .with_operator(RealityOperator::RequireEvidence)
    .with_operator(RealityOperator::AllowPermissions(vec![
        PermissionLabel::Internal,
    ]))
    .returning(vec![
        RealityReturnField::Belief,
        RealityReturnField::Evidence,
        RealityReturnField::Contradictions,
        RealityReturnField::DependencyTrace,
    ]);

    let result = vm.execute_native(query);

    assert_eq!(
        result.plan.strategy,
        NativeExecutionStrategy::LeapfrogTriejoinCandidate
    );
    assert_eq!(result.atoms.len(), 2);
    assert!(result
        .beliefs
        .iter()
        .any(
            |(atom_id, belief)| atom_id.as_str() == "atom-employment-accepted"
                && *belief == BeliefState::Disputed
        ));
    assert!(!result.evidence.is_empty());
    assert_eq!(result.contradictions.len(), 1);
    assert!(result
        .dependency_trace
        .iter()
        .any(|step| step.dependency_type == DependencyType::ContradictedBy));
    assert!(result.permission_filtered_atoms.is_empty());
    assert!(result
        .execution_trace
        .iter()
        .any(|step| step.operator == "LeapfrogTriejoinCandidate"));
    assert!(result
        .execution_trace
        .iter()
        .any(|step| step.operator == "PermissionFilter"));
}

#[test]
fn native_reasoning_vm_what_breaks_if_false_classifies_affected_state() {
    let mut kernel = RealityKernel::new();
    kernel.insert_atom(worked_at_atom(
        "atom-supplier-relationship",
        2024,
        None,
        100,
        Some("source-supplier"),
    ));
    kernel.insert_atom(
        derived_belief_atom("belief-supplier-current")
            .depending_on(vec![AtomId::new("atom-supplier-relationship")]),
    );
    kernel.insert_atom(
        plan_atom("plan-renew-supplier", "renew supplier contract")
            .depending_on(vec![AtomId::new("atom-supplier-relationship")]),
    );
    kernel.insert_atom(
        memory_atom("memory-supplier", "supplier was reliable")
            .depending_on(vec![AtomId::new("atom-supplier-relationship")]),
    );
    kernel.insert_atom(summary_atom(
        "summary-supplier-risk",
        vec![AtomId::new("atom-supplier-relationship")],
    ));

    let result = RealityQueryVm::new(&kernel).execute_native(
        NativeRealityQuery::what_breaks_if_false(AtomId::new("atom-supplier-relationship"))
            .returning(vec![
                RealityReturnField::AffectedBeliefs,
                RealityReturnField::Plans,
                RealityReturnField::Memories,
                RealityReturnField::Summaries,
                RealityReturnField::Agents,
            ]),
    );

    assert_eq!(
        result.plan.strategy,
        NativeExecutionStrategy::CounterfactualImpactSearch
    );
    assert!(result
        .affected_beliefs
        .contains(&AtomId::new("belief-supplier-current")));
    assert!(result
        .affected_plans
        .contains(&AtomId::new("plan-renew-supplier")));
    assert!(result
        .affected_memories
        .contains(&AtomId::new("memory-supplier")));
    assert!(result
        .affected_summaries
        .contains(&AtomId::new("summary-supplier-risk")));
    assert!(result
        .affected_agents
        .contains(&AgentId::new("agent-research")));
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.contains("not fact")));
    assert!(result
        .execution_trace
        .iter()
        .any(|step| step.operator == "DependencyInvalidation"));
}

#[test]
fn week_four_kernel_query_ast_supports_named_variants_and_metadata_results() {
    let mut kernel = RealityKernel::new();
    kernel.insert_atom(source_atom("source-kernel-query"));
    kernel.insert_atom(
        worked_at_atom(
            "atom-query-worked",
            2021,
            Some(2025),
            100,
            Some("source-kernel-query"),
        )
        .depending_on(vec![AtomId::new("source-kernel-query")]),
    );
    kernel.insert_atom(
        worked_at_atom(
            "atom-query-conflict",
            2022,
            Some(2023),
            120,
            Some("source-query-conflict"),
        )
        .with_confidence(Confidence::new(0.5).expect("confidence")),
    );
    kernel.add_conflict(ConflictSet::new(
        "conflict-query",
        vec![
            AtomId::new("atom-query-worked"),
            AtomId::new("atom-query-conflict"),
        ],
        ConflictType::ValidTimeOverlap,
        ConflictStatus::Unresolved,
        "query fixture conflict",
    ));

    let vm = RealityQueryVm::new(&kernel);
    let get_result = vm.execute_kernel(KernelQuery::GetAtom(AtomId::new("atom-query-worked")));
    assert_eq!(get_result.atom_ids, vec![AtomId::new("atom-query-worked")]);
    assert_eq!(
        get_result.beliefs,
        vec![(AtomId::new("atom-query-worked"), BeliefState::Disputed)]
    );
    assert_eq!(
        get_result.evidence_ids,
        vec![SourceId::new("source-kernel-query")]
    );

    let visible_result = vm.execute_kernel(KernelQuery::VisibleAt {
        valid_at: ValidTime::new(2022),
        known_at: TransactionTime::new(200),
        pattern: AtomPattern::new()
            .subject(EntityRef::new("person-a"))
            .predicate(PredicateId::new("WORKED_AT")),
    });
    assert!(visible_result
        .atom_ids
        .contains(&AtomId::new("atom-query-worked")));
    assert!(visible_result
        .atom_ids
        .contains(&AtomId::new("atom-query-conflict")));
    assert!(visible_result
        .valid_times
        .contains_key(&AtomId::new("atom-query-worked")));
    assert!(visible_result
        .transaction_times
        .contains_key(&AtomId::new("atom-query-worked")));

    let support = vm.execute_kernel(KernelQuery::ExplainSupport {
        atom_id: AtomId::new("atom-query-worked"),
    });
    assert!(support
        .support
        .expect("support")
        .supporting_atoms
        .contains(&AtomId::new("source-kernel-query")));

    let conflict = vm.execute_kernel(KernelQuery::ExplainConflict {
        atom_id: AtomId::new("atom-query-worked"),
    });
    assert_eq!(conflict.conflicts.len(), 1);

    let impact = vm.execute_kernel(KernelQuery::ImpactIfRetracted {
        atom_id: AtomId::new("source-kernel-query"),
        max_depth: 4,
    });
    assert!(impact
        .impact
        .expect("impact")
        .impacted_atoms
        .contains(&AtomId::new("atom-query-worked")));
}

#[test]
fn physical_graph_store_writes_atoms_to_columnar_table_and_compressed_adjacency() {
    let store = PhysicalGraphStore::from_atoms(vec![
        physical_atom((
            "atom-a-b",
            "person-a",
            "WORKED_AT",
            "company-b",
            2021,
            None,
            100,
            "source-payroll",
        )),
        physical_atom((
            "atom-a-c",
            "person-a",
            "ADVISED",
            "company-c",
            2022,
            None,
            110,
            "source-advisory",
        )),
        physical_atom((
            "atom-d-b",
            "person-d",
            "WORKED_AT",
            "company-b",
            2023,
            None,
            120,
            "source-payroll",
        )),
    ]);

    assert_eq!(store.atom_count(), 3);
    assert!(store.columnar().is_dense());

    let outgoing = store.outgoing_for_subject(&EntityRef::new("person-a"));
    assert_eq!(
        store.atom_ids_for_candidates(&outgoing),
        vec![AtomId::new("atom-a-b"), AtomId::new("atom-a-c")]
    );

    let incoming = store.incoming_for_object_entity(&EntityRef::new("company-b"));
    let worked_at = store.atoms_for_predicate(&PredicateId::new("WORKED_AT"));
    let payroll = store.atoms_for_source(&SourceId::new("source-payroll"));
    let joined = incoming.intersect(&worked_at).intersect(&payroll);

    assert_eq!(
        store.atom_ids_for_candidates(&joined),
        vec![AtomId::new("atom-a-b"), AtomId::new("atom-d-b")]
    );
}

#[test]
fn physical_graph_store_uses_temporal_indexes_and_bitmaps_for_point_in_time_candidates() {
    let store = PhysicalGraphStore::from_atoms(vec![
        physical_atom((
            "atom-historical",
            "person-a",
            "WORKED_AT",
            "company-b",
            2020,
            Some(2022),
            100,
            "source-old",
        )),
        physical_atom((
            "atom-current",
            "person-a",
            "WORKED_AT",
            "company-b",
            2022,
            None,
            200,
            "source-current",
        )),
        physical_atom((
            "atom-not-yet-known",
            "person-a",
            "WORKED_AT",
            "company-b",
            2021,
            None,
            500,
            "source-future",
        )),
    ]);

    let candidates = store.point_in_time_candidates(ValidTime::new(2023), TxTime::new(250));

    assert_eq!(
        store.atom_ids_for_candidates(&candidates),
        vec![AtomId::new("atom-current")]
    );
    assert_eq!(candidates.intersect(&candidates).len(), 1);
}

#[test]
fn physical_graph_store_has_trie_index_for_fully_bound_claim_lookup() {
    let store = PhysicalGraphStore::from_atoms(vec![
        physical_atom((
            "atom-target",
            "person-a",
            "WORKED_AT",
            "company-b",
            2021,
            None,
            100,
            "source-payroll",
        )),
        physical_atom((
            "atom-other-predicate",
            "person-a",
            "ADVISED",
            "company-b",
            2021,
            None,
            100,
            "source-advisory",
        )),
    ]);

    let candidates = store.trie_candidates_for_claim(
        &ClaimPattern::new()
            .subject(EntityRef::new("person-a"))
            .predicate(PredicateId::new("WORKED_AT"))
            .object(ValueOrEntity::entity("company-b")),
    );

    assert_eq!(
        store.atom_ids_for_candidates(&candidates),
        vec![AtomId::new("atom-target")]
    );
}

#[test]
fn physical_layout_manifest_names_hot_cold_snapshot_and_sidecar_layers() {
    let manifest = PhysicalGraphStore::layout_manifest();

    assert!(manifest.contains(&PhysicalLayoutKind::AppendOnlyEventLog));
    assert!(manifest.contains(&PhysicalLayoutKind::ColumnarAtomStore));
    assert!(manifest.contains(&PhysicalLayoutKind::CompressedAdjacencyLists));
    assert!(manifest.contains(&PhysicalLayoutKind::TemporalIntervalIndexes));
    assert!(manifest.contains(&PhysicalLayoutKind::RoaringBitmapCandidateSets));
    assert!(manifest.contains(&PhysicalLayoutKind::TrieJoinIndexes));
    assert!(manifest.contains(&PhysicalLayoutKind::MemoryMappedSnapshots));
    assert!(manifest.contains(&PhysicalLayoutKind::HotWorkingSetCache));
    assert!(manifest.contains(&PhysicalLayoutKind::ColdHistoricalSegmentStore));
    assert!(manifest.contains(&PhysicalLayoutKind::VectorSourceSidecar));
}

#[test]
fn model_context_compiler_returns_ai_native_context_not_random_chunks() {
    let mut kernel = RealityKernel::new();
    kernel.insert_atom(worked_at_atom(
        "atom-employment-accepted",
        2021,
        Some(2025),
        100,
        Some("source-payroll"),
    ));
    kernel.insert_atom(
        worked_at_atom(
            "atom-employment-conflict",
            2021,
            Some(2024),
            120,
            Some("source-rumor"),
        )
        .with_confidence(Confidence::new(0.42).expect("confidence")),
    );
    kernel.insert_atom(
        memory_atom("memory-employment", "agent remembers employment risk")
            .depending_on(vec![AtomId::new("atom-employment-accepted")]),
    );
    kernel.add_conflict(ConflictSet::new(
        "conflict-employment",
        vec![
            AtomId::new("atom-employment-accepted"),
            AtomId::new("atom-employment-conflict"),
        ],
        ConflictType::SourceDisagreement,
        ConflictStatus::Unresolved,
        "sources disagree about employment end date",
    ));

    let context = ModelContextCompiler::new(&kernel).compile(
        ModelContextRequest::new(
            "Can I rely on Person A working at Company B for a 2023 plan?",
            AgentId::new("agent-research"),
        )
        .current_goal("prepare a source-backed plan")
        .valid_at(ValidTime::new(2023))
        .known_at(TxTime::new(260512))
        .permission_scope(vec![PermissionLabel::Internal])
        .token_budget(240)
        .risk_level(RiskLevel::High),
    );

    assert_eq!(
        context.task,
        "Can I rely on Person A working at Company B for a 2023 plan?"
    );
    assert!(context.estimated_tokens <= 240);
    assert!(context
        .evidence_pack
        .atoms
        .iter()
        .any(|atom| atom.id.as_str() == "atom-employment-accepted"));
    assert!(context
        .current_belief_state
        .iter()
        .any(
            |belief| belief.atom_id.as_str() == "atom-employment-accepted"
                && belief.belief_state == BeliefState::Disputed
        ));
    assert!(context
        .relevant_memories
        .iter()
        .any(|memory| memory.id.as_str() == "memory-employment"));
    assert_eq!(context.contradictions.len(), 1);
    assert!(!context.evidence_pack.evidence.is_empty());
    assert!(context
        .safe_assumptions
        .iter()
        .any(|assumption| assumption.how_we_know.contains("source-backed")));
    assert!(context
        .missing_information
        .iter()
        .any(|missing| missing.description.contains("contradiction")));
    assert!(context
        .recommended_actions
        .iter()
        .any(|action| action.kind == RecommendedActionKind::ReviewContradiction));
}

#[test]
fn model_context_compiler_enforces_permissions_budget_and_missing_evidence_actions() {
    let mut kernel = RealityKernel::new();
    kernel.insert_atom(worked_at_atom(
        "atom-internal",
        2021,
        None,
        100,
        Some("source-internal"),
    ));
    let mut restricted = worked_at_atom(
        "atom-restricted",
        2021,
        None,
        100,
        Some("source-restricted"),
    )
    .with_confidence(Confidence::new(0.88).expect("confidence"))
    .with_belief_state(BeliefState::Accepted);
    restricted.permissions = PermissionLabel::Restricted;
    kernel.insert_atom(restricted);

    let context = ModelContextCompiler::new(&kernel).compile(
        ModelContextRequest::new(
            "Need complete context for employment decision",
            AgentId::new("agent-research"),
        )
        .valid_at(ValidTime::new(2024))
        .known_at(TxTime::new(200))
        .permission_scope(vec![PermissionLabel::Internal])
        .token_budget(32)
        .risk_level(RiskLevel::Critical),
    );

    assert!(context.estimated_tokens <= 32);
    assert!(context
        .permission_filtered_atoms
        .contains(&AtomId::new("atom-restricted")));
    assert!(!context
        .evidence_pack
        .atoms
        .iter()
        .any(|atom| atom.id.as_str() == "atom-restricted"));
    assert!(context
        .missing_information
        .iter()
        .any(|missing| missing.description.contains("permission")));
    assert!(context
        .recommended_actions
        .iter()
        .any(|action| action.kind == RecommendedActionKind::RetrieveEvidence));
}

#[test]
fn self_revision_engine_suggests_auditable_repairs_without_rewriting_truth() {
    let kernel = self_revision_fixture_kernel();
    let old_candidate_before = kernel
        .belief_at(&AtomId::new("belief-stale-candidate"), TxTime::new(1_000))
        .expect("candidate belief exists");
    let summary_before = kernel
        .atom(&AtomId::new("summary-employment"))
        .expect("summary exists")
        .belief_state
        .clone();

    let policy = SelfRevisionPolicy::review_only(TxTime::new(1_000))
        .with_stale_tx_lag(100)
        .with_known_predicates(vec![
            PredicateId::new("ENTITY_NAME"),
            PredicateId::new("WORKED_AT"),
            PredicateId::new("CURRENT_EMPLOYMENT_BELIEF"),
            PredicateId::new("HAS_MEMORY"),
            PredicateId::new("HAS_PLAN"),
            PredicateId::new("SUMMARY_EMPLOYMENT"),
        ]);
    let report = SelfRevisionEngine::new(policy).run_all(&kernel, SelfRevisionCursor::default());

    assert_eq!(report.review_status, SelfRevisionReviewStatus::Pending);
    assert_eq!(
        report.next_cursor,
        SelfRevisionCursor::from_tx(TxTime::new(1_000))
    );
    assert_eq!(report.jobs, SelfRevisionJob::all());

    let kinds = report
        .suggestions
        .iter()
        .map(|suggestion| suggestion.kind)
        .collect::<std::collections::BTreeSet<_>>();
    for expected in [
        SelfRevisionSuggestionKind::SuggestEntityDeduplication,
        SelfRevisionSuggestionKind::RecalibrateSourceTrust,
        SelfRevisionSuggestionKind::FlagOntologyDrift,
        SelfRevisionSuggestionKind::ClusterContradictions,
        SelfRevisionSuggestionKind::InvalidateSummary,
        SelfRevisionSuggestionKind::ConsolidateMemory,
        SelfRevisionSuggestionKind::MarkStaleBelief,
        SelfRevisionSuggestionKind::InvalidateDependencies,
        SelfRevisionSuggestionKind::RefineCausalHypothesis,
    ] {
        assert!(kinds.contains(&expected), "missing {expected:?}");
    }

    assert!(report
        .suggestions
        .iter()
        .all(|suggestion| suggestion.requires_review && !suggestion.auto_applied));
    assert!(report
        .suggestions
        .iter()
        .all(|suggestion| !suggestion.audit_event_id.is_empty()));
    assert!(report
        .audit_log
        .iter()
        .any(|entry| entry.message.contains("suggestions only")));
    assert!(report
        .suggestions
        .iter()
        .any(|suggestion| !suggestion.dependency_trace.is_empty()));

    assert_eq!(
        kernel.belief_at(&AtomId::new("belief-stale-candidate"), TxTime::new(1_000)),
        Some(old_candidate_before)
    );
    assert_eq!(
        kernel
            .atom(&AtomId::new("summary-employment"))
            .expect("summary still exists")
            .belief_state,
        summary_before
    );
}

#[test]
fn self_revision_cursor_limits_incremental_scans_without_losing_auditability() {
    let kernel = self_revision_fixture_kernel();
    let policy = SelfRevisionPolicy::review_only(TxTime::new(1_000)).with_stale_tx_lag(100);
    let report = SelfRevisionEngine::new(policy).run_job(
        SelfRevisionJob::StaleBeliefDetection,
        &kernel,
        SelfRevisionCursor::from_tx(TxTime::new(500)),
    );

    assert!(report.suggestions.is_empty());
    assert_eq!(report.jobs, vec![SelfRevisionJob::StaleBeliefDetection]);
    assert_eq!(report.review_status, SelfRevisionReviewStatus::Pending);
    assert!(report.incremental);
    assert!(report
        .audit_log
        .iter()
        .any(|entry| entry.message.contains("incremental cursor")));
}

#[test]
fn impact_if_retracted_reports_downstream_memory_plan_answer_and_simulation() {
    let mut kernel = RealityKernel::new();
    kernel.insert_atom(worked_at_atom(
        "atom-worked",
        2021,
        Some(2025),
        100,
        Some("source-1"),
    ));
    kernel.insert_atom(
        memory_atom("memory-plan", "safe to plan with Company B relationship")
            .depending_on(vec![AtomId::new("atom-worked")]),
    );
    kernel.add_dependency(
        DependencyNode::Atom(AtomId::new("memory-plan")),
        DependencyNode::Answer("answer-1".to_owned()),
        "answer relied on memory-plan",
    );
    kernel.add_dependency(
        DependencyNode::Answer("answer-1".to_owned()),
        DependencyNode::Simulation("simulation-1".to_owned()),
        "simulation reused answer",
    );

    let report: AtomImpactReport = kernel.impact_if_retracted(&AtomId::new("atom-worked"));

    assert_eq!(report.root.as_str(), "atom-worked");
    assert!(report
        .impacted_atoms
        .iter()
        .any(|atom| atom.as_str() == "memory-plan"));
    assert!(report.impacted_answers.contains(&"answer-1".to_owned()));
    assert!(report
        .impacted_simulations
        .contains(&"simulation-1".to_owned()));
    assert!(report.warning.contains("not fact"));
}

#[test]
fn entity_state_at_valid_time_and_known_at_never_returns_unsupported_ai_conclusions() {
    let mut kernel = RealityKernel::new();
    kernel.insert_atom(worked_at_atom(
        "atom-supported",
        2021,
        Some(2025),
        100,
        Some("source-1"),
    ));
    kernel.insert_atom(
        RealityAtom::builder(
            AtomId::new("atom-unsupported"),
            EntityRef::new("person-a"),
            PredicateId::new("LIKELY_TO_LEAVE"),
            ValueOrEntity::text("soon"),
        )
        .valid_time(TimeInterval::new(ValidTime::new(2024), None).expect("valid"))
        .transaction_time(TimeInterval::new(TxTime::new(150), None).expect("tx"))
        .confidence(Confidence::new(0.42).expect("confidence"))
        .source_ref(SourceRef::new(SourceId::new("source-agent-hypothesis")))
        .evidence_span(EvidenceSpan::new(
            SourceId::new("source-agent-hypothesis"),
            0,
            12,
            "unsupported guess",
        ))
        .belief_state(BeliefState::Candidate)
        .claim_type(ClaimType::Hypothesis)
        .ai_usage(AiUsage::UnsafeForPlanning(
            "candidate hypothesis, not supported conclusion".to_owned(),
        ))
        .build()
        .expect("atom"),
    );

    let vm = RealityQueryVm::new(&kernel);
    let result = vm.execute(RealityQuery::EntityState {
        entity: EntityRef::new("person-a"),
        valid_at: ValidTime::new(2024),
        known_at: TxTime::new(200),
        ai_facing: true,
    });

    assert_eq!(result.atoms.len(), 1);
    assert_eq!(result.atoms[0].id.as_str(), "atom-supported");
    assert!(result
        .unsupported_conclusions
        .iter()
        .any(|atom| atom.id.as_str() == "atom-unsupported"));
    assert!(result
        .evidence
        .iter()
        .any(|span| span.quote.contains("worked at")));
}

fn worked_at_atom(
    id: &str,
    valid_from: i64,
    valid_to: Option<i64>,
    tx_from: i64,
    source: Option<&str>,
) -> RealityAtom {
    let source_id = SourceId::new(source.unwrap_or("source-employment"));
    RealityAtom::builder(
        AtomId::new(id),
        EntityRef::new("person-a"),
        PredicateId::new("WORKED_AT"),
        ValueOrEntity::entity("company-b"),
    )
    .valid_time(
        TimeInterval::new(ValidTime::new(valid_from), valid_to.map(ValidTime::new))
            .expect("valid interval"),
    )
    .transaction_time(TimeInterval::new(TxTime::new(tx_from), None).expect("tx interval"))
    .observed_time(ValidTime::new(valid_from))
    .claim_type(ClaimType::Observation)
    .belief_state(BeliefState::Accepted)
    .confidence(Confidence::new(0.91).expect("confidence"))
    .source_ref(SourceRef::new(source_id.clone()).with_uri("file://employment.md"))
    .evidence_span(EvidenceSpan::new(
        source_id,
        12,
        68,
        "Person A worked at Company B from 2021 to 2024.",
    ))
    .extraction_trace(ExtractionTrace::new(
        "deterministic-fixture",
        "rule-worked-at",
    ))
    .tenant_id(TenantId::new("tenant-lab"))
    .agent_scope(AgentId::new("agent-research"))
    .permissions(PermissionLabel::Internal)
    .taint(TaintLabel::Trusted)
    .ai_usage(AiUsage::SafeForPlanning {
        caveat: Some("safe if exact end date is not critical".to_owned()),
    })
    .build()
    .expect("atom")
}

fn self_revision_fixture_kernel() -> RealityKernel {
    let mut kernel = RealityKernel::new();
    kernel.insert_atom(entity_name_atom(
        "entity-name-alice-a",
        "person-alice-a",
        "Alice Salehi",
        100,
    ));
    kernel.insert_atom(entity_name_atom(
        "entity-name-alice-b",
        "person-alice-b",
        " alice   salehi ",
        110,
    ));
    kernel.insert_atom(worked_at_atom(
        "atom-worked",
        2021,
        Some(2025),
        100,
        Some("source-payroll"),
    ));
    kernel.insert_atom(
        worked_at_atom(
            "atom-worked-conflict",
            2021,
            Some(2024),
            120,
            Some("source-conflict"),
        )
        .with_confidence(Confidence::new(0.42).expect("confidence")),
    );
    kernel.add_conflict(ConflictSet::new(
        "conflict-employment-end-date",
        vec![
            AtomId::new("atom-worked"),
            AtomId::new("atom-worked-conflict"),
        ],
        ConflictType::SourceDisagreement,
        ConflictStatus::Unresolved,
        "sources disagree about employment end date",
    ));
    kernel.insert_atom(summary_atom(
        "summary-employment",
        vec![AtomId::new("atom-worked")],
    ));
    kernel.insert_atom(memory_atom(
        "memory-employment-a",
        "remember Person A employment risk",
    ));
    kernel.insert_atom(memory_atom(
        "memory-employment-b",
        " remember   person a employment risk ",
    ));
    let mut stale_belief =
        derived_belief_atom("belief-stale-candidate").with_belief_state(BeliefState::Candidate);
    stale_belief.transaction_time = TimeInterval::new(TxTime::new(10), None).expect("tx interval");
    kernel.insert_atom(stale_belief);
    kernel.insert_atom(
        plan_atom("plan-employment", "renew plan depends on employment")
            .depending_on(vec![AtomId::new("atom-worked")]),
    );
    let mut tainted = worked_at_atom(
        "atom-tainted-source",
        2024,
        None,
        900,
        Some("source-tainted"),
    );
    tainted.taint = TaintLabel::PromptInjectionRisk;
    kernel.insert_atom(tainted);
    kernel.insert_atom(custom_predicate_atom(
        "atom-new-predicate",
        "PERSONALLY_GUARANTEES",
        910,
    ));
    kernel
        .insert_causal_atom(CausalAtom {
            cause: EventId::new("supplier-delay"),
            effect: EventId::new("revenue-risk"),
            mechanism: Some("untested correlation from low-confidence extraction".to_owned()),
            lag: Some(Duration::from_secs(7 * 86_400)),
            confidence: Confidence::new(0.24).expect("confidence"),
            evidence: vec![SourceId::new("source-causal-hypothesis")],
            counterfactual_notes: vec![
                "low-confidence causal hypothesis needs review before planning".to_owned(),
            ],
        })
        .expect("causal hypothesis");
    kernel
}

fn entity_name_atom(id: &str, entity: &str, name: &str, tx_from: i64) -> RealityAtom {
    let source_id = SourceId::new(format!("source-{id}"));
    RealityAtom::builder(
        AtomId::new(id),
        EntityRef::new(entity),
        PredicateId::new("ENTITY_NAME"),
        ValueOrEntity::text(name),
    )
    .valid_time(TimeInterval::new(ValidTime::new(2024), None).expect("valid"))
    .transaction_time(TimeInterval::new(TxTime::new(tx_from), None).expect("tx"))
    .claim_type(ClaimType::Observation)
    .belief_state(BeliefState::Accepted)
    .confidence(Confidence::new(0.94).expect("confidence"))
    .source_ref(SourceRef::new(source_id.clone()))
    .evidence_span(EvidenceSpan::new(source_id, 0, name.len(), name))
    .tenant_id(TenantId::new("tenant-lab"))
    .permissions(PermissionLabel::Internal)
    .taint(TaintLabel::Trusted)
    .ai_usage(AiUsage::SafeForPlanning { caveat: None })
    .build()
    .expect("entity name atom")
}

fn custom_predicate_atom(id: &str, predicate: &str, tx_from: i64) -> RealityAtom {
    let source_id = SourceId::new(format!("source-{id}"));
    RealityAtom::builder(
        AtomId::new(id),
        EntityRef::new("person-a"),
        PredicateId::new(predicate),
        ValueOrEntity::entity("company-b"),
    )
    .valid_time(TimeInterval::new(ValidTime::new(2024), None).expect("valid"))
    .transaction_time(TimeInterval::new(TxTime::new(tx_from), None).expect("tx"))
    .claim_type(ClaimType::Assertion)
    .belief_state(BeliefState::Candidate)
    .confidence(Confidence::new(0.62).expect("confidence"))
    .source_ref(SourceRef::new(source_id.clone()))
    .evidence_span(EvidenceSpan::new(
        source_id,
        0,
        48,
        "candidate extracted relationship with unknown predicate",
    ))
    .tenant_id(TenantId::new("tenant-lab"))
    .permissions(PermissionLabel::Internal)
    .taint(TaintLabel::Trusted)
    .ai_usage(AiUsage::UseWithCaution(
        "predicate is not yet approved by ontology".to_owned(),
    ))
    .build()
    .expect("custom predicate atom")
}

fn physical_atom(fixture: (&str, &str, &str, &str, i64, Option<i64>, i64, &str)) -> RealityAtom {
    let (id, subject, predicate, object, valid_from, valid_to, tx_from, source) = fixture;
    let source_id = SourceId::new(source);
    RealityAtom::builder(
        AtomId::new(id),
        EntityRef::new(subject),
        PredicateId::new(predicate),
        ValueOrEntity::entity(object),
    )
    .valid_time(
        TimeInterval::new(ValidTime::new(valid_from), valid_to.map(ValidTime::new))
            .expect("valid interval"),
    )
    .transaction_time(TimeInterval::new(TxTime::new(tx_from), None).expect("tx interval"))
    .observed_time(ValidTime::new(valid_from))
    .claim_type(ClaimType::Observation)
    .belief_state(BeliefState::Accepted)
    .confidence(Confidence::new(0.91).expect("confidence"))
    .source_ref(SourceRef::new(source_id.clone()))
    .evidence_span(EvidenceSpan::new(
        source_id,
        0,
        32,
        "physical storage fixture evidence",
    ))
    .tenant_id(TenantId::new("tenant-lab"))
    .permissions(PermissionLabel::Internal)
    .taint(TaintLabel::Trusted)
    .ai_usage(AiUsage::SafeForPlanning { caveat: None })
    .build()
    .expect("physical atom")
}

fn source_atom(id: &str) -> RealityAtom {
    RealityAtom::builder(
        AtomId::new(id),
        EntityRef::new(id),
        PredicateId::new("SOURCE_DOCUMENT"),
        ValueOrEntity::text("employment source document"),
    )
    .valid_time(TimeInterval::new(ValidTime::new(2024), None).expect("valid"))
    .transaction_time(TimeInterval::new(TxTime::new(90), None).expect("tx"))
    .claim_type(ClaimType::Observation)
    .belief_state(BeliefState::Accepted)
    .confidence(Confidence::new(0.99).expect("confidence"))
    .source_ref(SourceRef::new(SourceId::new(id)))
    .evidence_span(EvidenceSpan::new(
        SourceId::new(id),
        0,
        28,
        "employment source document",
    ))
    .tenant_id(TenantId::new("tenant-lab"))
    .permissions(PermissionLabel::Internal)
    .taint(TaintLabel::Trusted)
    .ai_usage(AiUsage::SafeForPlanning { caveat: None })
    .build()
    .expect("source atom")
}

struct AcquisitionClaim<'a> {
    id: &'a str,
    predicate: &'a str,
    quote: &'a str,
    valid_at: i64,
    tx_at: i64,
    belief_state: BeliefState,
    source: &'a str,
    confidence: f32,
}

fn acquisition_atom(claim: AcquisitionClaim<'_>) -> RealityAtom {
    RealityAtom::builder(
        AtomId::new(claim.id),
        EntityRef::new("company-x"),
        PredicateId::new(claim.predicate),
        ValueOrEntity::entity("company-y"),
    )
    .valid_time(TimeInterval::new(ValidTime::new(claim.valid_at), None).expect("valid"))
    .transaction_time(TimeInterval::new(TxTime::new(claim.tx_at), None).expect("tx"))
    .claim_type(ClaimType::Assertion)
    .belief_state(claim.belief_state)
    .confidence(Confidence::new(claim.confidence).expect("confidence"))
    .source_ref(
        SourceRef::new(SourceId::new(claim.source)).with_uri(format!("file://{}.md", claim.source)),
    )
    .evidence_span(EvidenceSpan::new(
        SourceId::new(claim.source),
        0,
        claim.quote.len(),
        claim.quote,
    ))
    .tenant_id(TenantId::new("tenant-lab"))
    .permissions(PermissionLabel::Internal)
    .taint(TaintLabel::Trusted)
    .ai_usage(AiUsage::SafeForPlanning {
        caveat: Some("use with acquisition timeline and conflict context".to_owned()),
    })
    .build()
    .expect("acquisition atom")
}

fn derived_belief_atom(id: &str) -> RealityAtom {
    RealityAtom::builder(
        AtomId::new(id),
        EntityRef::new("person-a"),
        PredicateId::new("CURRENT_EMPLOYMENT_BELIEF"),
        ValueOrEntity::entity("company-b"),
    )
    .valid_time(TimeInterval::new(ValidTime::new(2024), None).expect("valid"))
    .transaction_time(TimeInterval::new(TxTime::new(180), None).expect("tx"))
    .claim_type(ClaimType::Derived)
    .belief_state(BeliefState::Accepted)
    .confidence(Confidence::new(0.86).expect("confidence"))
    .source_ref(SourceRef::new(SourceId::new("source-employment")))
    .evidence_span(EvidenceSpan::new(
        SourceId::new("source-employment"),
        0,
        28,
        "derived employment belief",
    ))
    .dependencies(vec![AtomId::new("source-employment")])
    .tenant_id(TenantId::new("tenant-lab"))
    .permissions(PermissionLabel::Internal)
    .taint(TaintLabel::Trusted)
    .ai_usage(AiUsage::SafeForPlanning { caveat: None })
    .build()
    .expect("derived belief atom")
}

fn plan_atom(id: &str, content: &str) -> RealityAtom {
    RealityAtom::builder(
        AtomId::new(id),
        EntityRef::new("agent-research"),
        PredicateId::new("HAS_PLAN"),
        ValueOrEntity::text(content),
    )
    .valid_time(TimeInterval::new(ValidTime::new(2024), None).expect("valid"))
    .transaction_time(TimeInterval::new(TxTime::new(181), None).expect("tx"))
    .claim_type(ClaimType::AgentMemory)
    .belief_state(BeliefState::Accepted)
    .confidence(Confidence::new(0.78).expect("confidence"))
    .source_ref(SourceRef::new(SourceId::new("source-plan")))
    .evidence_span(EvidenceSpan::new(
        SourceId::new("source-plan"),
        0,
        20,
        "plan write accepted",
    ))
    .extraction_trace(ExtractionTrace::new(
        "deterministic-fixture",
        "memory-write",
    ))
    .tenant_id(TenantId::new("tenant-lab"))
    .agent_scope(AgentId::new("agent-research"))
    .permissions(PermissionLabel::Internal)
    .taint(TaintLabel::Trusted)
    .ai_usage(AiUsage::SafeForPlanning { caveat: None })
    .build()
    .expect("plan atom")
}

fn memory_atom(id: &str, content: &str) -> RealityAtom {
    RealityAtom::builder(
        AtomId::new(id),
        EntityRef::new("agent-research"),
        PredicateId::new("HAS_MEMORY"),
        ValueOrEntity::text(content),
    )
    .valid_time(TimeInterval::new(ValidTime::new(2024), None).expect("valid"))
    .transaction_time(TimeInterval::new(TxTime::new(180), None).expect("tx"))
    .claim_type(ClaimType::AgentMemory)
    .belief_state(BeliefState::Accepted)
    .confidence(Confidence::new(0.8).expect("confidence"))
    .source_ref(SourceRef::new(SourceId::new("source-memory")))
    .evidence_span(EvidenceSpan::new(
        SourceId::new("source-memory"),
        0,
        22,
        "memory write accepted",
    ))
    .extraction_trace(ExtractionTrace::new(
        "deterministic-fixture",
        "memory-write",
    ))
    .tenant_id(TenantId::new("tenant-lab"))
    .agent_scope(AgentId::new("agent-research"))
    .permissions(PermissionLabel::Internal)
    .taint(TaintLabel::Trusted)
    .ai_usage(AiUsage::SafeForPlanning { caveat: None })
    .build()
    .expect("memory atom")
}

fn summary_atom(id: &str, dependencies: Vec<AtomId>) -> RealityAtom {
    RealityAtom::builder(
        AtomId::new(id),
        EntityRef::new("person-a"),
        PredicateId::new("SUMMARY_EMPLOYMENT"),
        ValueOrEntity::text("Person A employment summary"),
    )
    .valid_time(TimeInterval::new(ValidTime::new(2024), None).expect("valid interval"))
    .transaction_time(TimeInterval::new(TxTime::new(150), None).expect("tx interval"))
    .claim_type(ClaimType::Summary)
    .belief_state(BeliefState::Accepted)
    .confidence(Confidence::new(0.86).expect("confidence"))
    .source_ref(SourceRef::new(SourceId::new("source-summary")))
    .evidence_span(EvidenceSpan::new(
        SourceId::new("source-summary"),
        0,
        32,
        "Summary generated from source atoms.",
    ))
    .tenant_id(TenantId::new("tenant-lab"))
    .agent_scope(AgentId::new("agent-research"))
    .permissions(PermissionLabel::Internal)
    .taint(TaintLabel::Trusted)
    .ai_usage(AiUsage::SafeForPlanning {
        caveat: Some("summary must be invalidated by upstream changes".to_owned()),
    })
    .build()
    .expect("summary atom")
    .depending_on(dependencies)
}

fn fixture_causal_kernel() -> RealityKernel {
    let mut kernel = RealityKernel::new();
    for atom in [
        causal_atom(
            "sanction-announced",
            "oil-price-increase",
            "supply restriction expectation",
            3,
            0.71,
            vec!["source-energy"],
            vec!["if sanctions do not happen, this price-pressure path weakens"],
        ),
        causal_atom(
            "oil-price-increase",
            "inflation-pressure",
            "energy input costs",
            14,
            0.68,
            vec!["source-macro"],
            vec!["without oil price pressure, inflation risk is lower"],
        ),
        causal_atom(
            "inflation-pressure",
            "contract-risk",
            "indexed supplier contracts",
            30,
            0.63,
            vec!["source-contracts"],
            vec!["contract-risk should be treated as downstream simulation"],
        ),
    ] {
        kernel.insert_causal_atom(atom).expect("causal atom");
    }
    kernel
}

fn causal_atom(
    cause: &str,
    effect: &str,
    mechanism: &str,
    lag_days: u64,
    confidence: f32,
    evidence: Vec<&str>,
    counterfactual_notes: Vec<&str>,
) -> CausalAtom {
    CausalAtom {
        cause: EventId::new(cause),
        effect: EventId::new(effect),
        mechanism: Some(mechanism.to_owned()),
        lag: Some(Duration::from_secs(lag_days * 86_400)),
        confidence: Confidence::new(confidence).expect("confidence"),
        evidence: evidence.into_iter().map(SourceId::new).collect(),
        counterfactual_notes: counterfactual_notes
            .into_iter()
            .map(str::to_owned)
            .collect(),
    }
}
