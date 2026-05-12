use rg_core::{AssertionId, EventId, SourceId, TenantId, TxTime};
use rg_lab_deploy::{
    ArtifactKind, BuildProfile, ClusterHealthReport, DataExportManifest, DeterministicBuildProfile,
    LabDeploymentMode, Migration, MigrationOperation, MigrationSimulator, OfflineArtifactBundle,
    ReproducibilityManifest, RollbackPlan, SchemaField, SchemaRegistry, VersionPin,
};

#[test]
fn deterministic_build_profile_pins_versions_and_disables_model_provider_lock_in() {
    let profile = DeterministicBuildProfile::paper_freeze("salehi-test-2026")
        .pin(VersionPin::new("reality-graph", "0.1.0", "sha256:rg"))
        .pin(VersionPin::new("rust", "1.78.0", "sha256:rust"))
        .with_mode(LabDeploymentMode::Offline)
        .model_provider_independent();

    assert_eq!(profile.name, "salehi-test-2026");
    assert_eq!(profile.build_profile, BuildProfile::ReproducibleRelease);
    assert_eq!(profile.mode, LabDeploymentMode::Offline);
    assert!(profile.offline_mode);
    assert!(profile.model_provider_independent);
    assert!(profile.exact_version_pinning);
    assert_eq!(
        profile
            .version_pins
            .iter()
            .map(|pin| pin.component.as_str())
            .collect::<Vec<_>>(),
        vec!["reality-graph", "rust"]
    );
}

#[test]
fn schema_registry_tracks_compatible_and_breaking_migrations() {
    let registry = SchemaRegistry::new()
        .register(
            "graph",
            1,
            vec![
                SchemaField::required("assertion.id", "AssertionId"),
                SchemaField::required("assertion.source_ids", "Vec<SourceId>"),
            ],
        )
        .register(
            "graph",
            2,
            vec![
                SchemaField::required("assertion.id", "AssertionId"),
                SchemaField::required("assertion.source_ids", "Vec<SourceId>"),
                SchemaField::optional("assertion.review_status", "ReviewStatus"),
            ],
        );

    let compatible = Migration::new("add-review-status", "graph", 1, 2).operation(
        MigrationOperation::AddOptionalField {
            name: "assertion.review_status".to_owned(),
            type_name: "ReviewStatus".to_owned(),
        },
    );
    let breaking = Migration::new("drop-source-ids", "graph", 2, 3).operation(
        MigrationOperation::RemoveField {
            name: "assertion.source_ids".to_owned(),
        },
    );

    let simulator = MigrationSimulator::new(registry);
    let compatible_report = simulator
        .simulate(&compatible)
        .expect("compatible migration");
    let breaking_report = simulator.simulate(&breaking).expect("breaking migration");

    assert!(compatible_report.allowed_for_lts);
    assert!(compatible_report.rollback_safe);
    assert_eq!(compatible_report.warnings, Vec::<String>::new());
    assert!(!breaking_report.allowed_for_lts);
    assert!(!breaking_report.rollback_safe);
    assert!(breaking_report
        .warnings
        .contains(&"removing fields is a breaking migration".to_owned()));
}

#[test]
fn rollback_plan_restores_previous_schema_snapshot_and_event_cursor() {
    let plan = RollbackPlan::new("rollback-paper-run")
        .from_version("0.2.0")
        .to_version("0.1.0")
        .restore_schema("graph", 1)
        .restore_snapshot("snapshot-2026-05-12")
        .truncate_events_after(EventId::new("evt-000120"))
        .require_audit_event("audit-rollback-1");

    let report = plan.validate();

    assert!(report.rollback_ready);
    assert_eq!(report.target_version, "0.1.0");
    assert_eq!(report.restored_schemas.get("graph"), Some(&1));
    assert_eq!(report.event_cursor, Some(EventId::new("evt-000120")));
    assert!(report.audit_requirements_met);
}

#[test]
fn offline_artifact_bundle_contains_everything_needed_to_reproduce_a_paper_run() {
    let profile = DeterministicBuildProfile::paper_freeze("temporal-graphrag")
        .pin(VersionPin::new("reality-graph", "0.1.0", "sha256:rg"));
    let manifest = ReproducibilityManifest::new("paper-run-001", profile)
        .with_seed(42)
        .with_dataset("temporal_employment", "sha256:dataset")
        .with_graph_snapshot("snapshot-temporal", "sha256:snapshot")
        .with_event_log("events-temporal", "sha256:events")
        .with_eval_report("frontier-eval", "sha256:eval");

    let bundle = OfflineArtifactBundle::new("bundle-paper-run-001")
        .with_manifest(manifest)
        .artifact(
            ArtifactKind::SourceArchive,
            "reality-graph-src.tar.zst",
            "sha256:src",
        )
        .artifact(
            ArtifactKind::ContainerImage,
            "reality-graph-api.oci",
            "sha256:image",
        )
        .artifact(
            ArtifactKind::Dataset,
            "temporal_employment.tsv",
            "sha256:dataset",
        )
        .artifact(
            ArtifactKind::GraphSnapshot,
            "snapshot-temporal.rg",
            "sha256:snapshot",
        )
        .artifact(
            ArtifactKind::EventLog,
            "events-temporal.rglog",
            "sha256:events",
        )
        .artifact(
            ArtifactKind::EvalReport,
            "frontier-eval.jsonl",
            "sha256:eval",
        );

    let validation = bundle.validate_for_offline_replay();

    assert!(validation.reproducible);
    assert!(validation.missing_artifacts.is_empty());
    assert!(validation
        .included_capabilities
        .contains(&"offline replay".to_owned()));
    assert!(validation
        .included_capabilities
        .contains(&"benchmark reproducibility".to_owned()));
}

#[test]
fn reproducibility_manifest_audits_every_memory_and_evidence_decision() {
    let profile = DeterministicBuildProfile::paper_freeze("memory-turing-test");
    let manifest = ReproducibilityManifest::new("memory-paper-run", profile)
        .with_seed(7)
        .record_decision(
            "memory-write-1",
            Some(AssertionId::new("assertion-memory")),
            vec![SourceId::new("source-memory")],
            TxTime::new(100),
        )
        .record_decision(
            "evidence-pack-1",
            Some(AssertionId::new("assertion-memory")),
            vec![SourceId::new("source-memory")],
            TxTime::new(101),
        );

    let audit = manifest.audit_coverage();

    assert_eq!(audit.total_decisions, 2);
    assert_eq!(audit.decisions_with_sources, 2);
    assert_eq!(audit.decisions_with_assertions, 2);
    assert!(audit.every_decision_auditable);
}

#[test]
fn cluster_health_report_marks_lab_freeze_readiness() {
    let report = ClusterHealthReport::new("lab-cluster-a")
        .node("api-0", true, "0.1.0")
        .node("worker-0", true, "0.1.0")
        .deterministic_replay(true)
        .schema_version("graph", 1)
        .offline_artifacts_available(true)
        .last_backup("backup-2026-05-12")
        .last_restore_test("restore-2026-05-12")
        .tenant(TenantId::new("tenant-lab"));

    assert!(report.ready_for_lab_freeze());
    assert_eq!(report.version_skew(), None);
    assert_eq!(report.tenants, vec![TenantId::new("tenant-lab")]);
}

#[test]
fn data_export_manifest_is_stable_and_labels_all_exported_material() {
    let export = DataExportManifest::new("export-paper-001")
        .include_sources(false)
        .artifact(
            ArtifactKind::GraphSnapshot,
            "snapshot.rg",
            "sha256:snapshot",
        )
        .artifact(ArtifactKind::EvalReport, "report.md", "sha256:report")
        .redaction("source-raw-text", "no-raw-source paper export");

    assert!(!export.includes_raw_sources);
    assert_eq!(export.artifacts.len(), 2);
    assert_eq!(export.redactions.len(), 1);
    assert_eq!(
        export.stable_listing(),
        "export-paper-001|GraphSnapshot:snapshot.rg:sha256:snapshot|EvalReport:report.md:sha256:report|redacted:source-raw-text:no-raw-source paper export"
    );
}
