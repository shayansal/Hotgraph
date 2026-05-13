use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rg_storage::{
    deterministic_state_hash, BackupReader, BackupWriter, RedbGraphStore, StorageError,
};

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn run(args: Vec<String>) -> Result<String, String> {
    let command = CliCommand::parse(args)?;
    match command {
        CliCommand::BackupCreate { store, output } => backup_create(&store, &output),
        CliCommand::BackupVerify { input } => backup_verify(&input),
        CliCommand::Restore { input, target } => restore_backup(&input, &target),
        CliCommand::RestoreVerify { input } => restore_verify(&input),
    }
    .map_err(|error| format!("{error:?}"))
}

#[derive(Debug, Eq, PartialEq)]
enum CliCommand {
    BackupCreate { store: PathBuf, output: PathBuf },
    BackupVerify { input: PathBuf },
    Restore { input: PathBuf, target: PathBuf },
    RestoreVerify { input: PathBuf },
}

impl CliCommand {
    fn parse(args: Vec<String>) -> Result<Self, String> {
        match args.as_slice() {
            [group, action, rest @ ..] if group == "backup" && action == "create" => {
                let store = required_path(rest, "--store")?;
                let output = required_path(rest, "--output")?;
                Ok(Self::BackupCreate { store, output })
            }
            [group, action, rest @ ..] if group == "backup" && action == "verify" => {
                let input = required_path(rest, "--input")?;
                Ok(Self::BackupVerify { input })
            }
            [command, rest @ ..] if command == "restore" => {
                if rest.first().is_some_and(|value| value == "verify") {
                    let input = required_path(&rest[1..], "--input")?;
                    return Ok(Self::RestoreVerify { input });
                }
                let input = required_path(rest, "--input")?;
                let target = required_path(rest, "--target")?;
                Ok(Self::Restore { input, target })
            }
            _ => Err(usage()),
        }
    }
}

fn backup_create(store_path: &Path, output_path: &Path) -> Result<String, StorageError> {
    let store = RedbGraphStore::open(store_path)?;
    let storage = store.materialized_storage()?;
    let manifest = BackupWriter::write(output_path, &storage)?;
    Ok(format!(
        "backup_created path={} events={} state_hash={}",
        output_path.display(),
        manifest.event_count,
        manifest.graph_state_hash
    ))
}

fn backup_verify(input_path: &Path) -> Result<String, StorageError> {
    let report = BackupReader::restore_report(input_path)?;
    Ok(format!(
        "backup_verified path={} events={} state_hash={} query_parity={}",
        input_path.display(),
        report.manifest.event_count,
        report.restored_state_hash,
        report.query_parity_checked
    ))
}

fn restore_backup(input_path: &Path, target_dir: &Path) -> Result<String, StorageError> {
    ensure_clean_target_dir(target_dir)?;
    let restored_path = target_dir.join("hotgraph.redb");
    let restored = BackupReader::restore(input_path)?;
    let expected_hash = deterministic_state_hash(&restored);
    let mut store = RedbGraphStore::create(&restored_path)?;
    for event in restored.events() {
        store.append_event(event, None)?;
    }
    let actual = store.materialized_storage()?;
    let actual_hash = deterministic_state_hash(&actual);
    if actual_hash != expected_hash {
        return Err(StorageError::SnapshotMismatch);
    }
    Ok(format!(
        "restore_completed store={} events={} state_hash={}",
        restored_path.display(),
        restored.events().len(),
        actual_hash
    ))
}

fn restore_verify(input_path: &Path) -> Result<String, StorageError> {
    let report = BackupReader::restore_report(input_path)?;
    Ok(format!(
        "restore_verified path={} events={} state_hash={} query_parity={}",
        input_path.display(),
        report.manifest.event_count,
        report.restored_state_hash,
        report.query_parity_checked
    ))
}

fn ensure_clean_target_dir(target_dir: &Path) -> Result<(), StorageError> {
    if target_dir.exists() {
        if !target_dir.is_dir() {
            return Err(StorageError::Codec(format!(
                "target path must be a directory: {}",
                target_dir.display()
            )));
        }
        let mut entries = fs::read_dir(target_dir).map_err(io_error)?;
        if entries.next().transpose().map_err(io_error)?.is_some() {
            return Err(StorageError::Codec(format!(
                "target directory must be empty: {}",
                target_dir.display()
            )));
        }
        return Ok(());
    }
    fs::create_dir_all(target_dir).map_err(io_error)
}

fn required_path(args: &[String], flag: &str) -> Result<PathBuf, String> {
    args.windows(2)
        .find_map(|window| (window[0] == flag).then(|| PathBuf::from(&window[1])))
        .ok_or_else(|| format!("missing required flag {flag}\n{}", usage()))
}

fn usage() -> String {
    "usage:
  hotgraph backup create --store <redb-path> --output <backup-file>
  hotgraph backup verify --input <backup-file>
  hotgraph restore --input <backup-file> --target <clean-dir>
  hotgraph restore verify --input <backup-file>"
        .to_owned()
}

fn io_error(error: std::io::Error) -> StorageError {
    StorageError::Io(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{CliCommand, PathBuf};

    #[test]
    fn parses_backup_create_command() {
        assert_eq!(
            CliCommand::parse(vec![
                "backup".to_owned(),
                "create".to_owned(),
                "--store".to_owned(),
                "graph.redb".to_owned(),
                "--output".to_owned(),
                "graph.backup".to_owned(),
            ])
            .expect("parse"),
            CliCommand::BackupCreate {
                store: PathBuf::from("graph.redb"),
                output: PathBuf::from("graph.backup"),
            }
        );
    }

    #[test]
    fn parses_restore_verify_command() {
        assert_eq!(
            CliCommand::parse(vec![
                "restore".to_owned(),
                "verify".to_owned(),
                "--input".to_owned(),
                "graph.backup".to_owned(),
            ])
            .expect("parse"),
            CliCommand::RestoreVerify {
                input: PathBuf::from("graph.backup"),
            }
        );
    }
}
