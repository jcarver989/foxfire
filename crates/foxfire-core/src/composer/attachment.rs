use crate::session_model::WorkspaceAccess;
use agent_client_protocol::v2::schema as acp;
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptAttachment {
    pub path: PathBuf,
    pub display_name: String,
    pub origin: AttachmentOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentOrigin {
    Client,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachedFile {
    Media(PromptAttachment),
    Mention(PromptAttachment),
    Unsupported(PromptAttachment),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    Text,
    Image,
    Audio,
    Unsupported,
}

#[derive(Debug, Default)]
pub struct AttachmentOutcome {
    pub blocks: Vec<acp::ContentBlock>,
    pub warnings: Vec<String>,
}

impl PromptAttachment {
    pub fn new(path: PathBuf, root: &Path) -> Self {
        let name = path.strip_prefix(root).ok().or_else(|| path.file_name().map(Path::new)).unwrap_or(&path);
        let display_name = name.to_string_lossy().replace('\\', "/");
        Self { path, display_name, origin: AttachmentOrigin::Client }
    }

    pub fn in_workspace(path: PathBuf, root: &Path) -> Self {
        Self { origin: AttachmentOrigin::Agent, ..Self::new(path, root) }
    }

    /// Names a file that only the agent can read.
    pub(crate) fn link(&self, access: WorkspaceAccess) -> Option<acp::ContentBlock> {
        (access == WorkspaceAccess::Remote && self.origin == AttachmentOrigin::Agent).then(|| {
            let mut url = Url::parse("file:///").expect("a valid base URL");
            url.set_path(&self.path.to_string_lossy());
            acp::ContentBlock::ResourceLink(acp::ResourceLink::new(self.display_name.clone(), url.to_string()))
        })
    }
}

pub fn attach_missing(attachments: &mut Vec<PromptAttachment>, added: impl IntoIterator<Item = PromptAttachment>) {
    for attachment in added {
        if attachments.iter().all(|existing| existing.path != attachment.path) {
            attachments.push(attachment);
        }
    }
}

pub fn classify_attachment(path: &Path) -> AttachmentKind {
    let mime = mime_guess::from_path(path).first_or_octet_stream().to_string();
    if IMAGE_MIME_TYPES.contains(&mime.as_str()) {
        AttachmentKind::Image
    } else if AUDIO_MIME_TYPES.contains(&mime.as_str()) {
        AttachmentKind::Audio
    } else if mime.starts_with("text/") {
        AttachmentKind::Text
    } else {
        AttachmentKind::Unsupported
    }
}

const IMAGE_MIME_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];
const AUDIO_MIME_TYPES: &[&str] = &["audio/wav", "audio/mpeg", "audio/mp3", "audio/ogg"];
