use foxfire_acp::artifact::{ArtifactContent, ImageFormat};
use foxfire_acp::testing::run_local;
use foxfire_artifacts::store::{ArtifactStore, StoreError, TmpDir};

#[test]
fn content_is_read_back_as_the_kind_it_was_kept_as() -> Result<(), StoreError> {
    run_local(async {
        let mut store = TmpDir::new()?;
        let page = ArtifactContent::Html("<h1>Pricing</h1>".to_owned());
        let chart = ArtifactContent::Image(ImageFormat::Png, vec![0x89, b'P', b'N', b'G'].into());
        let (page_id, chart_id) = (store.put(page.clone()).await?, store.put(chart.clone()).await?);
        assert_eq!((store.get(&page_id).await?, store.get(&chart_id).await?), (page, chart));
        Ok(())
    })
}

#[test]
fn the_same_content_is_kept_once_and_forgotten_once() -> Result<(), StoreError> {
    run_local(async {
        let mut store = TmpDir::new()?;
        let page = ArtifactContent::Html("<h1>Pricing</h1>".to_owned());
        let id = store.put(page.clone()).await?;
        assert_eq!(store.put(page).await?, id);

        store.remove(&id).await?;
        assert!(matches!(store.get(&id).await, Err(StoreError::Missing(missing)) if missing == id));
        store.remove(&id).await?;
        Ok(())
    })
}
