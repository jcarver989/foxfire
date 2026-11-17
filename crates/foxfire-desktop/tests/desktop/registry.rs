use aether_desktop::registry::{Registry, STATE_FILE, State, saved_in};
use aether_desktop::sessions::SessionKey;
use agent_client_protocol::v2::schema::SessionId;
use rouge::core::div;
use rouge::persist::FileStorage;
use rouge::reactive::Runtime as Reactive;
use rouge_widgets::testing::component_test;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[test]
fn undoing_every_change_to_a_thread_forgets_it() {
    let dir = tempfile::tempdir().unwrap();
    let session = SessionKey::new("agent", &SessionId::new("session"));

    let title = with_registry(dir.path(), |registry| {
        registry.update(|state| {
            state.archived.insert(session.clone());
            state.rename(session.clone(), "Custom title");
            state.add_project("/alpha".into());
        });
        registry.update(|state| {
            state.archived.remove(&session);
            state.rename(session.clone(), "   ");
        });
        registry.with(|state| state.title(Some(&session), "derived".into()))
    });

    assert_eq!(title, "derived");
    assert!(!fs::read_to_string(dir.path().join(STATE_FILE)).unwrap().contains("session"));
}

#[test]
fn a_corrupt_state_file_loads_as_nothing_remembered_and_is_replaced_by_the_next_save() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(STATE_FILE), "{ not json").unwrap();

    let loaded = with_registry(dir.path(), |registry| {
        let loaded = registry.get();
        registry.update(|state| state.add_project("/alpha".into()));
        loaded
    });

    assert_eq!(loaded, State::default());
    assert_eq!(with_registry(dir.path(), |registry| registry.get().projects), [PathBuf::from("/alpha")]);
}

#[test]
fn a_missing_state_file_loads_as_nothing_remembered() {
    let dir = tempfile::tempdir().unwrap();

    let loaded = with_registry(&dir.path().join("not-created-yet"), |registry| registry.get());

    assert_eq!(loaded, State::default());
}

/// Runs `test` against a registry saved in `dir`, then closes the window so everything is written.
fn with_registry<T>(dir: &Path, test: impl FnOnce(Registry) -> T) -> T {
    let _entered = Reactive::new().enter();
    let (ui, registry) =
        component_test().build_with(|| (div(), Registry::restore(saved_in(dir), Rc::new(FileStorage::new(dir)))));
    let result = test(registry);
    drop(ui);
    result
}
