use crate::registry::Registry;
use crate::theme;
use crate::ui::tokens::{Space, UNIT};
use crate::ui::{
    ButtonVariant, InterfaceSize, Material, ModalSize, ThemePickerEvent, UiStyles, button, modal, number_field,
    segment, segmented_control, theme_picker, theme_picker_view, toggle,
};
use clankerdiff_ui_core::{Appearance, AppearanceMode, ReviewTheme, ThemeChoice};
use rouge::core::{AnyView, Div, Styled, View, div, dynamic, text, view};
use rouge::reactive::{Callback, Signal, effect, memo, signal, untrack};
use rouge::widgets::{Placement, popover};

pub(crate) fn settings(registry: Registry, on_close: impl Fn() + 'static) -> impl View {
    let on_close = Callback::from(on_close);
    let close = move || {
        show_saved_theme(registry);
        on_close.call();
    };
    view! {
        <modal test_id="settings" title="Settings" hint="Esc to close" modal_size=ModalSize::Compact on_dismiss=close>
            {row("Reduce motion", "Stills the activity marks and every panel", motion(registry))}
            {row("Interface size", "Scales everything in the window", interface_size())}
            {row("Appearance", "Follow the system, or always look light or dark", mode(registry))}
            {row("Light theme", "Shown when the app looks light", theme_choice(registry, Appearance::Light))}
            {row("Dark theme", "Shown when the app looks dark", theme_choice(registry, Appearance::Dark))}
        </modal>
    }
}

fn motion(registry: Registry) -> AnyView {
    let reduced = signal(registry.with_untracked(|state| state.reduced_motion));
    effect(move || {
        reduced.set_if_changed(registry.with(|state| state.reduced_motion));
    });
    effect(move || {
        let chosen = reduced.get();
        if registry.with_untracked(|state| state.reduced_motion) != chosen {
            registry.update(|state| state.reduced_motion = chosen);
        }
    });
    view! { <toggle test_id="reduce-motion" checked=reduced/> }.into_any()
}

fn interface_size() -> AnyView {
    let size = InterfaceSize::current();
    let typed = signal(f64::from(untrack(|| size.get())));
    effect(move || {
        typed.set_if_changed(f64::from(size.get()));
    });
    effect(move || {
        #[allow(clippy::cast_possible_truncation, reason = "An interface size is a few dozen px.")]
        let chosen = typed.get() as f32;
        untrack(|| {
            if (size.get() - chosen).abs() >= 0.5 {
                size.set(chosen);
                typed.set_if_changed(f64::from(size.get()));
            }
        });
    });
    view! { <number_field test_id="interface-size" value=typed suffix="px" w=INTERFACE_SIZE_WIDTH/> }.into_any()
}

fn mode(registry: Registry) -> AnyView {
    let current = memo(move || registry.with(|state| state.theme.mode));
    let choices = [
        (AppearanceMode::System, "System"),
        (AppearanceMode::Light, "Light"),
        (AppearanceMode::Dark, "Dark"),
    ]
    .map(|(mode, label)| {
        view! {
            <segment test_id={format!("appearance-{}", label.to_lowercase())} label selected={move || current.get() == mode}
                     on_click={move || {
                         registry.update(|state| state.theme.mode = mode);
                         show_saved_theme(registry);
                     }}/>
        }
    });
    view! { <segmented_control flex_none>{..choices}</segmented_control> }.into_any()
}

fn theme_choice(registry: Registry, appearance: Appearance) -> AnyView {
    let key = key(appearance);
    let open = signal(false);
    effect(move || {
        if !open.get() {
            untrack(|| show_saved_theme(registry));
        }
    });
    let name = memo(move || registry.with(|state| theme_name(state.theme.pair.id(appearance))));
    let trigger = dynamic(move || {
        let label = name.get();
        view! {
            <button test_id={format!("{key}-theme")} label variant=ButtonVariant::Secondary
                    on_click={move || open.set(true)}/>
        }
    });
    popover(open, trigger, move || picker(registry, appearance, open)).placement(Placement::Below).into_any()
}

fn picker(registry: Registry, appearance: Appearance, open: Signal<bool>) -> impl View {
    let id = registry.with_untracked(|state| state.theme.pair.id(appearance).to_owned());
    let current = ReviewTheme::builtin(&id).unwrap_or_default();
    let handle = theme_picker(&current, ThemeChoice::catalog_for(appearance), move |event| match event {
        ThemePickerEvent::Preview(theme) => theme::preview(theme),
        ThemePickerEvent::Commit(theme) => {
            let id = theme.id().to_string();
            registry.update(|state| state.theme.pair.set(appearance, id));
            open.set(false);
        }
        ThemePickerEvent::Cancel(_) => open.set(false),
    });
    view! { <theme_picker_view id={format!("{}-themes", key(appearance))} handle material={Material::Bare}/> }
}

fn show_saved_theme(registry: Registry) {
    let preference = registry.with_untracked(|state| state.theme.clone());
    theme::apply(&preference);
}

fn key(appearance: Appearance) -> &'static str {
    match appearance {
        Appearance::Light => "light",
        Appearance::Dark => "dark",
    }
}

fn theme_name(id: &str) -> String {
    ReviewTheme::catalog().into_iter().find(|theme| theme.id == id).map_or_else(|| id.to_owned(), |theme| theme.name)
}

fn row(label: &'static str, detail: &'static str, control: AnyView) -> Div {
    view! {
        <div py=Space::X2 flex items_center justify_between gap=Space::X4>
            <div min_w=0.0 flex col>
                <text body primary>{label}</text>
                <text small muted>{detail}</text>
            </div>
            {control}
        </div>
    }
}

const INTERFACE_SIZE_WIDTH: f32 = 30.0 * UNIT;
