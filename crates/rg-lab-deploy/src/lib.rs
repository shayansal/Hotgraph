//! Frontier-lab deployment and reproducibility primitives.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rg_core::{AssertionId, EventId, SourceId, TenantId, TxTime};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LabDeploymentMode {
    Online,
    Offline,
    AirGapped,
    PrivateCloud,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuildProfile {
    Debug,
    Release,
    ReproducibleRelease,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VersionPin {
    pub component: String,
    pub version: String,
    pub artifact_digest: String,
}

impl VersionPin {
    pub fn new(
        component: impl Into<String>,
        version: impl Into<String>,
        artifact_digest: impl Into<String>,
    ) -> Self {
        Self {
            component: component.into(),
            version: version.into(),
            artifact_digest: artifact_digest.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeterministicBuildProfile {
    pub name: String,
    pub build_profile: BuildProfile,
    pub mode: LabDeploymentMode,
    pub offline_mode: bool,
    pub exact_version_pinning: bool,
    pub deterministic_replay: bool,
    pub model_provider_independent: bool,
    pub auditability: bool,
    pub benchmark_reproducibility: bool,
    pub version_pins: Vec<VersionPin>,
}

impl DeterministicBuildProfile {
    pub fn paper_freeze(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            build_profile: BuildProfile::ReproducibleRelease,
            mode: LabDeploymentMode::Offline,
            offline_mode: true,
            exact_version_pinning: true,
            deterministic_replay: true,
            model_provider_independent: false,
            auditability: true,
            benchmark_reproducibility: true,
            version_pins: Vec::new(),
        }
    }

    pub fn pin(mut self, pin: VersionPin) -> Self {
        self.version_pins.push(pin);
        self.version_pins
            .sort_by(|left, right| left.component.cmp(&right.component));
        self
    }

    pub fn with_mode(mut self, mode: LabDeploymentMode) -> Self {
        self.offline_mode = matches!(
            mode,
            LabDeploymentMode::Offline | LabDeploymentMode::AirGapped
        );
        self.mode = mode;
        self
    }

    pub fn model_provider_independent(mut self) -> Self {
        self.model_provider_independent = true;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaField {
    pub name: String,
    pub type_name: String,
    pub required: bool,
}

impl SchemaField {
    pub fn required(name: impl Into<String>, type_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            type_name: type_name.into(),
            required: true,
        }
    }

    pub fn optional(name: impl Into<String>, type_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            type_name: type_name.into(),
            required: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VersionedGraphSchema {
    pub name: String,
    pub version: u32,
    pub fields: BTreeMap<String, SchemaField>,
}

impl VersionedGraphSchema {
    fn new(name: impl Into<String>, version: u32, fields: Vec<SchemaField>) -> Self {
        Self {
            name: name.into(),
            version,
            fields: fields
                .into_iter()
                .map(|field| (field.name.clone(), field))
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SchemaRegistry {
    schemas: BTreeMap<(String, u32), VersionedGraphSchema>,
}

impl SchemaRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        mut self,
        name: impl Into<String>,
        version: u32,
        fields: Vec<SchemaField>,
    ) -> Self {
        let name = name.into();
        self.schemas.insert(
            (name.clone(), version),
            VersionedGraphSchema::new(name, version, fields),
        );
        self
    }

    pub fn get(&self, name: &str, version: u32) -> Option<&VersionedGraphSchema> {
        self.schemas.get(&(name.to_owned(), version))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MigrationOperation {
    AddOptionalField { name: String, type_name: String },
    AddRequiredField { name: String, type_name: String },
    RemoveField { name: String },
    RenameField { from: String, to: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Migration {
    pub id: String,
    pub schema_name: String,
    pub from_version: u32,
    pub to_version: u32,
    pub operations: Vec<MigrationOperation>,
}

impl Migration {
    pub fn new(
        id: impl Into<String>,
        schema_name: impl Into<String>,
        from_version: u32,
        to_version: u32,
    ) -> Self {
        Self {
            id: id.into(),
            schema_name: schema_name.into(),
            from_version,
            to_version,
            operations: Vec::new(),
        }
    }

    pub fn operation(mut self, operation: MigrationOperation) -> Self {
        self.operations.push(operation);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LabDeployError {
    UnknownSchema { name: String, version: u32 },
}

impl fmt::Display for LabDeployError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownSchema { name, version } => {
                write!(formatter, "unknown schema {name} version {version}")
            }
        }
    }
}

impl std::error::Error for LabDeployError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationSimulationReport {
    pub migration_id: String,
    pub schema_name: String,
    pub from_version: u32,
    pub to_version: u32,
    pub allowed_for_lts: bool,
    pub rollback_safe: bool,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationSimulator {
    registry: SchemaRegistry,
}

impl MigrationSimulator {
    pub fn new(registry: SchemaRegistry) -> Self {
        Self { registry }
    }

    pub fn simulate(
        &self,
        migration: &Migration,
    ) -> Result<MigrationSimulationReport, LabDeployError> {
        let Some(from_schema) = self
            .registry
            .get(&migration.schema_name, migration.from_version)
        else {
            return Err(LabDeployError::UnknownSchema {
                name: migration.schema_name.clone(),
                version: migration.from_version,
            });
        };

        let mut warnings = Vec::new();
        let mut allowed_for_lts = true;
        let mut rollback_safe = true;
        for operation in &migration.operations {
            match operation {
                MigrationOperation::AddOptionalField { .. } => {}
                MigrationOperation::AddRequiredField { name, .. } => {
                    allowed_for_lts = false;
                    warnings.push(format!("adding required field {name} requires backfill"));
                }
                MigrationOperation::RemoveField { name } => {
                    allowed_for_lts = false;
                    rollback_safe = false;
                    if from_schema.fields.contains_key(name) {
                        warnings.push("removing fields is a breaking migration".to_owned());
                    } else {
                        warnings.push(format!("removing unknown field {name} is unsafe"));
                    }
                }
                MigrationOperation::RenameField { from, to } => {
                    allowed_for_lts = false;
                    rollback_safe = false;
                    warnings.push(format!(
                        "renaming {from} to {to} requires compatibility shim"
                    ));
                }
            }
        }
        warnings.sort();
        warnings.dedup();

        Ok(MigrationSimulationReport {
            migration_id: migration.id.clone(),
            schema_name: migration.schema_name.clone(),
            from_version: migration.from_version,
            to_version: migration.to_version,
            allowed_for_lts,
            rollback_safe,
            warnings,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RollbackPlan {
    pub id: String,
    pub from_version: String,
    pub to_version: String,
    restored_schemas: BTreeMap<String, u32>,
    snapshot_id: Option<String>,
    event_cursor: Option<EventId>,
    audit_event_id: Option<String>,
}

impl RollbackPlan {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            from_version: String::new(),
            to_version: String::new(),
            restored_schemas: BTreeMap::new(),
            snapshot_id: None,
            event_cursor: None,
            audit_event_id: None,
        }
    }

    pub fn from_version(mut self, version: impl Into<String>) -> Self {
        self.from_version = version.into();
        self
    }

    pub fn to_version(mut self, version: impl Into<String>) -> Self {
        self.to_version = version.into();
        self
    }

    pub fn restore_schema(mut self, schema_name: impl Into<String>, version: u32) -> Self {
        self.restored_schemas.insert(schema_name.into(), version);
        self
    }

    pub fn restore_snapshot(mut self, snapshot_id: impl Into<String>) -> Self {
        self.snapshot_id = Some(snapshot_id.into());
        self
    }

    pub fn truncate_events_after(mut self, event_id: EventId) -> Self {
        self.event_cursor = Some(event_id);
        self
    }

    pub fn require_audit_event(mut self, audit_event_id: impl Into<String>) -> Self {
        self.audit_event_id = Some(audit_event_id.into());
        self
    }

    pub fn validate(&self) -> RollbackValidationReport {
        let rollback_ready = !self.from_version.is_empty()
            && !self.to_version.is_empty()
            && !self.restored_schemas.is_empty()
            && self.snapshot_id.is_some()
            && self.event_cursor.is_some()
            && self.audit_event_id.is_some();
        RollbackValidationReport {
            rollback_ready,
            target_version: self.to_version.clone(),
            restored_schemas: self.restored_schemas.clone(),
            event_cursor: self.event_cursor.clone(),
            audit_requirements_met: self.audit_event_id.is_some(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RollbackValidationReport {
    pub rollback_ready: bool,
    pub target_version: String,
    pub restored_schemas: BTreeMap<String, u32>,
    pub event_cursor: Option<EventId>,
    pub audit_requirements_met: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ArtifactKind {
    SourceArchive,
    ContainerImage,
    Dataset,
    GraphSnapshot,
    EventLog,
    EvalReport,
    Schema,
    Manifest,
}

impl ArtifactKind {
    pub fn slug(self) -> &'static str {
        match self {
            Self::SourceArchive => "SourceArchive",
            Self::ContainerImage => "ContainerImage",
            Self::Dataset => "Dataset",
            Self::GraphSnapshot => "GraphSnapshot",
            Self::EventLog => "EventLog",
            Self::EvalReport => "EvalReport",
            Self::Schema => "Schema",
            Self::Manifest => "Manifest",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactRecord {
    pub kind: ArtifactKind,
    pub path: String,
    pub digest: String,
}

impl ArtifactRecord {
    fn new(kind: ArtifactKind, path: impl Into<String>, digest: impl Into<String>) -> Self {
        Self {
            kind,
            path: path.into(),
            digest: digest.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditDecisionRecord {
    pub decision_id: String,
    pub assertion_id: Option<AssertionId>,
    pub source_ids: Vec<SourceId>,
    pub transaction_time: TxTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReproducibilityManifest {
    pub run_id: String,
    pub profile: DeterministicBuildProfile,
    pub seed: Option<u64>,
    pub datasets: Vec<ArtifactRecord>,
    pub graph_snapshots: Vec<ArtifactRecord>,
    pub event_logs: Vec<ArtifactRecord>,
    pub eval_reports: Vec<ArtifactRecord>,
    pub audit_decisions: Vec<AuditDecisionRecord>,
}

impl ReproducibilityManifest {
    pub fn new(run_id: impl Into<String>, profile: DeterministicBuildProfile) -> Self {
        Self {
            run_id: run_id.into(),
            profile,
            seed: None,
            datasets: Vec::new(),
            graph_snapshots: Vec::new(),
            event_logs: Vec::new(),
            eval_reports: Vec::new(),
            audit_decisions: Vec::new(),
        }
    }

    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    pub fn with_dataset(mut self, path: impl Into<String>, digest: impl Into<String>) -> Self {
        self.datasets
            .push(ArtifactRecord::new(ArtifactKind::Dataset, path, digest));
        self
    }

    pub fn with_graph_snapshot(
        mut self,
        path: impl Into<String>,
        digest: impl Into<String>,
    ) -> Self {
        self.graph_snapshots.push(ArtifactRecord::new(
            ArtifactKind::GraphSnapshot,
            path,
            digest,
        ));
        self
    }

    pub fn with_event_log(mut self, path: impl Into<String>, digest: impl Into<String>) -> Self {
        self.event_logs
            .push(ArtifactRecord::new(ArtifactKind::EventLog, path, digest));
        self
    }

    pub fn with_eval_report(mut self, path: impl Into<String>, digest: impl Into<String>) -> Self {
        self.eval_reports
            .push(ArtifactRecord::new(ArtifactKind::EvalReport, path, digest));
        self
    }

    pub fn record_decision(
        mut self,
        decision_id: impl Into<String>,
        assertion_id: Option<AssertionId>,
        source_ids: Vec<SourceId>,
        transaction_time: TxTime,
    ) -> Self {
        self.audit_decisions.push(AuditDecisionRecord {
            decision_id: decision_id.into(),
            assertion_id,
            source_ids,
            transaction_time,
        });
        self
    }

    pub fn audit_coverage(&self) -> AuditCoverageReport {
        let total_decisions = self.audit_decisions.len();
        let decisions_with_sources = self
            .audit_decisions
            .iter()
            .filter(|decision| !decision.source_ids.is_empty())
            .count();
        let decisions_with_assertions = self
            .audit_decisions
            .iter()
            .filter(|decision| decision.assertion_id.is_some())
            .count();
        AuditCoverageReport {
            total_decisions,
            decisions_with_sources,
            decisions_with_assertions,
            every_decision_auditable: total_decisions > 0
                && decisions_with_sources == total_decisions
                && decisions_with_assertions == total_decisions,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditCoverageReport {
    pub total_decisions: usize,
    pub decisions_with_sources: usize,
    pub decisions_with_assertions: usize,
    pub every_decision_auditable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfflineArtifactBundle {
    pub id: String,
    pub manifest: Option<ReproducibilityManifest>,
    pub artifacts: Vec<ArtifactRecord>,
}

impl OfflineArtifactBundle {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            manifest: None,
            artifacts: Vec::new(),
        }
    }

    pub fn with_manifest(mut self, manifest: ReproducibilityManifest) -> Self {
        self.manifest = Some(manifest);
        self
    }

    pub fn artifact(
        mut self,
        kind: ArtifactKind,
        path: impl Into<String>,
        digest: impl Into<String>,
    ) -> Self {
        self.artifacts.push(ArtifactRecord::new(kind, path, digest));
        self
    }

    pub fn validate_for_offline_replay(&self) -> OfflineBundleValidation {
        let present = self
            .artifacts
            .iter()
            .map(|artifact| artifact.kind)
            .collect::<BTreeSet<_>>();
        let required = [
            ArtifactKind::SourceArchive,
            ArtifactKind::ContainerImage,
            ArtifactKind::Dataset,
            ArtifactKind::GraphSnapshot,
            ArtifactKind::EventLog,
            ArtifactKind::EvalReport,
        ];
        let mut missing_artifacts = required
            .iter()
            .filter(|kind| !present.contains(kind))
            .map(|kind| kind.slug().to_owned())
            .collect::<Vec<_>>();
        if self.manifest.is_none() {
            missing_artifacts.push("ReproducibilityManifest".to_owned());
        }
        missing_artifacts.sort();
        let reproducible = missing_artifacts.is_empty()
            && self
                .manifest
                .as_ref()
                .is_some_and(|manifest| manifest.seed.is_some());
        let mut included_capabilities = Vec::new();
        if present.contains(&ArtifactKind::SourceArchive)
            && present.contains(&ArtifactKind::ContainerImage)
        {
            included_capabilities.push("offline replay".to_owned());
        }
        if present.contains(&ArtifactKind::Dataset) && present.contains(&ArtifactKind::EvalReport) {
            included_capabilities.push("benchmark reproducibility".to_owned());
        }
        if present.contains(&ArtifactKind::GraphSnapshot)
            && present.contains(&ArtifactKind::EventLog)
        {
            included_capabilities.push("deterministic replay".to_owned());
        }
        included_capabilities.sort();
        OfflineBundleValidation {
            reproducible,
            missing_artifacts,
            included_capabilities,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfflineBundleValidation {
    pub reproducible: bool,
    pub missing_artifacts: Vec<String>,
    pub included_capabilities: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataExportManifest {
    pub id: String,
    pub includes_raw_sources: bool,
    pub artifacts: Vec<ArtifactRecord>,
    pub redactions: Vec<ExportRedaction>,
}

impl DataExportManifest {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            includes_raw_sources: false,
            artifacts: Vec::new(),
            redactions: Vec::new(),
        }
    }

    pub fn include_sources(mut self, include_sources: bool) -> Self {
        self.includes_raw_sources = include_sources;
        self
    }

    pub fn artifact(
        mut self,
        kind: ArtifactKind,
        path: impl Into<String>,
        digest: impl Into<String>,
    ) -> Self {
        self.artifacts.push(ArtifactRecord::new(kind, path, digest));
        self.artifacts
            .sort_by(|left, right| left.kind.cmp(&right.kind).then(left.path.cmp(&right.path)));
        self
    }

    pub fn redaction(mut self, target: impl Into<String>, reason: impl Into<String>) -> Self {
        self.redactions.push(ExportRedaction {
            target: target.into(),
            reason: reason.into(),
        });
        self.redactions
            .sort_by(|left, right| left.target.cmp(&right.target));
        self
    }

    pub fn stable_listing(&self) -> String {
        let mut parts = vec![self.id.clone()];
        parts.extend(self.artifacts.iter().map(|artifact| {
            format!(
                "{}:{}:{}",
                artifact.kind.slug(),
                artifact.path,
                artifact.digest
            )
        }));
        parts.extend(
            self.redactions
                .iter()
                .map(|redaction| format!("redacted:{}:{}", redaction.target, redaction.reason)),
        );
        parts.join("|")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportRedaction {
    pub target: String,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClusterNodeHealth {
    pub name: String,
    pub healthy: bool,
    pub version: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClusterHealthReport {
    pub cluster_id: String,
    pub nodes: Vec<ClusterNodeHealth>,
    pub deterministic_replay_ok: bool,
    pub schema_versions: BTreeMap<String, u32>,
    pub offline_artifacts_available: bool,
    pub last_backup_id: Option<String>,
    pub last_restore_test_id: Option<String>,
    pub tenants: Vec<TenantId>,
}

impl ClusterHealthReport {
    pub fn new(cluster_id: impl Into<String>) -> Self {
        Self {
            cluster_id: cluster_id.into(),
            nodes: Vec::new(),
            deterministic_replay_ok: false,
            schema_versions: BTreeMap::new(),
            offline_artifacts_available: false,
            last_backup_id: None,
            last_restore_test_id: None,
            tenants: Vec::new(),
        }
    }

    pub fn node(
        mut self,
        name: impl Into<String>,
        healthy: bool,
        version: impl Into<String>,
    ) -> Self {
        self.nodes.push(ClusterNodeHealth {
            name: name.into(),
            healthy,
            version: version.into(),
        });
        self.nodes.sort_by(|left, right| left.name.cmp(&right.name));
        self
    }

    pub fn deterministic_replay(mut self, deterministic_replay_ok: bool) -> Self {
        self.deterministic_replay_ok = deterministic_replay_ok;
        self
    }

    pub fn schema_version(mut self, schema_name: impl Into<String>, version: u32) -> Self {
        self.schema_versions.insert(schema_name.into(), version);
        self
    }

    pub fn offline_artifacts_available(mut self, available: bool) -> Self {
        self.offline_artifacts_available = available;
        self
    }

    pub fn last_backup(mut self, backup_id: impl Into<String>) -> Self {
        self.last_backup_id = Some(backup_id.into());
        self
    }

    pub fn last_restore_test(mut self, restore_test_id: impl Into<String>) -> Self {
        self.last_restore_test_id = Some(restore_test_id.into());
        self
    }

    pub fn tenant(mut self, tenant_id: TenantId) -> Self {
        self.tenants.push(tenant_id);
        self.tenants.sort();
        self.tenants.dedup();
        self
    }

    pub fn version_skew(&self) -> Option<Vec<String>> {
        let versions = self
            .nodes
            .iter()
            .map(|node| node.version.clone())
            .collect::<BTreeSet<_>>();
        (versions.len() > 1).then(|| versions.into_iter().collect())
    }

    pub fn ready_for_lab_freeze(&self) -> bool {
        !self.nodes.is_empty()
            && self.nodes.iter().all(|node| node.healthy)
            && self.version_skew().is_none()
            && self.deterministic_replay_ok
            && !self.schema_versions.is_empty()
            && self.offline_artifacts_available
            && self.last_backup_id.is_some()
            && self.last_restore_test_id.is_some()
    }
}
