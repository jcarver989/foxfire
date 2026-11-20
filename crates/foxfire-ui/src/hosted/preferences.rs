use crate::ui::ThemePreference;
use clankerdiff_ui_core::{Appearance, ReviewTheme};
use rouge::persist::Storage;
use rouge::reactive::spawn;

const KEY: &str = "hosted-appearance";

pub(super) async fn load(storage: &dyn Storage) -> ThemePreference {
    let Some(mut preference) =
        storage.read(KEY).await.and_then(|json| serde_json::from_str::<ThemePreference>(&json).ok())
    else {
        return ThemePreference::default();
    };
    let defaults = ThemePreference::default();
    for slot in [Appearance::Light, Appearance::Dark] {
        if !ReviewTheme::builtin(preference.pair.id(slot)).is_ok_and(|theme| theme.appearance() == slot) {
            preference.pair.set(slot, defaults.pair.id(slot));
        }
    }
    preference
}

pub(super) fn save(storage: &dyn Storage, preference: &ThemePreference) {
    if let Ok(json) = serde_json::to_string(preference) {
        drop(spawn(storage.write(KEY, json)));
    }
}
