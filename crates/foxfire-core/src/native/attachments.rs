use crate::composer::attachment::{
    AttachedFile, AttachmentKind, AttachmentOutcome, PromptAttachment, classify_attachment,
};
use crate::session_model::WorkspaceAccess;
use agent_client_protocol::v2::schema as acp;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use std::io::Read;
use std::path::{Path, PathBuf};
use url::Url;

/// Classifies dropped or picked paths using the local filesystem.
pub fn classify_paths(paths: impl IntoIterator<Item = PathBuf>, root: &Path) -> Vec<AttachedFile> {
    paths
        .into_iter()
        .map(|path| {
            let kind = if path.is_file() { classify_attachment(&path) } else { AttachmentKind::Unsupported };
            let attachment = PromptAttachment::new(path, root);
            match kind {
                AttachmentKind::Image | AttachmentKind::Audio => AttachedFile::Media(attachment),
                AttachmentKind::Text => AttachedFile::Mention(attachment),
                AttachmentKind::Unsupported => AttachedFile::Unsupported(attachment),
            }
        })
        .collect()
}

/// Reads at most `limit` bytes, so truncation checks never pull an unbounded file into memory.
fn read_capped(path: &Path, display_name: &str, limit: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|error| format!("Failed to read {display_name}: {error}"))?
        .take(limit as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Failed to read {display_name}: {error}"))?;
    Ok(bytes)
}

fn file_uri(path: &Path, display_name: &str) -> Result<String, String> {
    let uri_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    Url::from_file_path(uri_path)
        .map(|url| url.to_string())
        .map_err(|()| format!("Failed to build file URI for {display_name}"))
}

/// Encodes local or agent-owned attachments for the active workspace.
pub fn build_attachments(attachments: &[PromptAttachment], access: WorkspaceAccess) -> AttachmentOutcome {
    let mut outcome = AttachmentOutcome::default();
    for attachment in attachments {
        if let Some(link) = attachment.link(access) {
            outcome.blocks.push(link);
            continue;
        }
        let encoded = read_capped(&attachment.path, &attachment.display_name, MAX_MEDIA_BYTES + 1)
            .and_then(|bytes| encode_attachment(attachment, access, bytes));
        match encoded {
            Ok((block, warning)) => {
                outcome.blocks.push(block);
                outcome.warnings.extend(warning);
            }
            Err(warning) => outcome.warnings.push(warning),
        }
    }
    outcome
}

const MAX_EMBED_TEXT_BYTES: usize = 1024 * 1024;
const MAX_MEDIA_BYTES: usize = 5 * 1024 * 1024;
/// Pure encoding of one attachment from its capped bytes.
fn encode_attachment(
    attachment: &PromptAttachment,
    access: WorkspaceAccess,
    bytes: Vec<u8>,
) -> Result<(acp::ContentBlock, Option<String>), String> {
    let PromptAttachment { path, display_name, .. } = attachment;
    let mime_type = mime_guess::from_path(path).first_or_octet_stream().to_string();
    match classify_attachment(path) {
        kind @ (AttachmentKind::Image | AttachmentKind::Audio) => {
            encode_media_block(&bytes, display_name, &mime_type, kind).map(|block| (block, None))
        }
        AttachmentKind::Text | AttachmentKind::Unsupported => {
            let uri = match access {
                WorkspaceAccess::Local => file_uri(path, display_name)?,
                WorkspaceAccess::Remote => attached_uri(display_name),
            };
            encode_text_block(bytes, uri, display_name, &mime_type)
        }
    }
}

fn encode_media_block(
    bytes: &[u8],
    display_name: &str,
    mime_type: &str,
    kind: AttachmentKind,
) -> Result<acp::ContentBlock, String> {
    if bytes.len() > MAX_MEDIA_BYTES {
        return Err(format!("Skipped {display_name}: file too large (max {MAX_MEDIA_BYTES})"));
    }
    let data = BASE64.encode(bytes);
    Ok(if kind == AttachmentKind::Image {
        acp::ContentBlock::Image(acp::ImageContent::new(data, mime_type))
    } else {
        acp::ContentBlock::Audio(acp::AudioContent::new(data, mime_type))
    })
}

fn encode_text_block(
    mut bytes: Vec<u8>,
    uri: String,
    display_name: &str,
    mime_type: &str,
) -> Result<(acp::ContentBlock, Option<String>), String> {
    let truncated = bytes.len() > MAX_EMBED_TEXT_BYTES;
    if truncated {
        bytes.truncate(MAX_EMBED_TEXT_BYTES);
    }

    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text.to_string(),
        // Truncation can only split the final multi-byte code point, so back up to the
        // last complete boundary. A definite invalid sequence (error_len) means the file
        // is genuinely non-UTF-8 and must be rejected rather than silently truncated.
        Err(error) if truncated && error.error_len().is_none() => {
            std::str::from_utf8(&bytes[..error.valid_up_to()]).expect("valid_up_to marks a UTF-8 boundary").to_string()
        }
        Err(_) => return Err(format!("Skipped binary or non-UTF8 file: {display_name}")),
    };

    let warning = truncated.then(|| format!("Truncated {display_name} to {MAX_EMBED_TEXT_BYTES} bytes"));
    Ok((
        acp::ContentBlock::Resource(acp::EmbeddedResource::new(acp::EmbeddedResourceResource::TextResourceContents(
            acp::TextResourceContents::new(text, uri).mime_type(mime_type),
        ))),
        warning,
    ))
}

fn attached_uri(display_name: &str) -> String {
    let mut url = Url::parse("attachment:///").expect("a valid base URL");
    url.set_path(display_name);
    url.to_string()
}
