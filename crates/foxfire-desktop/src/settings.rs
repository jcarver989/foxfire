use crate::registry::Registry;
use crate::ui::{InterfaceSize, SettingsField, SettingsModel};
use foxfire_ui::ui::settings as settings_view;
use rouge::core::View;
use rouge::reactive::memo;

pub(crate) fn settings(registry: Registry, on_close: impl Fn() + 'static) -> impl View {
    let model = SettingsModel {
        appearance: SettingsField::new(memo(move || registry.with(|state| state.theme.clone())), move |theme| {
            registry.update(|state| state.theme = theme);
        }),
        reduced_motion: Some(SettingsField::new(
            memo(move || registry.with(|state| state.reduced_motion)),
            move |reduced| registry.update(|state| state.reduced_motion = reduced),
        )),
        interface_size: Some(InterfaceSize::current()),
    };
    settings_view(model, on_close)
}
