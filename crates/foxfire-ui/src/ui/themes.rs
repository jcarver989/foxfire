use super::ActiveReviewTheme;
use clankerdiff_ui_core::{Appearance, ReviewTheme, ThemeChoice, ThemePair, ThemePreference};
use rouge::core::appearance;
use rouge::persist::Storage;
use rouge::reactive::spawn;
use std::sync::Arc;

/// The themes a host lets its users choose from.
#[derive(Clone, Default)]
pub struct ThemeSet {
    choices: Option<Arc<[ThemeChoice]>>,
    defaults: ThemePair,
}

impl ThemeSet {
    /// Offers only `choices`, starting with the pair of them in `defaults`. The default set offers every built-in theme.
    pub fn new(choices: impl Into<Arc<[ThemeChoice]>>, defaults: ThemePair) -> Self {
        Self { choices: Some(choices.into()), defaults }
    }

    pub fn choices_for(&self, slot: Appearance) -> Arc<[ThemeChoice]> {
        match &self.choices {
            Some(choices) => choices.iter().filter(|choice| choice.theme.appearance() == slot).cloned().collect(),
            None => ThemeChoice::catalog_for(slot),
        }
    }

    pub fn preference(&self) -> ThemePreference {
        ThemePreference { pair: self.defaults.clone(), ..ThemePreference::default() }
    }

    pub fn name(&self, id: &str) -> String {
        let name = match &self.choices {
            Some(choices) => choices.iter().find(|choice| is(&choice.theme, id)).map(|choice| choice.name.clone()),
            None => ReviewTheme::catalog().into_iter().find(|theme| theme.id == id).map(|theme| theme.name),
        };
        name.unwrap_or_else(|| id.to_owned())
    }

    /// The theme a preference chooses for one appearance, or the default if it is not in the set.
    pub fn theme(&self, preference: &ThemePreference, slot: Appearance) -> ReviewTheme {
        let chosen = self.find(preference.pair.id(slot), slot);
        chosen.or_else(|| self.find(self.defaults.id(slot), slot)).unwrap_or_default()
    }

    /// Shows what a preference chooses while the system looks as it does now.
    pub fn apply(&self, preference: &ThemePreference, active: ActiveReviewTheme) {
        active.set(self.theme(preference, preference.appearance(appearance().get())));
    }

    /// Reads a saved preference, replacing what it chooses from outside the set.
    pub async fn load(&self, storage: &dyn Storage, key: &str) -> ThemePreference {
        let saved = storage.read(key).await;
        let Some(mut preference) = saved.and_then(|json| serde_json::from_str::<ThemePreference>(&json).ok()) else {
            return self.preference();
        };
        for slot in [Appearance::Light, Appearance::Dark] {
            if self.find(preference.pair.id(slot), slot).is_none() {
                preference.pair.set(slot, self.defaults.id(slot));
            }
        }
        preference
    }

    pub fn save(storage: &dyn Storage, key: &str, preference: &ThemePreference) {
        if let Ok(json) = serde_json::to_string(preference) {
            drop(spawn(storage.write(key, json)));
        }
    }

    fn find(&self, id: &str, slot: Appearance) -> Option<ReviewTheme> {
        let theme = match &self.choices {
            Some(choices) => choices.iter().find(|choice| is(&choice.theme, id)).map(|choice| choice.theme.clone()),
            None => ReviewTheme::builtin(id).ok(),
        };
        theme.filter(|theme| theme.appearance() == slot)
    }
}

fn is(theme: &ReviewTheme, id: &str) -> bool {
    theme.id().to_string() == id
}
