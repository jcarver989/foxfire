use foxfire_ui::backend::Project;
use foxfire_ui::memory::MemoryBackend;
use foxfire_ui::platform::ChatPlatform;
use foxfire_ui::ui::ActiveReviewTheme;
use foxfire_web::web_view;
use rouge::core::testing::{TestError, TestUi};
use rouge::core::{Appearance, InputKind, Point};
use rouge::persist::MemoryStorage;
use rouge::reactive::Runtime;
use rouge_widgets::testing::component_test;
use std::rc::Rc;

const KEY: &str = "hosted-appearance";

#[tokio::test]
async fn appearance_follows_the_system_until_overridden_and_saves_both_palettes() -> Result<(), TestError> {
    let _entered = Runtime::new().enter();
    let storage = MemoryStorage::default();
    let (mut ui, theme) = hosted(&storage, InputKind::Pointer);
    assert_eq!(theme_id(theme), "github-light");
    ui.set_appearance(Appearance::Dark);
    ui.run(0.0);
    assert_eq!(theme_id(theme), "github-dark");
    assert_eq!(storage.get(KEY), None);

    ui.click_on("open-settings")?;
    assert!(ui.bounds_of("settings").is_some());
    assert!(ui.bounds_of("reduce-motion").is_none());
    assert!(ui.bounds_of("interface-size").is_none());
    ui.click_on("appearance-light")?;
    assert_eq!(theme_id(theme), "github-light");
    let saved = storage.get(KEY);
    ui.set_appearance(Appearance::Light);
    ui.set_appearance(Appearance::Dark);
    ui.run(0.0);
    assert_eq!(theme_id(theme), "github-light");
    assert_eq!(storage.get(KEY), saved);

    ui.click_on("dark-theme")?;
    ui.type_text("nord");
    assert_eq!(theme_id(theme), "nord");
    assert_eq!(storage.get(KEY), saved, "a preview is not saved");
    assert!(ui.press("escape"));
    assert_eq!(theme_id(theme), "github-light");
    assert!(ui.bounds_of("settings").is_some());
    assert!(ui.bounds_of("dark-themes").is_none());

    ui.click_on("dark-theme")?;
    ui.type_text("nord");
    assert!(ui.press("enter"));
    assert_eq!(theme_id(theme), "github-light", "committing an inactive palette restores the active one");
    ui.click_on("appearance-dark")?;
    assert_eq!(theme_id(theme), "nord");
    ui.click_on("light-theme")?;
    ui.type_text("alabaster");
    assert!(ui.press("enter"));
    assert_eq!(theme_id(theme), "nord");
    assert!(ui.press("escape"));
    assert!(ui.bounds_of("settings").is_none());
    drop(ui);

    let (mut restored, theme) = hosted(&storage, InputKind::Pointer);
    assert!(restored.bounds_of("hosted").is_some());
    assert_eq!(theme_id(theme), "nord", "the first hosted frame uses the saved choice");
    restored.click_on("open-settings")?;
    restored.click_on("appearance-system")?;
    assert_eq!(theme_id(theme), "alabaster");
    restored.set_appearance(Appearance::Dark);
    restored.run(0.0);
    assert_eq!(theme_id(theme), "nord");
    Ok(())
}

#[tokio::test]
async fn the_touch_sheet_scrolls_and_leaves_the_conversation_draft_intact() -> Result<(), TestError> {
    let _entered = Runtime::new().enter();
    let storage = MemoryStorage::default();
    let (mut ui, theme) = hosted(&storage, InputKind::Touch);
    ui.tap_on("launch-demo")?;
    ui.run(1.0);
    ui.tap_on("composer-text")?;
    assert!(ui.has_focus(), "the composer takes focus on tap");
    ui.type_text("unsent draft");
    assert!(ui.text_bounds("unsent draft").is_some(), "draft is visible before entering Settings");
    ui.tap_on("toggle-sidebar")?;
    ui.tap_on("open-settings")?;
    ui.run(0.5);
    assert!(ui.rect_of("settings")?.contains(ui.center_of("settings-done")?));
    ui.tap_on("appearance-dark")?;
    assert_eq!(theme_id(theme), "github-dark");
    let saved = storage.get(KEY);
    ui.tap_on("light-theme")?;
    assert!(ui.bounds_of("picker-query").is_none(), "the touch picker doesn't summon a keyboard");
    ui.tap_on("settings-back")?;
    assert!(ui.bounds_of("light-themes").is_none());
    assert_eq!(storage.get(KEY), saved);

    ui.tap_on("dark-theme")?;
    let list = ui.rect_of("picker-results")?;
    let before = ui.bounds_of("github-dark");
    let start = Point::new(list.center().x, (list.origin.y + list.size.height) - 16.0);
    let end = Point::new(start.x, list.origin.y + 16.0);
    ui.swipe(start, end);
    ui.run(0.5);
    assert_ne!(ui.bounds_of("github-dark"), before, "the picker rows scroll");
    let option = ["nord", "one-dark", "solarized-dark", "tokyo-night", "vesper"]
        .into_iter()
        .find(|id| ui.bounds_of(id).is_some_and(|row| list.contains(row.center())))
        .ok_or_else(|| TestError::NoTestId("visible selectable dark theme".to_owned()))?;
    ui.tap_on(option)?;
    assert_eq!(theme_id(theme), option);
    assert!(ui.bounds_of("dark-themes").is_none());
    ui.tap_on("settings-done")?;
    ui.run(0.5);
    assert!(ui.bounds_of("settings").is_none());
    ui.tap_on("thread-demo-1")?;
    ui.run(0.5);
    assert!(ui.text_bounds("unsent draft").is_some());
    Ok(())
}

#[tokio::test]
async fn unreadable_preferences_and_invalid_palette_ids_fall_back_without_writing() {
    let _entered = Runtime::new().enter();
    for (saved, dark) in [
        ("invalid json", "github-dark"),
        (r#"{"pair":{"light":"missing-theme","dark":"nord"}}"#, "nord"),
        (r#"{"pair":{"light":"nord","dark":"github-light"}}"#, "github-dark"),
    ] {
        let storage = MemoryStorage::default();
        storage.insert(KEY, saved);
        let (mut ui, theme) = hosted(&storage, InputKind::Pointer);
        assert_eq!(theme_id(theme), "github-light");
        ui.set_appearance(Appearance::Dark);
        ui.run(0.0);
        assert_eq!(theme_id(theme), dark);
        assert_eq!(storage.get(KEY).as_deref(), Some(saved));
    }
}

fn hosted(storage: &MemoryStorage, input: InputKind) -> (TestUi, ActiveReviewTheme) {
    let storage = storage.clone();
    let (width, height) = if input == InputKind::Touch { (390.0, 650.0) } else { (1120.0, 840.0) };
    component_test().size(width, height).input(input).build_with(move || {
        let backend = MemoryBackend::new(vec![Project { id: "demo".to_owned(), name: "Demo project".to_owned() }]);
        let view = web_view(Rc::new(backend), Rc::new(NoPlatform), Rc::new(storage));
        (view, ActiveReviewTheme::current())
    })
}

fn theme_id(theme: ActiveReviewTheme) -> String {
    theme.review_signal().get_untracked().id().to_string()
}

struct NoPlatform;

impl ChatPlatform for NoPlatform {
    fn open(&self, _: &str) {}
}
