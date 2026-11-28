use super::file::ArtifactFile;
use foxfire_acp::artifact::{Artifact, ArtifactId, ArtifactSlot};
use uuid::Uuid;

#[derive(Default)]
pub(crate) struct SessionArtifacts(Vec<Shown>);

pub(crate) enum Recorded<'a> {
    Unchanged(&'a Shown),
    Added(&'a Shown),
    Replaced(&'a Shown, ArtifactId),
}

pub(crate) struct Shown {
    file: ArtifactFile,
    slot: ArtifactSlot,
    pub(crate) version: u32,
    title: String,
    pub(crate) content: ArtifactId,
}

impl SessionArtifacts {
    pub(crate) fn record(&mut self, file: ArtifactFile, title: String, content: ArtifactId) -> Recorded<'_> {
        let Some(index) = self.0.iter().position(|shown| shown.file == file) else {
            let slot = ArtifactSlot::new(Uuid::new_v4().to_string());
            self.0.push(Shown { file, slot, version: 1, title, content });
            return Recorded::Added(&self.0[self.0.len() - 1]);
        };
        let shown = &mut self.0[index];
        if shown.content == content {
            return Recorded::Unchanged(shown);
        }
        let replaced = shown.content;
        (shown.version, shown.title, shown.content) = (shown.version + 1, title, content);
        Recorded::Replaced(shown, replaced)
    }

    pub(crate) fn shows(&self, content: &ArtifactId) -> bool {
        self.0.iter().any(|shown| shown.content == *content)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &Shown> {
        self.0.iter()
    }
}

impl Shown {
    pub(crate) fn artifact(&self) -> Artifact {
        Artifact {
            slot: self.slot.clone(),
            version: self.version,
            title: self.title.clone(),
            name: self.file.name(),
            kind: self.file.kind(),
            id: self.content,
        }
    }
}
