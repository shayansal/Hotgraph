use std::fs;
use std::path::PathBuf;

use rg_ai::{EvidencePack, SourceExcerpt};
use rg_confidential::{
    AnalyticsMetric, AnalyticsObservation, ConfidentialQueryPolicy, EncryptedEventLog,
    EncryptedSnapshotStore, EncryptedSourceStore, EnvelopeEncryptor, KeyId, KeyRing,
    LocalDevKmsProvider, PrivacyAnalyticsQuery, PrivateAnalyticsEngine, RedactionAwareQueryEngine,
    TenantKey,
};
#[cfg(feature = "aws-kms")]
use rg_confidential::{
    AwsGeneratedDataKey, AwsKmsClient, AwsKmsProvider, EncryptedDataKey, KmsProvider,
};
use rg_core::{
    Assertion, AssertionId, AssertionStatus, Confidence, ContentHash, ContextScope, Entity,
    EntityId, EntityType, GraphValue, PredicateId, PropertyMap, SourceId, SourceType, TenantId,
    TimeInterval, TxTime, ValidTime,
};

fn temp_file(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "reality-graph-confidential-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_file(&path);
    path
}

fn key_ring(active: &str, material: [u8; 32]) -> KeyRing {
    KeyRing::new(TenantKey::new(KeyId::new(active), material))
}

#[test]
fn encrypted_event_log_round_trips_without_plaintext_leakage() {
    let path = temp_file("event-log");
    let mut log =
        EncryptedEventLog::open(&path, key_ring("key-a", [7; 32])).expect("encrypted log opens");

    let first_header = log
        .append_record(b"AssertionAdded:source-secret:Person A worked at Lab B")
        .expect("append first record");
    let second_header = log
        .append_record(b"SourceAdded:file://classified-source.txt")
        .expect("append second record");

    assert_eq!(first_header.sequence, 1);
    assert_eq!(second_header.sequence, 2);
    assert_eq!(first_header.key_id, KeyId::new("key-a"));

    let bytes_on_disk = fs::read_to_string(&path).expect("read encrypted file");
    assert!(!bytes_on_disk.contains("AssertionAdded"));
    assert!(!bytes_on_disk.contains("classified-source"));

    let recovered = log.read_all().expect("decrypt log records");
    assert_eq!(
        recovered,
        vec![
            b"AssertionAdded:source-secret:Person A worked at Lab B".to_vec(),
            b"SourceAdded:file://classified-source.txt".to_vec()
        ]
    );

    fs::remove_file(path).expect("cleanup");
}

#[test]
fn envelope_encryptor_detects_tamper_wrong_key_and_wrong_associated_data() {
    let encryptor = EnvelopeEncryptor::new(LocalDevKmsProvider::new("dev-master"));
    let envelope = encryptor
        .encrypt("tenant-a", "event-log", b"source-backed secret")
        .expect("encrypt envelope");

    assert_eq!(envelope.algorithm, "XChaCha20Poly1305");
    assert_eq!(envelope.encrypted_data_key.key_id, KeyId::new("dev-master"));
    assert!(!envelope.ciphertext.is_empty());
    assert_ne!(envelope.ciphertext, b"source-backed secret");
    assert_eq!(
        encryptor
            .decrypt("tenant-a", "event-log", &envelope)
            .expect("decrypt envelope"),
        b"source-backed secret"
    );

    let mut tampered = envelope.clone();
    tampered.ciphertext[0] ^= 0x80;
    assert!(matches!(
        encryptor.decrypt("tenant-a", "event-log", &tampered),
        Err(rg_confidential::ConfidentialError::AuthenticationFailed)
    ));

    assert!(matches!(
        encryptor.decrypt("tenant-a", "snapshot", &envelope),
        Err(rg_confidential::ConfidentialError::AuthenticationFailed)
    ));

    let wrong_encryptor = EnvelopeEncryptor::new(LocalDevKmsProvider::new("other-master"));
    assert!(matches!(
        wrong_encryptor.decrypt("tenant-a", "event-log", &envelope),
        Err(rg_confidential::ConfidentialError::MissingKey(_))
    ));
}

#[test]
fn production_envelope_encryptor_rejects_local_development_kms() {
    let error = EnvelopeEncryptor::new_production(LocalDevKmsProvider::new("dev-master"))
        .expect_err("local development KMS must not be accepted in production mode");

    assert!(
        matches!(error, rg_confidential::ConfidentialError::Codec(message) if message.contains("LocalDevKmsProvider"))
    );
}

#[cfg(feature = "aws-kms")]
#[test]
fn aws_kms_provider_uses_sdk_client_for_data_key_unwrap_health_and_rotation() {
    #[derive(Clone, Debug)]
    struct MockAwsClient {
        key_id: String,
    }

    impl AwsKmsClient for MockAwsClient {
        fn generate_data_key(
            &self,
            key_id: &str,
            tenant_id: &str,
            purpose: &str,
        ) -> Result<AwsGeneratedDataKey, rg_confidential::ConfidentialError> {
            assert_eq!(key_id, self.key_id);
            assert_eq!(tenant_id, "tenant-a");
            assert_eq!(purpose, "event-log");
            Ok(AwsGeneratedDataKey {
                plaintext: [42; 32],
                ciphertext_blob: b"aws-kms-ciphertext".to_vec(),
            })
        }

        fn decrypt_data_key(
            &self,
            key_id: &str,
            tenant_id: &str,
            purpose: &str,
            encrypted: &EncryptedDataKey,
        ) -> Result<[u8; 32], rg_confidential::ConfidentialError> {
            assert_eq!(key_id, self.key_id);
            assert_eq!(tenant_id, "tenant-a");
            assert_eq!(purpose, "event-log");
            assert_eq!(encrypted.ciphertext, b"aws-kms-ciphertext");
            Ok([42; 32])
        }

        fn describe_key(&self, key_id: &str) -> Result<(), rg_confidential::ConfidentialError> {
            assert_eq!(key_id, self.key_id);
            Ok(())
        }
    }

    let provider = AwsKmsProvider::with_client(
        "arn:aws:kms:us-east-1:123456789012:key/hotgraph",
        "us-east-1",
        MockAwsClient {
            key_id: "arn:aws:kms:us-east-1:123456789012:key/hotgraph".to_owned(),
        },
    );
    let encryptor = EnvelopeEncryptor::new_production(provider.clone())
        .expect("AWS KMS provider is accepted for production");

    let envelope = encryptor
        .encrypt("tenant-a", "event-log", b"production secret")
        .expect("encrypt with AWS KMS client");
    assert_eq!(
        envelope.encrypted_data_key.key_id,
        KeyId::new("arn:aws:kms:us-east-1:123456789012:key/hotgraph")
    );
    assert_eq!(
        encryptor
            .decrypt("tenant-a", "event-log", &envelope)
            .expect("decrypt with AWS KMS client"),
        b"production secret"
    );
    assert_eq!(provider.key_metadata().provider, "aws-kms:us-east-1");
}

#[test]
fn key_rotation_reencrypts_existing_event_records_with_active_key() {
    let path = temp_file("rotation-log");
    let old_key = TenantKey::new(KeyId::new("key-old"), [1; 32]);
    let new_key = TenantKey::new(KeyId::new("key-new"), [2; 32]);
    let mut log =
        EncryptedEventLog::open(&path, KeyRing::new(old_key.clone())).expect("encrypted log opens");
    log.append_record(b"AssertionAdded:secret-before-rotation")
        .expect("append encrypted record");

    log.replace_key_ring(
        KeyRing::new(old_key)
            .with_key(new_key)
            .activate(&KeyId::new("key-new")),
    );
    let report = log
        .rewrite_with_active_key()
        .expect("rewrite with rotated key");

    assert_eq!(report.records_reencrypted, 1);
    assert_eq!(report.from_key_ids, vec![KeyId::new("key-old")]);
    assert_eq!(report.to_key_id, KeyId::new("key-new"));
    assert_eq!(
        log.record_headers()
            .expect("headers")
            .iter()
            .map(|header| header.key_id.clone())
            .collect::<Vec<_>>(),
        vec![KeyId::new("key-new")]
    );
    assert_eq!(
        log.read_all().expect("decrypt rotated log"),
        vec![b"AssertionAdded:secret-before-rotation".to_vec()]
    );

    fs::remove_file(path).expect("cleanup");
}

#[test]
fn encrypted_snapshot_store_round_trips_and_rotates_keys() {
    let path = temp_file("snapshot");
    let old_key = TenantKey::new(KeyId::new("snapshot-old"), [3; 32]);
    let new_key = TenantKey::new(KeyId::new("snapshot-new"), [4; 32]);
    let snapshot = b"snapshot contains source text and graph state";

    EncryptedSnapshotStore::write(
        &path,
        &KeyRing::new(old_key.clone()),
        "snapshot-001",
        snapshot,
    )
    .expect("write encrypted snapshot");

    let raw = fs::read_to_string(&path).expect("read encrypted snapshot");
    assert!(!raw.contains("source text"));

    let restored =
        EncryptedSnapshotStore::read(&path, &KeyRing::new(old_key.clone())).expect("read snapshot");
    assert_eq!(restored.snapshot_name, "snapshot-001");
    assert_eq!(restored.key_id, KeyId::new("snapshot-old"));
    assert_eq!(restored.plaintext, snapshot);

    let rotated_ring = KeyRing::new(old_key)
        .with_key(new_key)
        .activate(&KeyId::new("snapshot-new"));
    let report =
        EncryptedSnapshotStore::rotate(&path, &rotated_ring).expect("rotate encrypted snapshot");
    assert_eq!(report.records_reencrypted, 1);
    assert_eq!(report.to_key_id, KeyId::new("snapshot-new"));

    let rotated = EncryptedSnapshotStore::read(&path, &rotated_ring).expect("read rotated");
    assert_eq!(rotated.key_id, KeyId::new("snapshot-new"));
    assert_eq!(rotated.plaintext, snapshot);

    fs::remove_file(path).expect("cleanup");
}

#[test]
fn encrypted_source_store_round_trips_source_bytes_without_plaintext_leakage() {
    let path = temp_file("source-store");
    let key_ring = key_ring("source-key", [5; 32]);
    let source_id = SourceId::new("source-classified");
    let source_text = b"classified source evidence about Lab B";

    EncryptedSourceStore::put_source(&path, &key_ring, source_id.clone(), source_text)
        .expect("put source");

    let raw = fs::read_to_string(&path).expect("read source store");
    assert!(!raw.contains("classified source evidence"));

    let restored =
        EncryptedSourceStore::get_source(&path, &key_ring, &source_id).expect("get source");
    assert_eq!(restored.source_id, source_id);
    assert_eq!(restored.plaintext, source_text);

    fs::remove_file(path).expect("cleanup");
}

#[test]
fn redaction_aware_query_engine_preserves_ids_but_removes_raw_source_content() {
    let pack = evidence_pack_fixture();
    let policy = ConfidentialQueryPolicy::no_raw_source()
        .redact_source(SourceId::new("source-secret"), "legal hold");
    let engine = RedactionAwareQueryEngine::new(policy);

    let redacted = engine.redact_evidence_pack(&pack);

    assert_eq!(
        redacted.pack.assertions[0].source_ids,
        vec![SourceId::new("source-secret")]
    );
    assert_eq!(
        redacted.pack.sources[0].source_id,
        SourceId::new("source-secret")
    );
    assert_eq!(redacted.pack.sources[0].uri, None);
    assert_eq!(
        redacted.pack.sources[0].snippet,
        "[redacted: raw source disabled; legal hold]"
    );
    assert_eq!(redacted.redactions.len(), 1);
    assert!(redacted.redactions[0].raw_source_removed);
}

#[test]
fn privacy_preserving_analytics_suppresses_small_groups_and_adds_dp_metadata() {
    let observations = vec![
        AnalyticsObservation::new(TenantId::new("tenant-a"), "source_type", "document"),
        AnalyticsObservation::new(TenantId::new("tenant-a"), "source_type", "document"),
        AnalyticsObservation::new(TenantId::new("tenant-a"), "source_type", "document"),
        AnalyticsObservation::new(TenantId::new("tenant-a"), "source_type", "human_report"),
    ];
    let engine = PrivateAnalyticsEngine::new(0.7, 42);

    let report = engine.run(
        PrivacyAnalyticsQuery {
            metric: AnalyticsMetric::CountByLabel {
                label: "source_type".to_owned(),
            },
            min_group_size: 2,
        },
        &observations,
    );

    assert_eq!(report.epsilon, 0.7);
    assert_eq!(report.suppressed_groups, vec!["human_report"]);
    assert_eq!(report.rows.len(), 1);
    assert_eq!(report.rows[0].label, "document");
    assert_eq!(report.rows[0].exact_count, None);
    assert_ne!(report.rows[0].noisy_count, 3.0);
}

fn evidence_pack_fixture() -> EvidencePack {
    let source_id = SourceId::new("source-secret");
    EvidencePack {
        query: "where did Person A work?".to_owned(),
        entities: vec![
            Entity {
                id: EntityId::new("person-a"),
                entity_type: EntityType::Person,
                canonical_name: Some("Person A".to_owned()),
                properties: PropertyMap::default(),
                created_tx: TxTime::new(1),
            },
            Entity {
                id: EntityId::new("lab-b"),
                entity_type: EntityType::Organization,
                canonical_name: Some("Lab B".to_owned()),
                properties: PropertyMap::default(),
                created_tx: TxTime::new(1),
            },
        ],
        assertions: vec![Assertion {
            id: AssertionId::new("assertion-secret"),
            subject: EntityId::new("person-a"),
            predicate: PredicateId::new("worked_at"),
            object: GraphValue::Entity(EntityId::new("lab-b")),
            valid_time: TimeInterval::new(ValidTime::new(2024), None).expect("valid interval"),
            transaction_time: TimeInterval::new(TxTime::new(10), None).expect("tx interval"),
            confidence: Confidence::new(0.91).expect("confidence"),
            source_ids: vec![source_id.clone()],
            context: ContextScope::Named("tenant:tenant-a".to_owned()),
            status: AssertionStatus::Active,
        }],
        sources: vec![SourceExcerpt {
            source_id,
            source_type: SourceType::Document,
            uri: Some("file://secret/source.txt".to_owned()),
            content_hash: ContentHash::new("sha256:secret"),
            snippet: "Person A worked at Lab B inside the classified program.".to_owned(),
            trust_score: Some(0.99),
        }],
        paths: Vec::new(),
        contradictions: Vec::new(),
        generated_at: TxTime::new(50),
    }
}
