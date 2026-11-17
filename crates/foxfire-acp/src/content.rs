use agent_client_protocol::v2::schema as acp;
use std::borrow::Cow;

pub fn display_content_blocks(blocks: &[acp::ContentBlock]) -> Vec<acp::ContentBlock> {
    blocks
        .iter()
        .map(|block| match block {
            acp::ContentBlock::Resource(resource) => {
                let uri = match &resource.resource {
                    acp::EmbeddedResourceResource::TextResourceContents(text) => &text.uri,
                    acp::EmbeddedResourceResource::BlobResourceContents(blob) => &blob.uri,
                    _ => return acp::ContentBlock::from("[Unknown resource type]"),
                };
                acp::ContentBlock::from(format!("[Resource: {uri}]"))
            }
            block => block.clone(),
        })
        .collect()
}

/// Converts ACP `ContentBlock` to plain text.
///
/// Embedded resources (e.g., file attachments) are formatted with their URI
/// and content for inclusion in the agent's context.
pub fn map_content_blocks_to_text(blocks: &[acp::ContentBlock]) -> String {
    blocks.iter().map(block_text).collect::<Vec<_>>().join("\n")
}

pub fn first_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|line| !line.is_empty())
}

pub fn prompt_title(text: Option<&str>) -> String {
    truncate_title(text.and_then(first_line).unwrap_or("Media prompt"))
}

pub fn truncate_title(text: &str) -> String {
    if text.len() <= MAX_TITLE_LEN {
        text.to_owned()
    } else {
        format!("{}…", &text[..text.floor_char_boundary(MAX_TITLE_LEN)])
    }
}

const MAX_TITLE_LEN: usize = 80;

pub fn first_text(blocks: &[acp::ContentBlock]) -> Option<&str> {
    blocks.iter().find_map(|block| match block {
        acp::ContentBlock::Text(text) => Some(text.text.trim()).filter(|text| !text.is_empty()),
        _ => None,
    })
}

/// Formats an embedded resource as text for inclusion in agent context.
pub fn format_embedded_resource(resource: &acp::EmbeddedResource) -> String {
    match &resource.resource {
        acp::EmbeddedResourceResource::TextResourceContents(text) => {
            format!("<file uri=\"{}\">\n{}\n</file>", text.uri, text.text)
        }
        acp::EmbeddedResourceResource::BlobResourceContents(blob) => {
            format!("[Binary resource: {}]", blob.uri)
        }
        _ => "[Unknown resource type]".to_string(),
    }
}

fn block_text(block: &acp::ContentBlock) -> Cow<'_, str> {
    match block {
        acp::ContentBlock::Text(text) => Cow::Borrowed(&text.text),
        acp::ContentBlock::Image(_) => Cow::Borrowed("[Image content]"),
        acp::ContentBlock::Audio(_) => Cow::Borrowed("[Audio content]"),
        acp::ContentBlock::ResourceLink(link) => Cow::Owned(format!("[Resource: {}]", link.uri)),
        acp::ContentBlock::Resource(resource) => Cow::Owned(format_embedded_resource(resource)),
        _ => Cow::Borrowed("[Unknown content]"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_embedded_resource_text() {
        let resource = acp::EmbeddedResource::new(acp::EmbeddedResourceResource::TextResourceContents(
            acp::TextResourceContents::new("let x = 1;", "file://test.rs"),
        ));

        let result = format_embedded_resource(&resource);

        assert_eq!(result, "<file uri=\"file://test.rs\">\nlet x = 1;\n</file>");
    }

    #[test]
    fn test_format_embedded_resource_blob() {
        let resource = acp::EmbeddedResource::new(acp::EmbeddedResourceResource::BlobResourceContents(
            acp::BlobResourceContents::new("base64data", "file://image.png"),
        ));

        let result = format_embedded_resource(&resource);

        assert_eq!(result, "[Binary resource: file://image.png]");
    }

    #[test]
    fn test_map_content_blocks_to_text_with_embedded_resource() {
        let blocks = vec![
            acp::ContentBlock::Text(acp::TextContent::new("Check this file:")),
            acp::ContentBlock::Resource(acp::EmbeddedResource::new(
                acp::EmbeddedResourceResource::TextResourceContents(
                    acp::TextResourceContents::new("pub fn hello() {}", "file://src/lib.rs").mime_type("text/x-rust"),
                ),
            )),
        ];

        let result = map_content_blocks_to_text(&blocks);

        assert!(result.contains("Check this file:"));
        assert!(result.contains("<file uri=\"file://src/lib.rs\">"));
        assert!(result.contains("pub fn hello() {}"));
        assert!(result.contains("</file>"));
    }

    #[test]
    fn test_map_content_blocks_text_only() {
        let blocks = vec![
            acp::ContentBlock::Text(acp::TextContent::new("Hello")),
            acp::ContentBlock::Text(acp::TextContent::new("World")),
        ];

        assert_eq!(map_content_blocks_to_text(&blocks), "Hello\nWorld");
    }

    #[test]
    fn test_map_content_blocks_empty() {
        assert_eq!(map_content_blocks_to_text(&[]), "");
    }

    #[test]
    fn test_map_content_blocks_resource_link() {
        let blocks = vec![acp::ContentBlock::ResourceLink(acp::ResourceLink::new("readme.md", "file://readme.md"))];

        let result = map_content_blocks_to_text(&blocks);
        assert_eq!(result, "[Resource: file://readme.md]");
    }
}
