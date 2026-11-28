use agent_client_protocol::v2::schema::SessionId;
use agent_client_protocol::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use bytes::Bytes;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub slot: ArtifactSlot,
    pub version: u32,
    pub title: String,
    pub name: String,
    #[schemars(with = "String")]
    pub kind: ArtifactKind,
    #[schemars(with = "String")]
    pub id: ArtifactId,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct ArtifactSlot(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct ArtifactId([u8; 32]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
#[non_exhaustive]
pub enum ArtifactKind {
    Html,
    Image(ImageFormat),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "WireContent", try_from = "WireContent")]
#[non_exhaustive]
pub enum ArtifactContent {
    Html(String),
    Image(ImageFormat, Bytes),
}

#[derive(Debug, Error)]
pub enum ContentError {
    #[error("`{0}` is not a kind of artifact")]
    UnsupportedType(String),
    #[error("HTML artifacts must be valid UTF-8")]
    InvalidUtf8,
    #[error("artifact data is not valid base64")]
    InvalidData,
}

#[derive(Debug, Error)]
#[error("`{0}` is not an artifact id")]
pub struct InvalidArtifactId(String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonRpcNotification)]
#[notification(method = "_foxfire/artifact")]
#[serde(rename_all = "camelCase")]
pub struct ArtifactPayload {
    pub session_id: SessionId,
    pub artifact: Artifact,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonRpcRequest)]
#[request(method = "_foxfire/artifact_fetch", response = ArtifactFetchResponse)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactFetchParams {
    pub session_id: SessionId,
    pub id: ArtifactId,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactFetchResponse {
    pub content: ArtifactContent,
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum WireContent {
    Html { html: String },
    Image { media_type: String, data: String },
}

const KINDS: [ArtifactKind; 5] = [
    ArtifactKind::Html,
    ArtifactKind::Image(ImageFormat::Png),
    ArtifactKind::Image(ImageFormat::Jpeg),
    ArtifactKind::Image(ImageFormat::Gif),
    ArtifactKind::Image(ImageFormat::Webp),
];

impl ArtifactSlot {
    pub fn new(unique: impl Into<String>) -> Self {
        Self(unique.into())
    }
}

impl Display for ArtifactId {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

impl FromStr for ArtifactId {
    type Err = InvalidArtifactId;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = || InvalidArtifactId(text.to_owned());
        if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        let mut digest = [0; 32];
        for (index, byte) in digest.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).map_err(|_| invalid())?;
        }
        Ok(Self(digest))
    }
}

impl From<ArtifactId> for String {
    fn from(id: ArtifactId) -> Self {
        id.to_string()
    }
}

impl TryFrom<String> for ArtifactId {
    type Error = InvalidArtifactId;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

impl ArtifactKind {
    pub fn from_extension(extension: &str) -> Result<Self, ContentError> {
        KINDS
            .into_iter()
            .find(|kind| kind.extensions().iter().any(|known| known.eq_ignore_ascii_case(extension)))
            .ok_or_else(|| ContentError::UnsupportedType(extension.to_owned()))
    }

    pub fn from_media_type(media_type: &str) -> Result<Self, ContentError> {
        KINDS
            .into_iter()
            .find(|kind| kind.media_type() == media_type)
            .ok_or_else(|| ContentError::UnsupportedType(media_type.to_owned()))
    }

    pub fn media_type(self) -> &'static str {
        match self {
            Self::Html => "text/html",
            Self::Image(ImageFormat::Png) => "image/png",
            Self::Image(ImageFormat::Jpeg) => "image/jpeg",
            Self::Image(ImageFormat::Gif) => "image/gif",
            Self::Image(ImageFormat::Webp) => "image/webp",
        }
    }

    fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Html => &["html", "htm"],
            Self::Image(ImageFormat::Png) => &["png"],
            Self::Image(ImageFormat::Jpeg) => &["jpg", "jpeg"],
            Self::Image(ImageFormat::Gif) => &["gif"],
            Self::Image(ImageFormat::Webp) => &["webp"],
        }
    }
}

impl From<ArtifactKind> for String {
    fn from(kind: ArtifactKind) -> Self {
        kind.media_type().to_owned()
    }
}

impl TryFrom<String> for ArtifactKind {
    type Error = ContentError;

    fn try_from(media_type: String) -> Result<Self, Self::Error> {
        Self::from_media_type(&media_type)
    }
}

impl ArtifactContent {
    pub fn decode(kind: ArtifactKind, bytes: Bytes) -> Result<Self, ContentError> {
        match kind {
            ArtifactKind::Html => {
                String::from_utf8(bytes.into()).map(Self::Html).map_err(|_| ContentError::InvalidUtf8)
            }
            ArtifactKind::Image(format) => Ok(Self::Image(format, bytes)),
        }
    }

    pub fn kind(&self) -> ArtifactKind {
        match self {
            Self::Html(_) => ArtifactKind::Html,
            Self::Image(format, _) => ArtifactKind::Image(*format),
        }
    }

    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Html(html) => html.as_bytes(),
            Self::Image(_, bytes) => bytes,
        }
    }

    pub fn id(&self) -> ArtifactId {
        let digest = Sha256::new()
            .chain_update(self.kind().media_type())
            .chain_update([0])
            .chain_update(self.bytes())
            .finalize();
        ArtifactId(digest.into())
    }

    pub fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.kind().media_type(), BASE64.encode(self.bytes()))
    }
}

impl From<ArtifactContent> for WireContent {
    fn from(content: ArtifactContent) -> Self {
        match content {
            ArtifactContent::Html(html) => Self::Html { html },
            ArtifactContent::Image(format, bytes) => Self::Image {
                media_type: ArtifactKind::Image(format).media_type().to_owned(),
                data: BASE64.encode(bytes),
            },
        }
    }
}

impl TryFrom<WireContent> for ArtifactContent {
    type Error = ContentError;

    fn try_from(wire: WireContent) -> Result<Self, Self::Error> {
        match wire {
            WireContent::Html { html } => Ok(Self::Html(html)),
            WireContent::Image { media_type, data } => {
                let ArtifactKind::Image(format) = ArtifactKind::from_media_type(&media_type)? else {
                    return Err(ContentError::UnsupportedType(media_type));
                };
                let bytes = BASE64.decode(data).map_err(|_| ContentError::InvalidData)?;
                Ok(Self::Image(format, bytes.into()))
            }
        }
    }
}
