use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use rg_core::{
    AssertionId, Confidence, ContentHash, ContextScope, EntityId, EntityType, GraphValue,
    PredicateId, PropertyMap, SourceId, SourceType, TimeInterval, TxTime, ValidTime,
};
use rg_events::{AddAssertion, AddSource, CreateEntity, EventLog, GraphCommand};
use rg_storage::{deterministic_state_hash, RedbGraphStore};

#[test]
fn hotgraph_backup_create_verify_and_restore_into_clean_redb_target() {
    let source_store = temp_file("source.redb");
    let backup_path = temp_file("backup.hotgraph");
    let restore_dir = temp_dir("restore-target");
    let restored_store = restore_dir.join("hotgraph.redb");
    seed_redb_store(&source_store);

    assert_command_ok(
        Command::new(hotgraph_bin())
            .arg("backup")
            .arg("create")
            .arg("--store")
            .arg(&source_store)
            .arg("--output")
            .arg(&backup_path),
    );

    assert_command_ok(
        Command::new(hotgraph_bin())
            .arg("backup")
            .arg("verify")
            .arg("--input")
            .arg(&backup_path),
    );

    assert_command_ok(
        Command::new(hotgraph_bin())
            .arg("restore")
            .arg("--input")
            .arg(&backup_path)
            .arg("--target")
            .arg(&restore_dir),
    );

    assert!(
        restored_store.exists(),
        "restore should create hotgraph.redb"
    );
    let source_hash = redb_state_hash(&source_store);
    let restored_hash = redb_state_hash(&restored_store);
    assert_eq!(restored_hash, source_hash);

    assert_command_ok(
        Command::new(hotgraph_bin())
            .arg("restore")
            .arg("verify")
            .arg("--input")
            .arg(&backup_path),
    );
}

#[test]
fn hotgraph_restore_refuses_non_empty_target_directory() {
    let source_store = temp_file("source-non-empty.redb");
    let backup_path = temp_file("non-empty-backup.hotgraph");
    let restore_dir = temp_dir("restore-non-empty");
    fs::write(restore_dir.join("existing-file"), b"do not overwrite").expect("seed target");
    seed_redb_store(&source_store);

    assert_command_ok(
        Command::new(hotgraph_bin())
            .arg("backup")
            .arg("create")
            .arg("--store")
            .arg(&source_store)
            .arg("--output")
            .arg(&backup_path),
    );

    let output = Command::new(hotgraph_bin())
        .arg("restore")
        .arg("--input")
        .arg(&backup_path)
        .arg("--target")
        .arg(&restore_dir)
        .output()
        .expect("run hotgraph restore");

    assert!(!output.status.success(), "restore should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("target directory must be empty"),
        "{stderr}"
    );
}

fn seed_redb_store(path: &Path) {
    let mut store = RedbGraphStore::create(path).expect("create redb store");
    for event in sample_events() {
        store.append_event(&event, None).expect("append event");
    }
}

fn redb_state_hash(path: &Path) -> String {
    let store = RedbGraphStore::open(path).expect("open redb store");
    let storage = store.materialized_storage().expect("materialized state");
    deterministic_state_hash(&storage)
}

fn sample_events() -> Vec<rg_events::GraphEvent> {
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

fn hotgraph_bin() -> PathBuf {
    std::env::var_os("CARGO_BIN_EXE_hotgraph")
        .map(PathBuf::from)
        .expect("hotgraph binary should be built for integration tests")
}

fn assert_command_ok(command: &mut Command) {
    let output = command.output().expect("run hotgraph command");
    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn temp_file(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "hotgraph-cli-{name}-{}-{}",
        std::process::id(),
        nanos()
    ));
    let _ = fs::remove_file(&path);
    path
}

fn temp_dir(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "hotgraph-cli-{name}-{}-{}",
        std::process::id(),
        nanos()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("create temp dir");
    path
}

fn nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos()
}
