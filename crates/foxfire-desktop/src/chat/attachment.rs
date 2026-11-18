use crate::ui::tokens::{HAIRLINE, ICON_SIZE_SMALL, Radius, Space};
use crate::ui::{Icon, IconName, UiStyles};
use agent_client_protocol::v2::schema as acp;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use foxfire_core::composer::attachment::{AttachmentKind, PromptAttachment, classify_attachment};
use rouge::core::{AnyView, Div, Fit, ImageSource, Role, Styled, View, div, image, text, view};
use std::sync::Arc;

#[derive(Clone, PartialEq)]
pub(crate) enum Attachment {
    Image(Arc<[u8]>),
    File { icon: IconName, name: String },
}

impl Attachment {
    pub(crate) fn of(block: &acp::ContentBlock) -> Option<Self> {
        let file = |icon, name: &str| Some(Self::File { icon, name: name.to_owned() });
        match block {
            acp::ContentBlock::Text(_) => None,
            acp::ContentBlock::Image(image) => {
                decode(image).map(Self::Image).or_else(|| file(IconName::Image, "Image"))
            }
            acp::ContentBlock::Audio(_) => file(IconName::AudioLines, "Audio"),
            acp::ContentBlock::ResourceLink(link) => file(IconName::File, &link.name),
            acp::ContentBlock::Resource(resource) => file(IconName::File, file_name(resource)),
            _ => file(IconName::File, "Attachment"),
        }
    }
}

pub(crate) fn chip(id: String, preview: impl View, name: &str) -> Div {
    view! {
        <div test_id=id flex items_center gap=Space::X1 min_w=0.0 px=Space::X2 py=Space::X1 rounded=Radius::Inner
             bg=Role::SurfaceSelected unselectable>
            {preview}
            <div min_w=0.0 overflow_hidden><text small secondary no_wrap>{name.to_owned()}</text></div>
        </div>
    }
}

pub(crate) fn chip_icon(icon: IconName) -> Icon {
    Icon::new(icon, Role::TextMuted).size(ICON_SIZE_SMALL)
}

pub(crate) fn thumbnail(id: String, source: ImageSource, size: f32) -> Div {
    view! {
        <div test_id=id flex_none size=size rounded=Radius::Inner overflow_hidden border=HAIRLINE
             border_color=Role::Border>
            {image(source).fit(Fit::Cover).size_full()}
        </div>
    }
}

pub(crate) fn preview(id: String, attachment: &PromptAttachment, size: f32) -> AnyView {
    match classify_attachment(&attachment.path) {
        AttachmentKind::Audio => div().flex().child(chip_icon(IconName::AudioLines)).into_any(),
        _ => thumbnail(id, ImageSource::File(attachment.path.clone()), size).into_any(),
    }
}

fn decode(image: &acp::ImageContent) -> Option<Arc<[u8]>> {
    matches!(&*image.mime_type.0, "image/png" | "image/jpeg" | "image/jpg")
        .then(|| BASE64.decode(&image.data).ok())
        .flatten()
        .map(Arc::from)
}

fn file_name(resource: &acp::EmbeddedResource) -> &str {
    let uri = match &resource.resource {
        acp::EmbeddedResourceResource::TextResourceContents(text) => text.uri.as_str(),
        acp::EmbeddedResourceResource::BlobResourceContents(blob) => blob.uri.as_str(),
        _ => "Attachment",
    };
    uri.rsplit('/').next().filter(|name| !name.is_empty()).unwrap_or(uri)
}
