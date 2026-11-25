use clankerdiff_ui_core::{Appearance, AppearanceMode, ReviewTheme};
use foxfire_ui::ChatPane;
use foxfire_ui::backend::{BackendError, HostedBackend, HostedState, Project, ThreadId};
use foxfire_ui::memory::MemoryBackend;
use foxfire_ui::platform::ChatPlatform;
use foxfire_ui::thread_list::ThreadList;
use foxfire_ui::ui::{
    ActiveReviewTheme, ActivityMark, ActivityMotion, ThemeChoice, ThemePair, ThemePreference, ThemeSet, Wording,
};
use rouge::core::testing::{TestError as UiError, TestUi};
use rouge::core::{Styled, View, div, dynamic, view};
use rouge::persist::{MemoryStorage, Storage};
use rouge::reactive::{Entered, Runtime, untrack};
use rouge_widgets::testing::component_test;
use std::rc::Rc;
use std::time::Duration;

#[tokio::test]
async fn a_host_shows_its_own_words_and_mark_in_every_state() -> Result<(), TestError> {
    let mut host = HostBuilder::default().build().await?;
    host.assert_unbranded();

    host.ui.click_on("composer-text")?;
    host.ui.type_text("/permission");
    assert!(host.ui.press("enter"));
    host.ui.run(0.2);
    assert!(host.ui.bounds_of("lemon").is_some(), "the host's mark shows the activity");
    host.assert_unbranded();

    host.backend.set_state(&host.thread, HostedState::Ended)?;
    host.ui.run(0.2);
    assert!(host.says("Lemon again"), "{:?}", host.text());
    assert!(host.says("Lemon returning") || host.says("Lemon stuck"), "{:?}", host.text());
    host.assert_unbranded();
    Ok(())
}

#[tokio::test]
async fn a_host_without_a_mark_still_shows_activity() -> Result<(), TestError> {
    let mut host = HostBuilder::default().without_mark().build().await?;
    host.ui.click_on("composer-text")?;
    host.ui.type_text("/permission");
    assert!(host.ui.press("enter"));
    host.ui.run(0.2);
    assert!(host.ui.bounds_of("activity").is_some());
    assert!(host.ui.bounds_of("lemon").is_none());
    Ok(())
}

#[tokio::test]
async fn saved_choices_from_outside_a_hosts_themes_fall_back_to_its_own() -> Result<(), TestError> {
    let _entered = Runtime::new().enter();
    let (storage, themes) = (MemoryStorage::default(), themes());
    assert_eq!(themes.load(&storage, KEY).await, themes.preference());

    let builtin = ThemePair { light: "github-light".to_owned(), dark: "lemon-dark".to_owned() };
    let saved = ThemePreference { mode: AppearanceMode::Dark, pair: builtin };
    storage.write(KEY, serde_json::to_string(&saved)?).await;
    let loaded = themes.load(&storage, KEY).await;
    assert_eq!((loaded.mode, loaded.pair.light.as_str()), (AppearanceMode::Dark, "lemon-light"));
    assert_eq!(themes.theme(&loaded, Appearance::Light).id().to_string(), "lemon-light");

    let offered: Vec<String> = themes.choices_for(Appearance::Dark).iter().map(|choice| choice.name.clone()).collect();
    assert_eq!((offered, themes.name("lemon-dark")), (vec!["Lemon Dark".to_owned()], "Lemon Dark".to_owned()));
    Ok(())
}

#[derive(Debug, thiserror::Error)]
enum TestError {
    #[error(transparent)]
    Ui(#[from] UiError),
    #[error(transparent)]
    Backend(#[from] BackendError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

const KEY: &str = "lemon-appearance";
const WORDS: Wording = Wording {
    untitled: "Lemon new",
    unprompted: "Lemon ready",
    connecting: "Lemon waking",
    connecting_detail: "Lemon wakes",
    stopped: "Lemon resting",
    stopped_detail: "Lemon rests",
    cannot_start: "Lemon stuck",
    reconnecting: "Lemon returning",
    disconnected: "Lemon went quiet",
    disconnected_detail: "Lemon will be back",
    reconnect: "Lemon again",
    placeholder: "Ask Lemon",
    files_waiting: "Lemon reads",
    files_stopped: "Lemon rests",
    files_stopped_detail: "Lemon put the files away",
    room: "Lemon lounge",
    room_detail: "Lemon lounges",
};

struct HostBuilder {
    mark: bool,
}

struct Host {
    ui: TestUi,
    backend: MemoryBackend,
    thread: ThreadId,
    _entered: Entered,
}

struct NoPlatform;

impl ChatPlatform for NoPlatform {
    fn open(&self, _: &str) {}
}

impl Default for HostBuilder {
    fn default() -> Self {
        Self { mark: true }
    }
}

impl HostBuilder {
    fn without_mark(mut self) -> Self {
        self.mark = false;
        self
    }

    async fn build(self) -> Result<Host, TestError> {
        let entered = Runtime::new().enter();
        let backend = MemoryBackend::new(vec![Project { id: "demo".to_owned(), name: "Demo".to_owned() }])
            .with_reply(vec!["Done.".to_owned()], Duration::ZERO);
        let thread = backend.launch("demo".to_owned()).await?.id;
        let (listed, shown) = (backend.clone(), thread.clone());
        let mut ui = component_test().size(900.0, 700.0).build(move || {
            WORDS.provide();
            if self.mark {
                ActivityMark::provide(|_| div().test_id("lemon").size_full());
            }
            let _ = ActivityMotion::install(false);
            let themes = themes();
            let _ = ActiveReviewTheme::install(themes.theme(&themes.preference(), Appearance::Light));
            let list =
                ThreadList::new(Rc::new(listed), |thread| ChatPane::new(thread, Rc::new(NoPlatform), |_| {}, || {}));
            dynamic(move || {
                list.can_open(&shown).then(|| untrack(|| list.open(&shown).map(|(_, pane)| lay_out(&pane))))
            })
        });
        ui.run(0.5);
        Ok(Host { ui, backend, thread, _entered: entered })
    }
}

impl Host {
    fn text(&mut self) -> Vec<String> {
        self.ui.access_nodes().into_iter().map(|node| node.name).filter(|name| !name.is_empty()).collect()
    }

    fn says(&mut self, words: &str) -> bool {
        self.ui.text_bounds(words).is_some() || self.text().iter().any(|text| text.contains(words))
    }

    fn assert_unbranded(&mut self) {
        let default = Wording::default();
        let foxfires = [
            "Foxfire",
            "What do you want to build?",
            default.connecting_detail,
            default.disconnected,
            default.disconnected_detail,
            default.reconnect,
        ];
        for words in foxfires {
            assert!(!self.says(words), "{words:?} in {:?}", self.text());
        }
        assert!(self.ui.bounds_of("kindling").is_none());
    }
}

fn lay_out(pane: &ChatPane) -> impl View + use<> {
    view! {
        <div size_full flex col track_bounds={pane.stage()}>
            <div relative flex_1 min_h=0.0>{pane.transcript()}</div>
            <div flex_none flex col track_bounds={pane.dock()}>
                {pane.reconnect_banner()}
                {pane.elicitation()}
                <div track_bounds={pane.composer_bounds()}>{pane.composer().view()}</div>
                <div flex items_center>{pane.options()}{pane.status()}</div>
            </div>
        </div>
    }
}

fn themes() -> ThemeSet {
    let theme = |id: &str, base: &str| {
        let base = ReviewTheme::builtin(base).unwrap_or_default();
        ReviewTheme::builder(id).diff(base.diff).ui(base.ui).markdown(base.markdown).build()
    };
    let choices = vec![
        ThemeChoice::new("Lemon Light", theme("lemon-light", "github-light")),
        ThemeChoice::new("Lemon Dark", theme("lemon-dark", "github-dark")),
    ];
    ThemeSet::new(choices, ThemePair { light: "lemon-light".to_owned(), dark: "lemon-dark".to_owned() })
}
