use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum JsonFileError {
    #[error("failed to access {}: {source}", .path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error("invalid JSON in {}: {source}", .path.display())]
    Json { path: PathBuf, source: serde_json::Error },
}

impl JsonFileError {
    pub fn path(&self) -> &Path {
        match self {
            Self::Io { path, .. } | Self::Json { path, .. } => path,
        }
    }
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, JsonFileError> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|source| JsonFileError::Json { path: path.to_path_buf(), source }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(JsonFileError::Io { path: path.to_path_buf(), source }),
    }
}

pub fn write_json_atomic<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), JsonFileError> {
    let mut bytes =
        serde_json::to_vec_pretty(value).map_err(|source| JsonFileError::Json { path: path.to_path_buf(), source })?;
    bytes.push(b'\n');
    write_atomic(path, &bytes).map_err(|source| JsonFileError::Io { path: path.to_path_buf(), source })
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = create_parent_dir(path)?;
    let mut temp = NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    if let Ok(existing) = fs::metadata(path) {
        temp.as_file().set_permissions(existing.permissions())?;
    }
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn create_parent_dir(path: &Path) -> io::Result<&Path> {
    let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    Ok(parent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn json_round_trips_and_missing_files_read_as_none() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nested/data.json");

        assert_eq!(read_json::<Vec<u32>>(&path).unwrap(), None);
        write_json_atomic(&path, &[1, 2]).unwrap();

        assert_eq!(read_json::<Vec<u32>>(&path).unwrap(), Some(vec![1, 2]));
        assert!(fs::read_to_string(&path).unwrap().ends_with("]\n"));
    }

    #[test]
    fn read_json_reports_malformed_files_with_their_path() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.json");
        fs::write(&path, "{").unwrap();

        let error = read_json::<Vec<u32>>(&path).unwrap_err();

        assert!(matches!(error, JsonFileError::Json { path: ref failed, .. } if failed == &path));
    }

    #[test]
    fn writes_create_parents_and_replace_contents() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nested/file.json");

        write_json_atomic(&path, "first").unwrap();
        write_json_atomic(&path, "second").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "\"second\"\n");
        let entries: Vec<_> = fs::read_dir(path.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(entries, ["file.json"], "temp files should not be left behind");
    }

    #[cfg(unix)]
    #[test]
    fn writes_create_private_files_and_keep_existing_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("file");
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;

        write_json_atomic(&path, "first").unwrap();
        assert_eq!(mode(&path), 0o600);

        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        write_json_atomic(&path, "second").unwrap();
        assert_eq!(mode(&path), 0o640);
    }

    #[test]
    fn failed_writes_leave_no_temp_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("target");
        fs::create_dir_all(path.join("occupied")).unwrap();

        assert!(write_json_atomic(&path, "data").is_err());

        let entries: Vec<_> = fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(entries, ["target"], "the temp file should be removed after a failed rename");
    }
}
