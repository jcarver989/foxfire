use std::env::temp_dir;
use std::fs::remove_dir_all;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// A uniquely named directory under the system temp dir, created by whoever writes into it and removed on drop.
#[derive(Debug)]
pub(crate) struct TempDir(PathBuf);

impl TempDir {
    pub(crate) fn new() -> Self {
        Self(temp_dir().join(format!("foxfire-{}", Uuid::new_v4())))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Default for TempDir {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = remove_dir_all(&self.0);
    }
}
