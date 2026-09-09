use crate::{LorgarError, Result};
use fs2::FileExt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub fn private_dir(path: &Path) -> Result<()> {
    match fs::create_dir(path) {
        Ok(()) => fs::set_permissions(path, fs::Permissions::from_mode(0o700))?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e.into()),
    }
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
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

pub struct FileLock(File);

impl FileLock {
    pub fn acquire(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        file.try_lock_exclusive().map_err(|e| {
            if e.kind() == std::io::ErrorKind::WouldBlock {
                LorgarError::DeviceBusy
            } else {
                e.into()
            }
        })?;
        Ok(Self(file))
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

pub fn atomic_save(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| LorgarError::Config("file needs a parent directory".into()))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atomic_replacement_and_abandoned_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        atomic_save(&path, b"old").unwrap();
        let mut abandoned = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        abandoned.write_all(b"partial").unwrap();
        drop(abandoned);
        assert_eq!(fs::read(&path).unwrap(), b"old");
        atomic_save(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(fs::metadata(path).unwrap().mode() & 0o777, 0o600);
    }
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
}
