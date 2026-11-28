use super::error::ArtifactError;
use bytes::Bytes;
use foxfire_acp::artifact::{ArtifactContent, ArtifactKind};
use std::path::{Path, PathBuf};
use tokio::fs;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArtifactFile {
    path: PathBuf,
    kind: ArtifactKind,
}

pub(crate) struct Snapshot {
    pub(crate) file: ArtifactFile,
    pub(crate) title: String,
    pub(crate) content: ArtifactContent,
}

impl ArtifactFile {
    pub(crate) async fn locate(location: &str) -> Result<Self, ArtifactError> {
        let path = Path::new(location);
        let path = if path.is_absolute() {
            path.to_owned()
        } else {
            Url::parse(location)
                .ok()
                .filter(|url| url.scheme() == "file")
                .and_then(|url| url.to_file_path().ok())
                .ok_or_else(|| ArtifactError::Location(location.to_owned()))?
        };
        let path = fs::canonicalize(&path).await.map_err(|source| ArtifactError::Read { path, source })?;
        let extension = path.extension().unwrap_or_default().to_string_lossy();
        match ArtifactKind::from_extension(&extension) {
            Ok(kind) => Ok(Self { path, kind }),
            Err(source) => Err(ArtifactError::Content { path, source }),
        }
    }

    pub(crate) fn kind(&self) -> ArtifactKind {
        self.kind
    }

    pub(crate) fn name(&self) -> String {
        self.path.file_name().unwrap_or_default().to_string_lossy().into_owned()
    }

    pub(crate) async fn read(self, title: Option<String>) -> Result<Snapshot, ArtifactError> {
        let bytes =
            fs::read(&self.path).await.map_err(|source| ArtifactError::Read { path: self.path.clone(), source })?;
        let content = ArtifactContent::decode(self.kind, Bytes::from(bytes))
            .map_err(|source| ArtifactError::Content { path: self.path.clone(), source })?;
        let title = title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| self.name());
        Ok(Snapshot { file: self, title, content })
    }
}
