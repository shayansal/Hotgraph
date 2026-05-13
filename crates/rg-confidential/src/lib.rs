//! Confidential and privacy-preserving graph mode primitives.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use rg_ai::EvidencePack;
use rg_core::{SourceId, TenantId};

macro_rules! string_newtype {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
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
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

string_newtype!(KeyId);

const EVENT_LOG_HEADER: &str = "RGCONF-EVENTLOG-V1";
const SNAPSHOT_HEADER: &str = "RGCONF-SNAPSHOT-V1";
const SOURCE_STORE_HEADER: &str = "RGCONF-SOURCESTORE-V1";
const AUTH_TAG_BYTES: usize = 16;
const XCHACHA20_NONCE_BYTES: usize = 24;

#[derive(Clone, Eq, PartialEq)]
pub struct TenantKey {
    pub id: KeyId,
    material: [u8; 32],
}

impl TenantKey {
    pub fn new(id: KeyId, material: [u8; 32]) -> Self {
        Self { id, material }
    }
}

impl fmt::Debug for TenantKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TenantKey")
            .field("id", &self.id)
            .field("material", &"[redacted]")
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyRing {
    active_key_id: KeyId,
    keys: BTreeMap<KeyId, TenantKey>,
}

impl KeyRing {
    pub fn new(active_key: TenantKey) -> Self {
        let active_key_id = active_key.id.clone();
        let mut keys = BTreeMap::new();
        keys.insert(active_key.id.clone(), active_key);
        Self {
            active_key_id,
            keys,
        }
    }

    pub fn with_key(mut self, key: TenantKey) -> Self {
        self.keys.insert(key.id.clone(), key);
        self
    }

    pub fn activate(mut self, key_id: &KeyId) -> Self {
        if self.keys.contains_key(key_id) {
            self.active_key_id = key_id.clone();
        }
        self
    }

    pub fn active_key_id(&self) -> &KeyId {
        &self.active_key_id
    }

    fn active_key(&self) -> Result<&TenantKey, ConfidentialError> {
        self.keys
            .get(&self.active_key_id)
            .ok_or_else(|| ConfidentialError::MissingKey(self.active_key_id.clone()))
    }

    fn key(&self, key_id: &KeyId) -> Result<&TenantKey, ConfidentialError> {
        self.keys
            .get(key_id)
            .ok_or_else(|| ConfidentialError::MissingKey(key_id.clone()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfidentialError {
    AuthenticationFailed,
    Codec(String),
    Io(String),
    MissingKey(KeyId),
}

impl fmt::Display for ConfidentialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthenticationFailed => {
                formatter.write_str("encrypted record authentication failed")
            }
            Self::Codec(message) => write!(formatter, "confidential codec error: {message}"),
            Self::Io(message) => write!(formatter, "confidential IO error: {message}"),
            Self::MissingKey(key_id) => write!(formatter, "missing tenant key {key_id}"),
        }
    }
}

impl std::error::Error for ConfidentialError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncryptedDataKey {
    pub key_id: KeyId,
    pub ciphertext: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KmsKeyMetadata {
    pub key_id: KeyId,
    pub provider: String,
}

pub trait KmsProvider: Clone {
    fn create_data_key(
        &self,
        tenant_id: &str,
        purpose: &str,
    ) -> Result<([u8; 32], EncryptedDataKey), ConfidentialError>;

    fn decrypt_data_key(
        &self,
        tenant_id: &str,
        purpose: &str,
        encrypted: &EncryptedDataKey,
    ) -> Result<[u8; 32], ConfidentialError>;

    fn rotate_key(&mut self, new_key_id: impl Into<String>) -> Result<KeyId, ConfidentialError>;
    fn key_metadata(&self) -> KmsKeyMetadata;
    fn health_check(&self) -> Result<(), ConfidentialError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalDevKmsProvider {
    active_key_id: KeyId,
    master_keys: BTreeMap<KeyId, [u8; 32]>,
}

impl LocalDevKmsProvider {
    pub fn new(key_id: impl Into<String>) -> Self {
        let key_id = KeyId::new(key_id);
        let material = derive_local_kms_master_key(&key_id);
        let mut master_keys = BTreeMap::new();
        master_keys.insert(key_id.clone(), material);
        Self {
            active_key_id: key_id,
            master_keys,
        }
    }
}

impl KmsProvider for LocalDevKmsProvider {
    fn create_data_key(
        &self,
        tenant_id: &str,
        purpose: &str,
    ) -> Result<([u8; 32], EncryptedDataKey), ConfidentialError> {
        let master = self
            .master_keys
            .get(&self.active_key_id)
            .ok_or_else(|| ConfidentialError::MissingKey(self.active_key_id.clone()))?;
        let data_key = derive_data_key(master, tenant_id, purpose);
        Ok((
            data_key,
            EncryptedDataKey {
                key_id: self.active_key_id.clone(),
                ciphertext: wrap_data_key(master, tenant_id, purpose, &data_key),
            },
        ))
    }

    fn decrypt_data_key(
        &self,
        tenant_id: &str,
        purpose: &str,
        encrypted: &EncryptedDataKey,
    ) -> Result<[u8; 32], ConfidentialError> {
        let master = self
            .master_keys
            .get(&encrypted.key_id)
            .ok_or_else(|| ConfidentialError::MissingKey(encrypted.key_id.clone()))?;
        unwrap_data_key(master, tenant_id, purpose, &encrypted.ciphertext)
    }

    fn rotate_key(&mut self, new_key_id: impl Into<String>) -> Result<KeyId, ConfidentialError> {
        let key_id = KeyId::new(new_key_id);
        self.master_keys
            .entry(key_id.clone())
            .or_insert_with(|| derive_local_kms_master_key(&key_id));
        self.active_key_id = key_id.clone();
        Ok(key_id)
    }

    fn key_metadata(&self) -> KmsKeyMetadata {
        KmsKeyMetadata {
            key_id: self.active_key_id.clone(),
            provider: "local-dev".to_owned(),
        }
    }

    fn health_check(&self) -> Result<(), ConfidentialError> {
        if self.master_keys.contains_key(&self.active_key_id) {
            Ok(())
        } else {
            Err(ConfidentialError::MissingKey(self.active_key_id.clone()))
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg(feature = "aws-kms")]
pub struct AwsGeneratedDataKey {
    pub plaintext: [u8; 32],
    pub ciphertext_blob: Vec<u8>,
}

#[cfg(feature = "aws-kms")]
pub trait AwsKmsClient: Clone {
    fn generate_data_key(
        &self,
        key_id: &str,
        tenant_id: &str,
        purpose: &str,
    ) -> Result<AwsGeneratedDataKey, ConfidentialError>;

    fn decrypt_data_key(
        &self,
        key_id: &str,
        tenant_id: &str,
        purpose: &str,
        encrypted: &EncryptedDataKey,
    ) -> Result<[u8; 32], ConfidentialError>;

    fn describe_key(&self, key_id: &str) -> Result<(), ConfidentialError>;
}

#[derive(Clone, Debug)]
#[cfg(feature = "aws-kms")]
pub struct AwsSdkKmsClient {
    client: aws_sdk_kms::Client,
}

#[cfg(feature = "aws-kms")]
impl AwsSdkKmsClient {
    pub fn new(client: aws_sdk_kms::Client) -> Self {
        Self { client }
    }

    pub fn from_env(region: impl Into<Option<String>>) -> Result<Self, ConfidentialError> {
        let region = region.into();
        let config = block_on_aws(async move {
            let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest());
            if let Some(region) = region {
                loader = loader.region(aws_config::Region::new(region));
            }
            loader.load().await
        });
        Ok(Self::new(aws_sdk_kms::Client::new(&config)))
    }
}

#[cfg(feature = "aws-kms")]
impl AwsKmsClient for AwsSdkKmsClient {
    fn generate_data_key(
        &self,
        key_id: &str,
        tenant_id: &str,
        purpose: &str,
    ) -> Result<AwsGeneratedDataKey, ConfidentialError> {
        let response = block_on_aws(
            self.client
                .generate_data_key()
                .key_id(key_id)
                .key_spec(aws_sdk_kms::types::DataKeySpec::Aes256)
                .encryption_context("tenant_id", tenant_id)
                .encryption_context("purpose", purpose)
                .send(),
        )
        .map_err(|error| {
            ConfidentialError::Codec(format!("aws kms generate_data_key failed: {error}"))
        })?;
        let plaintext = response
            .plaintext()
            .ok_or_else(|| {
                ConfidentialError::Codec("aws kms data key response omitted plaintext".to_owned())
            })?
            .as_ref();
        let ciphertext_blob = response
            .ciphertext_blob()
            .ok_or_else(|| {
                ConfidentialError::Codec("aws kms data key response omitted ciphertext".to_owned())
            })?
            .as_ref()
            .to_vec();
        Ok(AwsGeneratedDataKey {
            plaintext: data_key_from_slice(plaintext)?,
            ciphertext_blob,
        })
    }

    fn decrypt_data_key(
        &self,
        key_id: &str,
        tenant_id: &str,
        purpose: &str,
        encrypted: &EncryptedDataKey,
    ) -> Result<[u8; 32], ConfidentialError> {
        let response = block_on_aws(
            self.client
                .decrypt()
                .key_id(key_id)
                .ciphertext_blob(aws_sdk_kms::primitives::Blob::new(
                    encrypted.ciphertext.clone(),
                ))
                .encryption_context("tenant_id", tenant_id)
                .encryption_context("purpose", purpose)
                .send(),
        )
        .map_err(|error| ConfidentialError::Codec(format!("aws kms decrypt failed: {error}")))?;
        let plaintext = response
            .plaintext()
            .ok_or_else(|| {
                ConfidentialError::Codec("aws kms decrypt response omitted plaintext".to_owned())
            })?
            .as_ref();
        data_key_from_slice(plaintext)
    }

    fn describe_key(&self, key_id: &str) -> Result<(), ConfidentialError> {
        block_on_aws(self.client.describe_key().key_id(key_id).send())
            .map(|_| ())
            .map_err(|error| {
                ConfidentialError::Codec(format!("aws kms describe_key failed: {error}"))
            })
    }
}

#[derive(Clone, Debug)]
#[cfg(feature = "aws-kms")]
pub struct AwsKmsProvider<C: AwsKmsClient = AwsSdkKmsClient> {
    key_id: KeyId,
    region: String,
    client: C,
}

#[cfg(feature = "aws-kms")]
impl AwsKmsProvider<AwsSdkKmsClient> {
    pub fn new(
        key_id: impl Into<String>,
        region: impl Into<String>,
    ) -> Result<Self, ConfidentialError> {
        let region = region.into();
        let client = AwsSdkKmsClient::from_env(Some(region.clone()))?;
        Ok(Self {
            key_id: KeyId::new(key_id),
            region,
            client,
        })
    }
}

#[cfg(feature = "aws-kms")]
impl<C: AwsKmsClient> AwsKmsProvider<C> {
    pub fn with_client(key_id: impl Into<String>, region: impl Into<String>, client: C) -> Self {
        Self {
            key_id: KeyId::new(key_id),
            region: region.into(),
            client,
        }
    }

    pub fn region(&self) -> &str {
        &self.region
    }
}

#[cfg(feature = "aws-kms")]
impl<C: AwsKmsClient> KmsProvider for AwsKmsProvider<C> {
    fn create_data_key(
        &self,
        tenant_id: &str,
        purpose: &str,
    ) -> Result<([u8; 32], EncryptedDataKey), ConfidentialError> {
        let generated = self
            .client
            .generate_data_key(self.key_id.as_str(), tenant_id, purpose)?;
        Ok((
            generated.plaintext,
            EncryptedDataKey {
                key_id: self.key_id.clone(),
                ciphertext: generated.ciphertext_blob,
            },
        ))
    }

    fn decrypt_data_key(
        &self,
        tenant_id: &str,
        purpose: &str,
        encrypted: &EncryptedDataKey,
    ) -> Result<[u8; 32], ConfidentialError> {
        if encrypted.key_id != self.key_id {
            return Err(ConfidentialError::MissingKey(encrypted.key_id.clone()));
        }
        self.client
            .decrypt_data_key(self.key_id.as_str(), tenant_id, purpose, encrypted)
    }

    fn rotate_key(&mut self, new_key_id: impl Into<String>) -> Result<KeyId, ConfidentialError> {
        self.key_id = KeyId::new(new_key_id);
        Ok(self.key_id.clone())
    }

    fn key_metadata(&self) -> KmsKeyMetadata {
        KmsKeyMetadata {
            key_id: self.key_id.clone(),
            provider: format!("aws-kms:{}", self.region),
        }
    }

    fn health_check(&self) -> Result<(), ConfidentialError> {
        self.client.describe_key(self.key_id.as_str())
    }
}

#[cfg(feature = "aws-kms")]
fn data_key_from_slice(bytes: &[u8]) -> Result<[u8; 32], ConfidentialError> {
    bytes.try_into().map_err(|_| {
        ConfidentialError::Codec(format!(
            "AWS KMS data key must be 32 bytes, got {} bytes",
            bytes.len()
        ))
    })
}

#[cfg(feature = "aws-kms")]
fn block_on_aws<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build AWS KMS runtime")
        .block_on(future)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncryptedEnvelope {
    pub algorithm: String,
    pub encrypted_data_key: EncryptedDataKey,
    pub nonce_hex: String,
    pub associated_data: String,
    pub ciphertext: Vec<u8>,
    pub tag: [u8; AUTH_TAG_BYTES],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvelopeEncryptor<P: KmsProvider> {
    kms: P,
}

impl<P: KmsProvider> EnvelopeEncryptor<P> {
    pub fn new(kms: P) -> Self {
        Self { kms }
    }

    pub fn new_production(kms: P) -> Result<Self, ConfidentialError> {
        let metadata = kms.key_metadata();
        if metadata.provider == "local-dev" {
            return Err(ConfidentialError::Codec(
                "LocalDevKmsProvider is dev/test-only and cannot protect production data"
                    .to_owned(),
            ));
        }
        kms.health_check()?;
        Ok(Self { kms })
    }

    pub fn encrypt(
        &self,
        tenant_id: &str,
        purpose: &str,
        plaintext: &[u8],
    ) -> Result<EncryptedEnvelope, ConfidentialError> {
        self.kms.health_check()?;
        let (data_key, encrypted_data_key) = self.kms.create_data_key(tenant_id, purpose)?;
        let nonce = derive_envelope_nonce(&data_key, tenant_id, purpose);
        let associated_data = envelope_associated_data(tenant_id, purpose);
        let (ciphertext, tag) =
            aead_encrypt(&data_key, &nonce, associated_data.as_bytes(), plaintext)?;
        Ok(EncryptedEnvelope {
            algorithm: "XChaCha20Poly1305".to_owned(),
            encrypted_data_key,
            nonce_hex: hex_encode(&nonce),
            associated_data,
            ciphertext,
            tag,
        })
    }

    pub fn decrypt(
        &self,
        tenant_id: &str,
        purpose: &str,
        envelope: &EncryptedEnvelope,
    ) -> Result<Vec<u8>, ConfidentialError> {
        if envelope.algorithm != "XChaCha20Poly1305" {
            return Err(ConfidentialError::Codec(format!(
                "unsupported envelope algorithm {}",
                envelope.algorithm
            )));
        }
        let expected_associated_data = envelope_associated_data(tenant_id, purpose);
        if envelope.associated_data != expected_associated_data {
            return Err(ConfidentialError::AuthenticationFailed);
        }
        let data_key =
            self.kms
                .decrypt_data_key(tenant_id, purpose, &envelope.encrypted_data_key)?;
        let nonce = decode_xchacha_nonce(&envelope.nonce_hex)?;
        aead_decrypt(
            &data_key,
            &nonce,
            expected_associated_data.as_bytes(),
            &envelope.ciphertext,
            &envelope.tag,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncryptedRecordHeader {
    pub sequence: u64,
    pub key_id: KeyId,
    pub nonce_hex: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EncryptedRecord {
    header: EncryptedRecordHeader,
    ciphertext: Vec<u8>,
    tag: [u8; AUTH_TAG_BYTES],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyRotationReport {
    pub records_reencrypted: usize,
    pub from_key_ids: Vec<KeyId>,
    pub to_key_id: KeyId,
}

#[derive(Clone, Debug)]
pub struct EncryptedEventLog {
    path: PathBuf,
    key_ring: KeyRing,
}

impl EncryptedEventLog {
    pub fn open(path: impl AsRef<Path>, key_ring: KeyRing) -> Result<Self, ConfidentialError> {
        let path = path.as_ref().to_path_buf();
        initialize_file(&path, EVENT_LOG_HEADER)?;
        Ok(Self { path, key_ring })
    }

    pub fn replace_key_ring(&mut self, key_ring: KeyRing) {
        self.key_ring = key_ring;
    }

    pub fn append_record(
        &mut self,
        plaintext: &[u8],
    ) -> Result<EncryptedRecordHeader, ConfidentialError> {
        let sequence = self.next_sequence()?;
        let record = encrypt_record(
            self.key_ring.active_key()?,
            sequence,
            "event-log",
            plaintext,
        )?;
        let mut file = OpenOptions::new()
            .append(true)
            .open(&self.path)
            .map_err(io_error)?;
        file.write_all(encode_event_record(&record).as_bytes())
            .map_err(io_error)?;
        file.write_all(b"\n").map_err(io_error)?;
        file.sync_data().map_err(io_error)?;
        Ok(record.header)
    }

    pub fn read_all(&self) -> Result<Vec<Vec<u8>>, ConfidentialError> {
        self.read_records()?
            .iter()
            .map(|record| decrypt_record(&self.key_ring, record, "event-log"))
            .collect()
    }

    pub fn record_headers(&self) -> Result<Vec<EncryptedRecordHeader>, ConfidentialError> {
        Ok(self
            .read_records()?
            .into_iter()
            .map(|record| record.header)
            .collect())
    }

    pub fn rewrite_with_active_key(&mut self) -> Result<KeyRotationReport, ConfidentialError> {
        let records = self.read_records()?;
        let mut from_key_ids = BTreeSet::new();
        let plaintext_records = records
            .iter()
            .map(|record| {
                from_key_ids.insert(record.header.key_id.clone());
                decrypt_record(&self.key_ring, record, "event-log")
            })
            .collect::<Result<Vec<_>, _>>()?;
        let rewritten = plaintext_records
            .iter()
            .enumerate()
            .map(|(index, plaintext)| {
                encrypt_record(
                    self.key_ring.active_key()?,
                    (index + 1) as u64,
                    "event-log",
                    plaintext,
                )
            })
            .collect::<Result<Vec<_>, ConfidentialError>>()?;
        write_event_records(&self.path, &rewritten)?;
        Ok(KeyRotationReport {
            records_reencrypted: rewritten.len(),
            from_key_ids: from_key_ids.into_iter().collect(),
            to_key_id: self.key_ring.active_key_id().clone(),
        })
    }

    fn next_sequence(&self) -> Result<u64, ConfidentialError> {
        Ok(self
            .read_records()?
            .iter()
            .map(|record| record.header.sequence)
            .max()
            .unwrap_or(0)
            + 1)
    }

    fn read_records(&self) -> Result<Vec<EncryptedRecord>, ConfidentialError> {
        read_records(&self.path, EVENT_LOG_HEADER, decode_event_record)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncryptedSnapshot {
    pub snapshot_name: String,
    pub key_id: KeyId,
    pub plaintext: Vec<u8>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EncryptedSnapshotStore;

impl EncryptedSnapshotStore {
    pub fn write(
        path: impl AsRef<Path>,
        key_ring: &KeyRing,
        snapshot_name: impl Into<String>,
        plaintext: &[u8],
    ) -> Result<(), ConfidentialError> {
        let record = encrypt_record(key_ring.active_key()?, 1, "snapshot", plaintext)?;
        let line = encode_snapshot_record(&snapshot_name.into(), &record);
        write_records(path.as_ref(), SNAPSHOT_HEADER, &[line])
    }

    pub fn read(
        path: impl AsRef<Path>,
        key_ring: &KeyRing,
    ) -> Result<EncryptedSnapshot, ConfidentialError> {
        let records = read_records(path.as_ref(), SNAPSHOT_HEADER, decode_snapshot_record)?;
        let Some((snapshot_name, record)) = records.into_iter().next() else {
            return Err(ConfidentialError::Codec(
                "snapshot file is empty".to_owned(),
            ));
        };
        let plaintext = decrypt_record(key_ring, &record, "snapshot")?;
        Ok(EncryptedSnapshot {
            snapshot_name,
            key_id: record.header.key_id,
            plaintext,
        })
    }

    pub fn rotate(
        path: impl AsRef<Path>,
        key_ring: &KeyRing,
    ) -> Result<KeyRotationReport, ConfidentialError> {
        let snapshot = Self::read(path.as_ref(), key_ring)?;
        let from_key_id = snapshot.key_id;
        Self::write(
            path.as_ref(),
            key_ring,
            snapshot.snapshot_name,
            &snapshot.plaintext,
        )?;
        Ok(KeyRotationReport {
            records_reencrypted: 1,
            from_key_ids: vec![from_key_id],
            to_key_id: key_ring.active_key_id().clone(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncryptedSourceDocument {
    pub source_id: SourceId,
    pub key_id: KeyId,
    pub plaintext: Vec<u8>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EncryptedSourceStore;

impl EncryptedSourceStore {
    pub fn put_source(
        path: impl AsRef<Path>,
        key_ring: &KeyRing,
        source_id: SourceId,
        plaintext: &[u8],
    ) -> Result<(), ConfidentialError> {
        initialize_file(path.as_ref(), SOURCE_STORE_HEADER)?;
        let mut entries = read_records(path.as_ref(), SOURCE_STORE_HEADER, decode_source_record)?;
        entries.retain(|(stored_source_id, _)| stored_source_id != &source_id);
        let sequence = entries
            .iter()
            .map(|(_, record)| record.header.sequence)
            .max()
            .unwrap_or(0)
            + 1;
        entries.push((
            source_id,
            encrypt_record(key_ring.active_key()?, sequence, "source-store", plaintext)?,
        ));
        let lines = entries
            .iter()
            .map(|(source_id, record)| encode_source_record(source_id, record))
            .collect::<Vec<_>>();
        write_records(path.as_ref(), SOURCE_STORE_HEADER, &lines)
    }

    pub fn get_source(
        path: impl AsRef<Path>,
        key_ring: &KeyRing,
        source_id: &SourceId,
    ) -> Result<EncryptedSourceDocument, ConfidentialError> {
        let entries = read_records(path.as_ref(), SOURCE_STORE_HEADER, decode_source_record)?;
        let Some((_, record)) = entries
            .into_iter()
            .find(|(stored_source_id, _)| stored_source_id == source_id)
        else {
            return Err(ConfidentialError::Codec(format!(
                "missing encrypted source {source_id}"
            )));
        };
        let plaintext = decrypt_record(key_ring, &record, "source-store")?;
        Ok(EncryptedSourceDocument {
            source_id: source_id.clone(),
            key_id: record.header.key_id,
            plaintext,
        })
    }

    pub fn rotate(
        path: impl AsRef<Path>,
        key_ring: &KeyRing,
    ) -> Result<KeyRotationReport, ConfidentialError> {
        let entries = read_records(path.as_ref(), SOURCE_STORE_HEADER, decode_source_record)?;
        let mut from_key_ids = BTreeSet::new();
        let rewritten = entries
            .iter()
            .enumerate()
            .map(|(index, (source_id, record))| {
                from_key_ids.insert(record.header.key_id.clone());
                let plaintext = decrypt_record(key_ring, record, "source-store")?;
                let encrypted = encrypt_record(
                    key_ring.active_key()?,
                    (index + 1) as u64,
                    "source-store",
                    &plaintext,
                )?;
                Ok((source_id.clone(), encrypted))
            })
            .collect::<Result<Vec<_>, ConfidentialError>>()?;
        let lines = rewritten
            .iter()
            .map(|(source_id, record)| encode_source_record(source_id, record))
            .collect::<Vec<_>>();
        write_records(path.as_ref(), SOURCE_STORE_HEADER, &lines)?;
        Ok(KeyRotationReport {
            records_reencrypted: rewritten.len(),
            from_key_ids: from_key_ids.into_iter().collect(),
            to_key_id: key_ring.active_key_id().clone(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfidentialQueryPolicy {
    pub no_raw_source: bool,
    source_redactions: BTreeMap<SourceId, String>,
}

impl ConfidentialQueryPolicy {
    pub fn allow_raw_source() -> Self {
        Self {
            no_raw_source: false,
            source_redactions: BTreeMap::new(),
        }
    }

    pub fn no_raw_source() -> Self {
        Self {
            no_raw_source: true,
            source_redactions: BTreeMap::new(),
        }
    }

    pub fn redact_source(mut self, source_id: SourceId, reason: impl Into<String>) -> Self {
        self.source_redactions.insert(source_id, reason.into());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedactionDecision {
    pub source_id: SourceId,
    pub reason: String,
    pub raw_source_removed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RedactedEvidencePack {
    pub pack: EvidencePack,
    pub redactions: Vec<RedactionDecision>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedactionAwareQueryEngine {
    policy: ConfidentialQueryPolicy,
}

impl RedactionAwareQueryEngine {
    pub fn new(policy: ConfidentialQueryPolicy) -> Self {
        Self { policy }
    }

    pub fn redact_evidence_pack(&self, pack: &EvidencePack) -> RedactedEvidencePack {
        let mut redactions = Vec::new();
        let mut pack = pack.clone();
        for source in &mut pack.sources {
            let mut reasons = Vec::new();
            if self.policy.no_raw_source {
                reasons.push("raw source disabled".to_owned());
            }
            if let Some(reason) = self.policy.source_redactions.get(&source.source_id) {
                reasons.push(reason.clone());
            }
            if reasons.is_empty() {
                continue;
            }
            let reason = reasons.join("; ");
            source.snippet = format!("[redacted: {reason}]");
            source.uri = None;
            redactions.push(RedactionDecision {
                source_id: source.source_id.clone(),
                reason,
                raw_source_removed: true,
            });
        }
        RedactedEvidencePack { pack, redactions }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AnalyticsMetric {
    CountByLabel { label: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrivacyAnalyticsQuery {
    pub metric: AnalyticsMetric,
    pub min_group_size: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnalyticsObservation {
    pub tenant_id: TenantId,
    pub label: String,
    pub value: String,
}

impl AnalyticsObservation {
    pub fn new(tenant_id: TenantId, label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            tenant_id,
            label: label.into(),
            value: value.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrivacyAnalyticsRow {
    pub label: String,
    pub exact_count: Option<usize>,
    pub noisy_count: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrivacyAnalyticsReport {
    pub epsilon: f64,
    pub min_group_size: usize,
    pub rows: Vec<PrivacyAnalyticsRow>,
    pub suppressed_groups: Vec<String>,
    pub privacy_notes: Vec<String>,
}

pub trait PrivacyPreservingAnalytics {
    fn run(
        &self,
        query: PrivacyAnalyticsQuery,
        observations: &[AnalyticsObservation],
    ) -> PrivacyAnalyticsReport;
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrivateAnalyticsEngine {
    epsilon: f64,
    noise_seed: u64,
}

impl PrivateAnalyticsEngine {
    pub fn new(epsilon: f64, noise_seed: u64) -> Self {
        Self {
            epsilon: if epsilon.is_sign_positive() && epsilon.is_finite() {
                epsilon
            } else {
                1.0
            },
            noise_seed,
        }
    }

    pub fn run(
        &self,
        query: PrivacyAnalyticsQuery,
        observations: &[AnalyticsObservation],
    ) -> PrivacyAnalyticsReport {
        <Self as PrivacyPreservingAnalytics>::run(self, query, observations)
    }
}

impl PrivacyPreservingAnalytics for PrivateAnalyticsEngine {
    fn run(
        &self,
        query: PrivacyAnalyticsQuery,
        observations: &[AnalyticsObservation],
    ) -> PrivacyAnalyticsReport {
        let AnalyticsMetric::CountByLabel { label } = &query.metric;
        let mut counts = BTreeMap::<String, usize>::new();
        for observation in observations
            .iter()
            .filter(|observation| &observation.label == label)
        {
            *counts.entry(observation.value.clone()).or_default() += 1;
        }

        let mut suppressed_groups = Vec::new();
        let mut rows = Vec::new();
        for (group, count) in counts {
            if count < query.min_group_size {
                suppressed_groups.push(group);
                continue;
            }
            rows.push(PrivacyAnalyticsRow {
                noisy_count: (count as f64
                    + deterministic_noise(self.noise_seed, &group, self.epsilon))
                .max(0.0),
                exact_count: None,
                label: group,
            });
        }
        PrivacyAnalyticsReport {
            epsilon: self.epsilon,
            min_group_size: query.min_group_size,
            rows,
            suppressed_groups,
            privacy_notes: vec![
                "exact counts are withheld".to_owned(),
                "groups below the minimum size are suppressed".to_owned(),
                "deterministic noise is used for reproducible tests and evals".to_owned(),
            ],
        }
    }
}

fn initialize_file(path: &Path, header: &str) -> Result<(), ConfidentialError> {
    let exists = path.exists();
    if exists && path.metadata().map_err(io_error)?.len() > 0 {
        let file = File::open(path).map_err(io_error)?;
        let mut lines = BufReader::new(file).lines();
        let first = lines
            .next()
            .ok_or_else(|| ConfidentialError::Codec("empty confidential file".to_owned()))?
            .map_err(io_error)?;
        if first != header {
            return Err(ConfidentialError::Codec(format!(
                "invalid confidential file header {first}"
            )));
        }
        return Ok(());
    }
    let mut file = File::create(path).map_err(io_error)?;
    file.write_all(header.as_bytes()).map_err(io_error)?;
    file.write_all(b"\n").map_err(io_error)?;
    file.sync_data().map_err(io_error)
}

fn read_records<T>(
    path: &Path,
    expected_header: &str,
    decode: fn(&str) -> Result<T, ConfidentialError>,
) -> Result<Vec<T>, ConfidentialError> {
    let file = File::open(path).map_err(io_error)?;
    let mut lines = BufReader::new(file).lines();
    let header = lines
        .next()
        .ok_or_else(|| ConfidentialError::Codec("confidential file is empty".to_owned()))?
        .map_err(io_error)?;
    if header != expected_header {
        return Err(ConfidentialError::Codec(format!(
            "invalid confidential file header {header}"
        )));
    }
    let mut records = Vec::new();
    for line in lines {
        let line = line.map_err(io_error)?;
        if line.trim().is_empty() {
            continue;
        }
        records.push(decode(&line)?);
    }
    Ok(records)
}

fn write_event_records(path: &Path, records: &[EncryptedRecord]) -> Result<(), ConfidentialError> {
    let lines = records.iter().map(encode_event_record).collect::<Vec<_>>();
    write_records(path, EVENT_LOG_HEADER, &lines)
}

fn write_records(path: &Path, header: &str, lines: &[String]) -> Result<(), ConfidentialError> {
    let tmp_path = path.with_extension("tmp");
    let mut file = File::create(&tmp_path).map_err(io_error)?;
    file.write_all(header.as_bytes()).map_err(io_error)?;
    file.write_all(b"\n").map_err(io_error)?;
    for line in lines {
        file.write_all(line.as_bytes()).map_err(io_error)?;
        file.write_all(b"\n").map_err(io_error)?;
    }
    file.sync_data().map_err(io_error)?;
    fs::rename(&tmp_path, path).map_err(io_error)
}

fn encrypt_record(
    key: &TenantKey,
    sequence: u64,
    purpose: &str,
    plaintext: &[u8],
) -> Result<EncryptedRecord, ConfidentialError> {
    let nonce = derive_record_nonce(sequence, &key.id, purpose);
    let associated_data = record_associated_data(sequence, &key.id, purpose);
    let (ciphertext, tag) =
        aead_encrypt(&key.material, &nonce, associated_data.as_bytes(), plaintext)?;
    Ok(EncryptedRecord {
        header: EncryptedRecordHeader {
            sequence,
            key_id: key.id.clone(),
            nonce_hex: hex_encode(&nonce),
        },
        ciphertext,
        tag,
    })
}

fn decrypt_record(
    key_ring: &KeyRing,
    record: &EncryptedRecord,
    purpose: &str,
) -> Result<Vec<u8>, ConfidentialError> {
    let key = key_ring.key(&record.header.key_id)?;
    let nonce = decode_xchacha_nonce(&record.header.nonce_hex)?;
    let associated_data = record_associated_data(record.header.sequence, &key.id, purpose);
    aead_decrypt(
        &key.material,
        &nonce,
        associated_data.as_bytes(),
        &record.ciphertext,
        &record.tag,
    )
}

fn derive_record_nonce(
    sequence: u64,
    key_id: &KeyId,
    purpose: &str,
) -> [u8; XCHACHA20_NONCE_BYTES] {
    let mut seed = stable_hash(0x8c7d_f2a9_d11a_7701, &sequence.to_le_bytes());
    seed = stable_hash(seed, key_id.as_str().as_bytes());
    seed = stable_hash(seed, purpose.as_bytes());
    let mut nonce = [0_u8; XCHACHA20_NONCE_BYTES];
    for (index, byte) in nonce.iter_mut().enumerate() {
        seed = splitmix64(seed ^ index as u64);
        *byte = (seed >> ((index % 8) * 8)) as u8;
    }
    nonce
}

fn derive_local_kms_master_key(key_id: &KeyId) -> [u8; 32] {
    let mut seed = stable_hash(0x6a09_e667_f3bc_c909, key_id.as_str().as_bytes());
    let mut material = [0_u8; 32];
    for (index, byte) in material.iter_mut().enumerate() {
        seed = splitmix64(seed ^ ((index as u64) << 32));
        *byte = (seed >> ((index % 8) * 8)) as u8;
    }
    material
}

fn derive_data_key(master: &[u8; 32], tenant_id: &str, purpose: &str) -> [u8; 32] {
    let mut seed = stable_hash(0xbb67_ae85_84ca_a73b, master);
    seed = stable_hash(seed, tenant_id.as_bytes());
    seed = stable_hash(seed, purpose.as_bytes());
    let mut material = [0_u8; 32];
    for (index, byte) in material.iter_mut().enumerate() {
        seed = splitmix64(seed ^ index as u64);
        *byte = (seed >> ((index % 8) * 8)) as u8;
    }
    material
}

fn wrap_data_key(
    master: &[u8; 32],
    tenant_id: &str,
    purpose: &str,
    data_key: &[u8; 32],
) -> Vec<u8> {
    let nonce = derive_kms_nonce(master, tenant_id, purpose);
    let associated_data = local_kms_associated_data(tenant_id, purpose);
    let (mut ciphertext, tag) = aead_encrypt(master, &nonce, associated_data.as_bytes(), data_key)
        .expect("local development KMS wrapping uses valid fixed-size AEAD inputs");
    ciphertext.extend_from_slice(&tag);
    ciphertext
}

fn unwrap_data_key(
    master: &[u8; 32],
    tenant_id: &str,
    purpose: &str,
    encrypted: &[u8],
) -> Result<[u8; 32], ConfidentialError> {
    if encrypted.len() < AUTH_TAG_BYTES {
        return Err(ConfidentialError::AuthenticationFailed);
    }
    let tag_offset = encrypted.len() - AUTH_TAG_BYTES;
    let mut tag = [0_u8; AUTH_TAG_BYTES];
    tag.copy_from_slice(&encrypted[tag_offset..]);
    let nonce = derive_kms_nonce(master, tenant_id, purpose);
    let associated_data = local_kms_associated_data(tenant_id, purpose);
    let plaintext = aead_decrypt(
        master,
        &nonce,
        associated_data.as_bytes(),
        &encrypted[..tag_offset],
        &tag,
    )?;
    if plaintext.len() != 32 {
        return Err(ConfidentialError::Codec(
            "decrypted data key must be 32 bytes".to_owned(),
        ));
    }
    let mut data_key = [0_u8; 32];
    data_key.copy_from_slice(&plaintext);
    Ok(data_key)
}

fn derive_kms_nonce(
    master: &[u8; 32],
    tenant_id: &str,
    purpose: &str,
) -> [u8; XCHACHA20_NONCE_BYTES] {
    let mut seed = stable_hash(0x3c6e_f372_fe94_f82b, master);
    seed = stable_hash(seed, tenant_id.as_bytes());
    seed = stable_hash(seed, purpose.as_bytes());
    let mut nonce = [0_u8; XCHACHA20_NONCE_BYTES];
    for (index, byte) in nonce.iter_mut().enumerate() {
        seed = splitmix64(seed ^ (index as u64).rotate_left(11));
        *byte = (seed >> ((index % 8) * 8)) as u8;
    }
    nonce
}

fn derive_envelope_nonce(
    data_key: &[u8; 32],
    tenant_id: &str,
    purpose: &str,
) -> [u8; XCHACHA20_NONCE_BYTES] {
    let mut seed = stable_hash(0x510e_527f_ade6_82d1, data_key);
    seed = stable_hash(seed, tenant_id.as_bytes());
    seed = stable_hash(seed, purpose.as_bytes());
    let mut nonce = [0_u8; XCHACHA20_NONCE_BYTES];
    for (index, byte) in nonce.iter_mut().enumerate() {
        seed = splitmix64(seed ^ (index as u64).rotate_left(7));
        *byte = (seed >> ((index % 8) * 8)) as u8;
    }
    nonce
}

fn record_associated_data(sequence: u64, key_id: &KeyId, purpose: &str) -> String {
    format!(
        "record:v1;purpose={purpose};sequence={sequence};key_id={}",
        key_id.as_str()
    )
}

fn envelope_associated_data(tenant_id: &str, purpose: &str) -> String {
    format!("envelope:v1;tenant={tenant_id};purpose={purpose}")
}

fn local_kms_associated_data(tenant_id: &str, purpose: &str) -> String {
    format!("local-kms:v1;tenant={tenant_id};purpose={purpose}")
}

fn decode_xchacha_nonce(nonce_hex: &str) -> Result<[u8; XCHACHA20_NONCE_BYTES], ConfidentialError> {
    let nonce_bytes = hex_decode(nonce_hex)?;
    if nonce_bytes.len() != XCHACHA20_NONCE_BYTES {
        return Err(ConfidentialError::Codec(format!(
            "nonce must be {XCHACHA20_NONCE_BYTES} bytes"
        )));
    }
    let mut nonce = [0_u8; XCHACHA20_NONCE_BYTES];
    nonce.copy_from_slice(&nonce_bytes);
    Ok(nonce)
}

fn aead_encrypt(
    key_material: &[u8; 32],
    nonce: &[u8; XCHACHA20_NONCE_BYTES],
    associated_data: &[u8],
    plaintext: &[u8],
) -> Result<(Vec<u8>, [u8; AUTH_TAG_BYTES]), ConfidentialError> {
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key_material));
    let mut sealed = cipher
        .encrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: plaintext,
                aad: associated_data,
            },
        )
        .map_err(|_| ConfidentialError::AuthenticationFailed)?;
    if sealed.len() < AUTH_TAG_BYTES {
        return Err(ConfidentialError::Codec(
            "AEAD ciphertext shorter than auth tag".to_owned(),
        ));
    }
    let tag_offset = sealed.len() - AUTH_TAG_BYTES;
    let tag_bytes = sealed.split_off(tag_offset);
    let mut tag = [0_u8; AUTH_TAG_BYTES];
    tag.copy_from_slice(&tag_bytes);
    Ok((sealed, tag))
}

fn aead_decrypt(
    key_material: &[u8; 32],
    nonce: &[u8; XCHACHA20_NONCE_BYTES],
    associated_data: &[u8],
    ciphertext: &[u8],
    tag: &[u8; AUTH_TAG_BYTES],
) -> Result<Vec<u8>, ConfidentialError> {
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key_material));
    let mut sealed = Vec::with_capacity(ciphertext.len() + tag.len());
    sealed.extend_from_slice(ciphertext);
    sealed.extend_from_slice(tag);
    cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: &sealed,
                aad: associated_data,
            },
        )
        .map_err(|_| ConfidentialError::AuthenticationFailed)
}

fn encode_event_record(record: &EncryptedRecord) -> String {
    encode_parts(&[
        record.header.sequence.to_string(),
        record.header.key_id.as_str().to_owned(),
        record.header.nonce_hex.clone(),
        hex_encode(&record.ciphertext),
        hex_encode(&record.tag),
    ])
}

fn decode_event_record(record: &str) -> Result<EncryptedRecord, ConfidentialError> {
    let parts = decode_parts(record)?;
    let sequence = required(&parts, 0, "sequence")?
        .parse::<u64>()
        .map_err(|error| ConfidentialError::Codec(error.to_string()))?;
    let key_id = KeyId::new(required(&parts, 1, "key id")?);
    let nonce_hex = required(&parts, 2, "nonce")?.to_owned();
    let ciphertext = hex_decode(required(&parts, 3, "ciphertext")?)?;
    let tag_bytes = hex_decode(required(&parts, 4, "tag")?)?;
    if tag_bytes.len() != AUTH_TAG_BYTES {
        return Err(ConfidentialError::Codec(
            "invalid auth tag length".to_owned(),
        ));
    }
    let mut tag = [0_u8; AUTH_TAG_BYTES];
    tag.copy_from_slice(&tag_bytes);
    Ok(EncryptedRecord {
        header: EncryptedRecordHeader {
            sequence,
            key_id,
            nonce_hex,
        },
        ciphertext,
        tag,
    })
}

fn encode_snapshot_record(snapshot_name: &str, record: &EncryptedRecord) -> String {
    encode_parts(&[
        snapshot_name.to_owned(),
        record.header.sequence.to_string(),
        record.header.key_id.as_str().to_owned(),
        record.header.nonce_hex.clone(),
        hex_encode(&record.ciphertext),
        hex_encode(&record.tag),
    ])
}

fn decode_snapshot_record(record: &str) -> Result<(String, EncryptedRecord), ConfidentialError> {
    let parts = decode_parts(record)?;
    let snapshot_name = required(&parts, 0, "snapshot name")?.to_owned();
    let event_record = encode_parts(&[
        required(&parts, 1, "sequence")?.to_owned(),
        required(&parts, 2, "key id")?.to_owned(),
        required(&parts, 3, "nonce")?.to_owned(),
        required(&parts, 4, "ciphertext")?.to_owned(),
        required(&parts, 5, "tag")?.to_owned(),
    ]);
    Ok((snapshot_name, decode_event_record(&event_record)?))
}

fn encode_source_record(source_id: &SourceId, record: &EncryptedRecord) -> String {
    encode_parts(&[
        source_id.as_str().to_owned(),
        record.header.sequence.to_string(),
        record.header.key_id.as_str().to_owned(),
        record.header.nonce_hex.clone(),
        hex_encode(&record.ciphertext),
        hex_encode(&record.tag),
    ])
}

fn decode_source_record(record: &str) -> Result<(SourceId, EncryptedRecord), ConfidentialError> {
    let parts = decode_parts(record)?;
    let source_id = SourceId::new(required(&parts, 0, "source id")?);
    let event_record = encode_parts(&[
        required(&parts, 1, "sequence")?.to_owned(),
        required(&parts, 2, "key id")?.to_owned(),
        required(&parts, 3, "nonce")?.to_owned(),
        required(&parts, 4, "ciphertext")?.to_owned(),
        required(&parts, 5, "tag")?.to_owned(),
    ]);
    Ok((source_id, decode_event_record(&event_record)?))
}

fn deterministic_noise(seed: u64, label: &str, epsilon: f64) -> f64 {
    let hash = stable_hash(seed, label.as_bytes());
    let bucket = (hash % 2_001) as f64;
    let mut centered = bucket / 1_000.0 - 1.0;
    if centered == 0.0 {
        centered = 0.5;
    }
    centered / epsilon
}

fn stable_hash(mut seed: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        seed ^= u64::from(*byte);
        seed = seed.wrapping_mul(0x1000_0000_01b3);
        seed ^= seed >> 32;
    }
    splitmix64(seed)
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn hex_decode(value: &str) -> Result<Vec<u8>, ConfidentialError> {
    if value.len() % 2 != 0 {
        return Err(ConfidentialError::Codec(
            "hex value has odd length".to_owned(),
        ));
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    let raw = value.as_bytes();
    for index in (0..raw.len()).step_by(2) {
        let high = hex_nibble(raw[index])?;
        let low = hex_nibble(raw[index + 1])?;
        bytes.push((high << 4) | low);
    }
    Ok(bytes)
}

fn hex_nibble(byte: u8) -> Result<u8, ConfidentialError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(ConfidentialError::Codec("invalid hex digit".to_owned())),
    }
}

fn encode_parts(parts: &[String]) -> String {
    let mut encoded = String::new();
    for part in parts {
        encoded.push_str(&part.len().to_string());
        encoded.push(':');
        encoded.push_str(part);
    }
    encoded
}

fn decode_parts(record: &str) -> Result<Vec<String>, ConfidentialError> {
    let mut parts = Vec::new();
    let mut index = 0;
    while index < record.len() {
        let colon = record[index..]
            .find(':')
            .ok_or_else(|| ConfidentialError::Codec("missing part delimiter".to_owned()))?
            + index;
        let len = record[index..colon]
            .parse::<usize>()
            .map_err(|error| ConfidentialError::Codec(error.to_string()))?;
        let start = colon + 1;
        let end = start + len;
        if end > record.len() {
            return Err(ConfidentialError::Codec(
                "part length exceeds record".to_owned(),
            ));
        }
        parts.push(record[start..end].to_owned());
        index = end;
    }
    Ok(parts)
}

fn required<'a>(
    parts: &'a [String],
    index: usize,
    field: &str,
) -> Result<&'a str, ConfidentialError> {
    parts
        .get(index)
        .map(String::as_str)
        .ok_or_else(|| ConfidentialError::Codec(format!("missing {field}")))
}

fn io_error(error: std::io::Error) -> ConfidentialError {
    ConfidentialError::Io(error.to_string())
}
