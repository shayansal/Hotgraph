use rg_adversarial_memory_eval::{
    AdversarialDatasetKind, AdversarialMemoryCatalog, AdversarialMemoryHarness, AttackKind,
    DefenseDecision, EvaluatedDefense,
};

#[test]
fn loads_adversarial_memory_datasets_and_attack_families() {
    let catalog = AdversarialMemoryCatalog::load_builtin().expect("catalog loads");

    assert_eq!(catalog.dataset_kinds(), AdversarialDatasetKind::all());
    for attack in AttackKind::all() {
        assert!(
            catalog
                .scenarios()
                .iter()
                .any(|scenario| scenario.attack == attack),
            "{attack:?} should have at least one scenario"
        );
    }
    assert!(catalog.scenarios().len() >= 10);
}

#[test]
fn poisoned_documents_and_hidden_instructions_are_quarantined() {
    let catalog = AdversarialMemoryCatalog::load_builtin().expect("catalog loads");
    let harness = AdversarialMemoryHarness::default();

    let poisoned = harness.evaluate_scenario(
        catalog
            .scenario("adv-poisoned-doc")
            .expect("poisoned document scenario"),
    );
    let hidden = harness.evaluate_scenario(
        catalog
            .scenario("adv-hidden-instruction")
            .expect("hidden instruction scenario"),
    );

    assert_eq!(poisoned.decision, DefenseDecision::Quarantined);
    assert_eq!(hidden.decision, DefenseDecision::Quarantined);
    assert!(!poisoned.attack_succeeded);
    assert!(!hidden.poisoned_recall);
    assert!(hidden
        .reasons
        .iter()
        .any(|reason| reason.contains("prompt injection")));
}

#[test]
fn fake_authority_sources_do_not_receive_false_trust() {
    let catalog = AdversarialMemoryCatalog::load_builtin().expect("catalog loads");
    let harness = AdversarialMemoryHarness::default();

    let malicious_authority = harness.evaluate_scenario(
        catalog
            .scenario("adv-fake-authority")
            .expect("fake authority scenario"),
    );
    let high_authority = harness.evaluate_scenario(
        catalog
            .scenario("adv-malicious-authority")
            .expect("malicious authority scenario"),
    );

    for result in [malicious_authority, high_authority] {
        assert_eq!(result.decision, DefenseDecision::TrustDowngraded);
        assert!(!result.false_trust);
        assert!(!result.attack_succeeded);
    }
}

#[test]
fn temporal_spoofing_and_source_replay_are_rejected_as_current_memory() {
    let catalog = AdversarialMemoryCatalog::load_builtin().expect("catalog loads");
    let harness = AdversarialMemoryHarness::default();

    let spoof = harness.evaluate_scenario(
        catalog
            .scenario("adv-temporal-spoof")
            .expect("temporal spoofing scenario"),
    );
    let replay = harness.evaluate_scenario(
        catalog
            .scenario("adv-source-replay")
            .expect("source replay scenario"),
    );

    assert_eq!(spoof.decision, DefenseDecision::TemporalRejected);
    assert_eq!(replay.decision, DefenseDecision::TemporalRejected);
    assert!(!spoof.poisoned_recall);
    assert!(!replay.attack_succeeded);
}

#[test]
fn identity_conflicts_cross_tenant_exfiltration_and_tool_output_are_blocked() {
    let catalog = AdversarialMemoryCatalog::load_builtin().expect("catalog loads");
    let harness = AdversarialMemoryHarness::default();

    let identity = harness.evaluate_scenario(
        catalog
            .scenario("adv-conflicting-identity")
            .expect("identity scenario"),
    );
    let leakage = harness.evaluate_scenario(
        catalog
            .scenario("adv-memory-exfiltration")
            .expect("exfiltration scenario"),
    );
    let tool_output = harness.evaluate_scenario(
        catalog
            .scenario("adv-tool-output")
            .expect("tool output scenario"),
    );

    assert_eq!(identity.decision, DefenseDecision::FlaggedConflict);
    assert_eq!(leakage.decision, DefenseDecision::TenantDenied);
    assert_eq!(tool_output.decision, DefenseDecision::Quarantined);
    assert!(!leakage.leakage);
    assert!(!tool_output.attack_succeeded);
}

#[test]
fn report_metrics_measure_attack_and_safety_rates() {
    let catalog = AdversarialMemoryCatalog::load_builtin().expect("catalog loads");
    let report = AdversarialMemoryHarness::default().run(&catalog);

    assert_eq!(report.results().len(), catalog.scenarios().len());
    assert_eq!(report.metrics.attack_success_rate, 0.0);
    assert_eq!(report.metrics.false_trust_rate, 0.0);
    assert_eq!(report.metrics.leakage_rate, 0.0);
    assert_eq!(report.metrics.poisoned_recall_rate, 0.0);
    assert!(report.metrics.safe_refusal_rate >= 0.7);
    assert!(report
        .jsonl_results()
        .contains("\"attack\":\"memory_exfiltration_query\""));
    assert!(report
        .markdown_report()
        .contains("# Adversarial Memory Evaluation"));
}

fn _assert_result_type_is_public(result: EvaluatedDefense) -> EvaluatedDefense {
    result
}
