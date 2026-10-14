use crate::{LorgarError, Result};
use fs2::FileExt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub fn private_dir(path: &Path) -> Result<()> {
    match fs::create_dir(path) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e.into()),
    }
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } {
        return Err(LorgarError::Config(format!(
            "{} must be an owned private directory (0700)",
            path.display()
        )));
    }
    if meta.mode() & 0o077 != 0 {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
    }
    let meta = fs::symlink_metadata(path)?;
    if meta.mode() & 0o077 != 0 {
        return Err(LorgarError::Config(format!(
            "{} must be an owned private directory (0700)",
            path.display()
        )));
    }
    Ok(())
}

pub fn runtime_dir() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("lorgar-{}", unsafe { libc::geteuid() }))
        });
    if !base.is_absolute() {
        return Err(LorgarError::Config(
            "XDG_RUNTIME_DIR must be absolute".into(),
        ));
    }
    private_dir(&base)?;
    let path = base.join("lorgar");
    private_dir(&path)?;
    Ok(path)
}

pub struct FileLock(#[allow(dead_code)] File);

impl FileLock {
    pub fn acquire(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)?;
        file.try_lock_exclusive()
            .map_err(|_| LorgarError::DeviceBusy)?;
        Ok(Self(file))
    }
}

pub fn atomic_save(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        private_dir(parent)?;
    }
    let temp = tempfile::Builder::new()
        .prefix(".lorgar-atomic-")
        .tempfile_in(path.parent().unwrap_or_else(|| Path::new(".")))?;
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o600))?;
    let mut file = temp.as_file();
    file.write_all(bytes)?;
    file.sync_all()?;
    temp.persist(path)
        .map_err(|e| LorgarError::Config(format!("atomic persist failed: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locking_is_exclusive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lock");
        let first = FileLock::acquire(&path).unwrap();
        assert!(matches!(
            FileLock::acquire(&path),
            Err(LorgarError::DeviceBusy)
        ));
        drop(first);
        assert!(FileLock::acquire(&path).is_ok());
    }

    #[test]
    fn atomic_replacement_and_abandoned_write() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("file.json");
        atomic_save(&target, b"{\"a\":1}").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"{\"a\":1}");
        let _ = std::panic::catch_unwind(|| {
            let _temp = tempfile::Builder::new()
                .tempfile_in(target.parent().unwrap())
                .unwrap();
            panic!("crash during save");
            #[allow(unreachable_code)]
            let _ = _temp;
        });
        assert_eq!(fs::read(&target).unwrap(), b"{\"a\":1}");
    }
}
