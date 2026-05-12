use proptest::prelude::*;
use rg_core::{
    Confidence, GraphValue, PredicateId, SourceId, TenantId, TimeInterval, TxTime, ValidTime,
};
use rg_kernel::{
    active_during, visible_at, AiUsage, AtomId, BeliefState, ClaimType, ConflictSet,
    ConflictStatus, ConflictType, DependencyNode, EntityRef, EvidenceSpan, PermissionLabel,
    RealityAtom, RealityKernel, SourceRef, TaintLabel, TransactionTime, ValueOrEntity,
};

fn arb_interval() -> impl Strategy<Value = (i64, Option<i64>)> {
    (0_i64..10_000, prop::option::of(1_i64..1_000)).prop_map(|(start, len)| {
        let end = len.map(|value| start + value);
        (start, end)
    })
}

fn property_atom(
    id: &str,
    valid_start: i64,
    valid_end: Option<i64>,
    tx_start: i64,
    tx_end: Option<i64>,
) -> RealityAtom {
    RealityAtom::builder(
        AtomId::new(id),
        EntityRef::new("entity-a"),
        PredicateId::new("RELATED_TO"),
        ValueOrEntity::Value(GraphValue::Text("entity-b".to_owned())),
    )
    .valid_time(
        TimeInterval::new(ValidTime::new(valid_start), valid_end.map(ValidTime::new))
            .expect("valid interval"),
    )
    .transaction_time(
        TimeInterval::new(TxTime::new(tx_start), tx_end.map(TxTime::new)).expect("tx interval"),
    )
    .claim_type(ClaimType::Assertion)
    .belief_state(BeliefState::Accepted)
    .confidence(Confidence::new(0.9).expect("confidence"))
    .source_ref(SourceRef::new(SourceId::new("source-property")))
    .evidence_span(EvidenceSpan::new(
        SourceId::new("source-property"),
        0,
        16,
        "property evidence",
    ))
    .tenant_id(TenantId::new("tenant-property"))
    .permissions(PermissionLabel::Internal)
    .taint(TaintLabel::Trusted)
    .ai_usage(AiUsage::SafeForPlanning { caveat: None })
    .build()
    .expect("property atom")
}

proptest! {
    #[test]
    fn property_valid_time_containment_matches_interval((start, end) in arb_interval(), offset in -100_i64..1_200) {
        let atom = property_atom("atom-valid", start, end, 10, None);
        let instant = ValidTime::new(start + offset);

        prop_assert_eq!(active_during(&atom, &TimeInterval::new(instant, Some(ValidTime::new(instant.as_i64() + 1))).expect("point-ish interval")), atom.valid_time.overlaps(&TimeInterval::new(instant, Some(ValidTime::new(instant.as_i64() + 1))).expect("point-ish interval")));
        prop_assert_eq!(atom.valid_time.contains(instant), instant >= atom.valid_time.start && atom.valid_time.end.map_or(true, |end| instant < end));
    }

    #[test]
    fn property_transaction_time_containment_matches_interval((tx_start, tx_end) in arb_interval(), offset in -100_i64..1_200) {
        let atom = property_atom("atom-tx", 1, None, tx_start, tx_end);
        let instant = TransactionTime::new(tx_start + offset);

        prop_assert_eq!(super_known_at_for_property(&atom, instant), atom.transaction_time.contains(instant.into()));
    }

    #[test]
    fn property_bitemporal_visibility_requires_both_axes(
        (valid_start, valid_end) in arb_interval(),
        (tx_start, tx_end) in arb_interval(),
        valid_offset in -100_i64..1_200,
        tx_offset in -100_i64..1_200,
    ) {
        let atom = property_atom("atom-visible", valid_start, valid_end, tx_start, tx_end);
        let valid_at = ValidTime::new(valid_start + valid_offset);
        let known_at = TransactionTime::new(tx_start + tx_offset);

        prop_assert_eq!(
            visible_at(&atom, valid_at, known_at),
            atom.valid_time.contains(valid_at) && atom.transaction_time.contains(known_at.into())
        );
    }

    #[test]
    fn property_open_ended_intervals_contain_later_instants(start in 0_i64..10_000, later in 0_i64..10_000) {
        let atom = property_atom("atom-open", start, None, start, None);
        let valid_at = ValidTime::new(start + later);
        let known_at = TransactionTime::new(start + later);

        prop_assert!(visible_at(&atom, valid_at, known_at));
    }

    #[test]
    fn property_dependency_traversal_finds_generated_chain(length in 1_usize..12) {
        let mut kernel = RealityKernel::new();
        for index in 0..=length {
            let mut atom = property_atom(&format!("atom-{index}"), 1, None, index as i64 + 1, None);
            if index > 0 {
                atom = atom.depending_on(vec![AtomId::new(format!("atom-{}", index - 1))]);
            }
            kernel.insert_atom(atom);
        }

        let downstream = kernel.compute_downstream_dependencies(&AtomId::new("atom-0"));
        let terminal_atom_id = AtomId::new(format!("atom-{}", length));
        prop_assert!(downstream.contains(&DependencyNode::Atom(terminal_atom_id)));
    }

    #[test]
    fn property_supersession_marks_old_atom_historical(tx_new in 2_i64..10_000) {
        let mut kernel = RealityKernel::new();
        kernel.insert_atom(property_atom("old-atom", 1, None, 1, None));
        kernel.insert_atom(property_atom("new-atom", 1, None, tx_new, None).superseding(vec![AtomId::new("old-atom")]));

        prop_assert_eq!(kernel.belief_at(&AtomId::new("old-atom"), TxTime::new(tx_new)), Some(BeliefState::Superseded));
        prop_assert_eq!(kernel.belief_at(&AtomId::new("old-atom"), TxTime::new(1)), Some(BeliefState::Accepted));
    }

    #[test]
    fn property_contradiction_overlap_preserves_conflict_set(start in 0_i64..1_000, len in 1_i64..1_000) {
        let mut kernel = RealityKernel::new();
        kernel.insert_atom(property_atom("claim-a", start, Some(start + len + 10), 1, None));
        kernel.insert_atom(property_atom("claim-b", start + len, Some(start + len + 20), 2, None));
        kernel.add_conflict(ConflictSet::new(
            "conflict-property",
            vec![AtomId::new("claim-a"), AtomId::new("claim-b")],
            ConflictType::ValidTimeOverlap,
            ConflictStatus::Unresolved,
            "generated overlap",
        ));

        prop_assert_eq!(kernel.explain_conflict(&AtomId::new("claim-a")).len(), 1);
        prop_assert!(kernel.atom(&AtomId::new("claim-a")).expect("claim").valid_time.overlaps(&kernel.atom(&AtomId::new("claim-b")).expect("claim").valid_time));
    }
}

fn super_known_at_for_property(atom: &RealityAtom, known_at: TransactionTime) -> bool {
    rg_kernel::known_at(atom, known_at)
}
