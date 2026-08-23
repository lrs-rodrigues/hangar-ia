//! Offline operational commands. A backup is a filesystem snapshot only after
//! the source redb file has been opened successfully, which also proves no
//! other Hangar process owns that data directory. This is intentionally an
//! offline procedure: copying a live data volume cannot provide a consistent
//! cross-file snapshot of redb, blobs, and rebuildable projections.

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::Context;
use redb::Database;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const BACKUP_FORMAT: &str = "hangar.backup.v1";
const MANIFEST_FILE: &str = "hangar-backup-manifest.json";

#[derive(Debug, Serialize, Deserialize)]
struct BackupManifest {
    format: String,
    created_at_unix_ms: u128,
    files: Vec<BackupFile>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BackupFile {
    path: String,
    bytes: u64,
    sha256: String,
}

pub fn create_backup(source: &Path, destination: &Path) -> anyhow::Result<()> {
    validate_live_data_directory(source)?;
    anyhow::ensure!(
        !destination.exists(),
        "backup destination already exists: {}",
        destination.display()
    );
    let temporary = sibling_temporary_path(destination)?;
    anyhow::ensure!(
        !temporary.exists(),
        "temporary backup path already exists: {}",
        temporary.display()
    );
    fs::create_dir_all(&temporary)?;
    let files = match copy_tree(source, &temporary, true) {
        Ok(files) => files,
        Err(error) => {
            let _ = fs::remove_dir_all(&temporary);
            return Err(error);
        }
    };
    let manifest = BackupManifest {
        format: BACKUP_FORMAT.to_owned(),
        created_at_unix_ms: now_unix_ms()?,
        files,
    };
    let manifest_path = temporary.join(MANIFEST_FILE);
    write_json_sync(&manifest_path, &manifest)?;
    fs::rename(&temporary, destination).with_context(|| {
        format!(
            "atomically publishing backup from {} to {}",
            temporary.display(),
            destination.display()
        )
    })?;
    Ok(())
}

pub fn verify_backup(source: &Path) -> anyhow::Result<()> {
    let manifest = read_manifest(source)?;
    anyhow::ensure!(
        manifest.format == BACKUP_FORMAT,
        "unsupported backup format"
    );
    for file in &manifest.files {
        let relative = validated_relative_path(&file.path)?;
        let path = source.join(relative);
        anyhow::ensure!(path.is_file(), "backup file is missing: {}", file.path);
        let metadata = fs::metadata(&path)?;
        anyhow::ensure!(
            metadata.len() == file.bytes,
            "backup file size differs: {}",
            file.path
        );
        anyhow::ensure!(
            hash_file(&path)? == file.sha256,
            "backup file checksum differs: {}",
            file.path
        );
    }
    validate_live_data_directory(source)?;
    Ok(())
}

pub fn restore_backup(source: &Path, destination: &Path) -> anyhow::Result<()> {
    verify_backup(source)?;
    anyhow::ensure!(
        !destination.exists(),
        "restore destination already exists: {}",
        destination.display()
    );
    let temporary = sibling_temporary_path(destination)?;
    anyhow::ensure!(
        !temporary.exists(),
        "temporary restore path already exists: {}",
        temporary.display()
    );
    fs::create_dir_all(&temporary)?;
    if let Err(error) = copy_tree(source, &temporary, false) {
        let _ = fs::remove_dir_all(&temporary);
        return Err(error);
    }
    validate_live_data_directory(&temporary)?;
    fs::rename(&temporary, destination).with_context(|| {
        format!(
            "atomically publishing restored data from {} to {}",
            temporary.display(),
            destination.display()
        )
    })?;
    Ok(())
}

fn validate_live_data_directory(directory: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        directory.is_dir(),
        "data directory does not exist: {}",
        directory.display()
    );
    let canonical = directory.join("canonical.redb");
    anyhow::ensure!(
        canonical.is_file(),
        "canonical.redb is missing from {}",
        directory.display()
    );
    // Opening redb acquires its process lock. A running Hangar instance makes
    // this fail rather than allowing a falsely consistent copy.
    let database = Database::open(&canonical)
        .with_context(|| format!("opening canonical database {}", canonical.display()))?;
    let transaction = database.begin_read()?;
    drop(transaction);
    Ok(())
}

fn copy_tree(
    source: &Path,
    destination: &Path,
    include_manifest: bool,
) -> anyhow::Result<Vec<BackupFile>> {
    let mut files = Vec::new();
    copy_tree_inner(source, source, destination, include_manifest, &mut files)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn copy_tree_inner(
    root: &Path,
    current: &Path,
    destination: &Path,
    include_manifest: bool,
    files: &mut Vec<BackupFile>,
) -> anyhow::Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        anyhow::ensure!(
            !file_type.is_symlink(),
            "backup refuses symlink: {}",
            path.display()
        );
        let relative = path.strip_prefix(root)?.to_path_buf();
        if !include_manifest && relative == Path::new(MANIFEST_FILE) {
            continue;
        }
        let target = destination.join(&relative);
        if file_type.is_dir() {
            fs::create_dir_all(&target)?;
            copy_tree_inner(root, &path, destination, include_manifest, files)?;
        } else if file_type.is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            copy_file_sync(&path, &target)?;
            files.push(BackupFile {
                path: relative_to_manifest_path(&relative)?,
                bytes: fs::metadata(&path)?.len(),
                sha256: hash_file(&path)?,
            });
        }
    }
    Ok(())
}

fn copy_file_sync(source: &Path, destination: &Path) -> anyhow::Result<()> {
    let mut input = fs::File::open(source)?;
    let mut output = fs::File::create(destination)?;
    std::io::copy(&mut input, &mut output)?;
    output.flush()?;
    output.sync_all()?;
    Ok(())
}

fn hash_file(path: &Path) -> anyhow::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn write_json_sync(path: &Path, value: &impl Serialize) -> anyhow::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(serde_json::to_string_pretty(value)?.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn read_manifest(source: &Path) -> anyhow::Result<BackupManifest> {
    let path = source.join(MANIFEST_FILE);
    let bytes =
        fs::read(&path).with_context(|| format!("reading backup manifest {}", path.display()))?;
    serde_json::from_slice(&bytes).context("parsing backup manifest")
}

fn sibling_temporary_path(destination: &Path) -> anyhow::Result<PathBuf> {
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .context("backup destination must have a normal directory name")?;
    Ok(destination.with_file_name(format!(".{name}.hangar-incomplete")))
}

fn validated_relative_path(value: &str) -> anyhow::Result<PathBuf> {
    let path = Path::new(value);
    anyhow::ensure!(path.is_relative(), "backup manifest path is absolute");
    anyhow::ensure!(
        !path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir)),
        "backup manifest path escapes its root"
    );
    Ok(path.to_path_buf())
}

fn relative_to_manifest_path(path: &Path) -> anyhow::Result<String> {
    path.to_str()
        .map(|value| value.replace('\\', "/"))
        .context("backup path is not valid UTF-8")
}

fn now_unix_ms() -> anyhow::Result<u128> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use redb::Database;
    use tempfile::tempdir;

    use super::{create_backup, restore_backup, verify_backup};

    #[test]
    fn backup_verifies_and_restores_a_complete_data_directory() {
        let temp = tempdir().expect("temporary directory");
        let source = temp.path().join("source");
        fs::create_dir_all(source.join("blobs")).expect("blob directory");
        Database::create(source.join("canonical.redb")).expect("database");
        fs::write(source.join("blobs/example"), b"knowledge").expect("blob");
        let backup = temp.path().join("backup");
        create_backup(&source, &backup).expect("backup");
        verify_backup(&backup).expect("verification");
        let restored = temp.path().join("restored");
        restore_backup(&backup, &restored).expect("restore");
        assert_eq!(
            fs::read(restored.join("blobs/example")).expect("restored blob"),
            b"knowledge"
        );
        Database::open(restored.join("canonical.redb")).expect("restored database");
    }

    #[test]
    fn verification_rejects_tampered_backup() {
        let temp = tempdir().expect("temporary directory");
        let source = temp.path().join("source");
        fs::create_dir_all(source.join("blobs")).expect("blob directory");
        Database::create(source.join("canonical.redb")).expect("database");
        fs::write(source.join("blobs/example"), b"knowledge").expect("blob");
        let backup = temp.path().join("backup");
        create_backup(&source, &backup).expect("backup");
        fs::write(backup.join("blobs/example"), b"tampered").expect("tamper");
        assert!(verify_backup(&backup).is_err());
    }
}
