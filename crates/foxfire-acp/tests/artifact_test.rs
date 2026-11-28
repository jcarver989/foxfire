use foxfire_acp::artifact::{Artifact, ArtifactContent, ArtifactId, ArtifactKind, ImageFormat};
use foxfire_acp::testing;
use serde_json::{Error, json};

#[test]
fn an_image_travels_as_base64_with_its_media_type() -> Result<(), Error> {
    let chart = ArtifactContent::Image(ImageFormat::Png, vec![0x89, b'P', b'N', b'G'].into());
    let wire = serde_json::to_value(&chart)?;
    assert_eq!(wire, json!({ "type": "image", "mediaType": "image/png", "data": "iVBORw==" }));
    assert_eq!(serde_json::from_value::<ArtifactContent>(wire)?, chart);
    Ok(())
}

#[test]
fn content_of_an_unknown_kind_is_refused() {
    let video = json!({ "type": "image", "mediaType": "video/mp4", "data": "" });
    assert!(serde_json::from_value::<ArtifactContent>(video).is_err());
    assert!(ArtifactKind::from_extension("mp4").is_err());
}

#[test]
fn content_is_named_by_what_it_is() {
    let page = ArtifactContent::Html("<b>hi</b>".to_owned());
    let same_bytes_as_image = ArtifactContent::Image(ImageFormat::Gif, page.bytes().to_vec().into());
    assert_eq!(page.id(), page.clone().id());
    assert_ne!(page.id(), same_bytes_as_image.id());
    assert_ne!(page.id(), ArtifactContent::Html("<b>bye</b>".to_owned()).id());
}

#[test]
fn an_announcement_names_its_content_by_kind_and_id() -> Result<(), Error> {
    let announced = testing::pricing_artifact(1);
    let wire = serde_json::to_value(&announced)?;
    assert_eq!(wire["kind"], json!("text/html"));
    assert_eq!(wire["id"], json!(testing::pricing_content(1).id().to_string()));
    assert_eq!(serde_json::from_value::<Artifact>(wire)?, announced);

    for malformed in [json!("pricing"), json!("zz".repeat(32)), json!(1)] {
        assert!(serde_json::from_value::<ArtifactId>(malformed).is_err());
    }
    Ok(())
}
