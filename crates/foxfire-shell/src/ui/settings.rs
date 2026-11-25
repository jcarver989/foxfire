use clankerdiff_ui_core::{Appearance, AppearanceMode, ThemePreference};
use foxfire_ui::ui::tokens::{Space, UNIT};
use foxfire_ui::ui::{
    ActiveReviewTheme, ButtonVariant, IconButton, IconName, InterfaceSize, Material, ModalSize, ThemePickerEvent,
    ThemeSet, UiStyles, button, icon_button, key_hints, modal, number_field, search_picker, segment, segmented_control,
    text_input, theme_picker, toggle,
};
use rouge::core::{AnyView, Div, Styled, View, div, dynamic, pct, presentation, scroll, text, view};
use rouge::reactive::{Callback, Memo, Signal, Stored, effect, memo, signal, stored, untrack};
use rouge::widgets::{Placement, list_group, popover};

/// A setting read from and committed to the host's state.
pub struct SettingsField<T: 'static> {
    value: Memo<T>,
    set: Callback<(T,)>,
}

impl<T> Copy for SettingsField<T> {}

impl<T> Clone for SettingsField<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> SettingsField<T> {
    pub fn new(value: Memo<T>, set: impl Fn(T) + 'static) -> Self {
        Self { value, set: Callback::from(set) }
    }
}

/// The user's choice among a host's themes.
#[derive(Clone, Copy)]
pub struct AppearanceField {
    themes: Stored<ThemeSet>,
    field: SettingsField<ThemePreference>,
}

impl AppearanceField {
    pub fn new(themes: ThemeSet, field: SettingsField<ThemePreference>) -> Self {
        Self { themes: stored(themes), field }
    }

    fn commit(self, active: ActiveReviewTheme, change: impl FnOnce(&mut ThemePreference)) {
        let current = self.field.value.get_untracked();
        let mut next = current.clone();
        change(&mut next);
        self.themes.with(|themes| themes.apply(&next, active));
        if next != current {
            self.field.set.call(next);
        }
    }

    fn restore(self, active: ActiveReviewTheme) {
        self.field.value.with_untracked(|value| self.themes.with(|themes| themes.apply(value, active)));
    }
}

/// The host's settings bindings. Absent capabilities have no row in Settings.
#[derive(Clone, Copy)]
pub struct SettingsModel {
    pub appearance: AppearanceField,
    pub reduced_motion: Option<SettingsField<bool>>,
    pub interface_size: Option<InterfaceSize>,
    pub display_name: Option<SettingsField<String>>,
}

/// The shared sidebar entry point for Settings.
pub fn settings_button(on_open: impl Fn() + 'static) -> IconButton {
    icon_button(IconName::Settings, "Settings".to_owned()).test_id("open-settings").on_click(on_open)
}

/// The shared Settings dialog or touch sheet.
pub fn settings(model: SettingsModel, on_close: impl Fn() + 'static) -> impl View {
    let appearance = model.appearance;
    let active = ActiveReviewTheme::current();
    let touch = presentation().get_untracked().is_touch();
    let on_close = Callback::from(on_close);
    let close = move || {
        appearance.restore(active);
        on_close.call();
    };
    let body = if touch {
        let selected = signal(None);
        dynamic(move || {
            if let Some(slot) = selected.get() {
                let back = move || {
                    appearance.restore(active);
                    selected.set(None);
                };
                view! {
                    <div flex col flex_1 min_h=0.0>
                        <button test_id="settings-back" label="Back" variant=ButtonVariant::Secondary
                                self_start flex_none on_click=back/>
                        {picker(appearance, active, slot, true, back)}
                    </div>
                }
                .into_any()
            } else {
                scroll().flex_1().min_h(0.0).child(overview(model, active, Some(selected))).into_any()
            }
        })
        .into_any()
    } else {
        overview(model, active, None).into_any()
    };
    let panel = modal("Settings".to_owned())
        .test_id("settings")
        .modal_size(ModalSize::Compact)
        .on_dismiss(close)
        .child(body)
        .child(view! {
            <button test_id="settings-done" label="Done" variant=ButtonVariant::Secondary
                    flex_none self_end on_click=close/>
        });
    if touch { panel.height(pct(90.0)) } else { panel.hint("Esc to close") }
}

fn overview(model: SettingsModel, active: ActiveReviewTheme, touch_picker: Option<Signal<Option<Appearance>>>) -> Div {
    let theme = model.appearance;
    let touch = touch_picker.is_some();
    let general = [
        model.display_name.map(|field| {
            row("Display name", "Shown to others in shared threads; applies on reconnect", display_name(field), touch)
        }),
        model
            .reduced_motion
            .map(|field| row("Reduce motion", "Stills the activity marks and every panel", motion(field), touch)),
        model
            .interface_size
            .map(|size| row("Interface size", "Scales everything in the window", interface_size(size), touch)),
    ];
    let appearance = [
        row("Appearance", "Follow the system, or always look light or dark", mode(theme, active), touch),
        row(
            "Light theme",
            "Shown when the app looks light",
            theme_choice(theme, active, Appearance::Light, touch_picker),
            touch,
        ),
        row(
            "Dark theme",
            "Shown when the app looks dark",
            theme_choice(theme, active, Appearance::Dark, touch_picker),
            touch,
        ),
    ];
    let content = div().flex().col().children(general.into_iter().flatten());
    if touch {
        content.child(list_group().children(appearance.map(|row| row.px(Space::X3))))
    } else {
        content.children(appearance)
    }
}

fn display_name(field: SettingsField<String>) -> AnyView {
    let typed = signal(field.value.get_untracked());
    effect(move || {
        typed.set_if_changed(field.value.get());
    });
    effect(move || {
        let next = typed.get();
        if field.value.get_untracked() != next {
            field.set.call(next);
        }
    });
    view! { <text_input test_id="display-name" value=typed placeholder="Your name" w=DISPLAY_NAME_WIDTH/> }.into_any()
}

fn motion(field: SettingsField<bool>) -> AnyView {
    let reduced = signal(field.value.get_untracked());
    effect(move || {
        reduced.set_if_changed(field.value.get());
    });
    effect(move || {
        let chosen = reduced.get();
        if field.value.get_untracked() != chosen {
            field.set.call(chosen);
        }
    });
    view! { <toggle test_id="reduce-motion" checked=reduced/> }.into_any()
}

fn interface_size(size: InterfaceSize) -> AnyView {
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

fn mode(field: AppearanceField, active: ActiveReviewTheme) -> AnyView {
    let current = memo(move || field.field.value.with(|value| value.mode));
    let choices = [(AppearanceMode::System, "System"), (AppearanceMode::Light, "Light"), (AppearanceMode::Dark, "Dark")]
        .map(|(mode, label)| {
            view! {
                <segment test_id={format!("appearance-{}", label.to_lowercase())} label selected={move || current.get() == mode}
                         on_click={move || field.commit(active, |value| value.mode = mode)}/>
            }
        });
    view! { <segmented_control flex_none>{..choices}</segmented_control> }.into_any()
}

fn theme_choice(
    field: AppearanceField,
    active: ActiveReviewTheme,
    slot: Appearance,
    touch_picker: Option<Signal<Option<Appearance>>>,
) -> AnyView {
    let open = signal(false);
    let name =
        memo(move || field.field.value.with(|value| field.themes.with(|themes| themes.name(value.pair.id(slot)))));
    let trigger = view! {
        <button test_id={format!("{}-theme", key(slot))} label={move || name.get()} variant=ButtonVariant::Secondary
                on_click={move || if let Some(touch_picker) = touch_picker {
                    touch_picker.set(Some(slot));
                } else {
                    open.set(true);
                }}/>
    };
    if touch_picker.is_some() {
        return trigger.into_any();
    }
    effect(move || {
        if !open.get() {
            untrack(|| field.restore(active));
        }
    });
    popover(open, trigger, move || picker(field, active, slot, false, move || open.set(false)))
        .placement(Placement::Below)
        .into_any()
}

fn picker(
    field: AppearanceField,
    active: ActiveReviewTheme,
    slot: Appearance,
    touch: bool,
    close: impl Fn() + 'static,
) -> impl View {
    let (current, choices) = field.themes.with(|themes| {
        let current = field.field.value.with_untracked(|value| themes.theme(value, slot));
        (current, themes.choices_for(slot))
    });
    let handle = theme_picker(&current, choices, move |event| match event {
        ThemePickerEvent::Preview(theme) => {
            if !touch {
                active.set(theme);
            }
        }
        ThemePickerEvent::Commit(theme) => {
            field.commit(active, |value| value.pair.set(slot, theme.id().to_string()));
            close();
        }
        ThemePickerEvent::Cancel(_) => {
            field.restore(active);
            close();
        }
    });
    let picker = search_picker(handle)
        .test_id(format!("{}-themes", key(slot)))
        .material(Material::Bare)
        .empty("No matching themes");
    if touch {
        picker.w_full().flex_1().min_h(0.0).max_list_height(pct(100.0))
    } else {
        picker.searchable("Search themes…").footer(key_hints([("↑↓", "Preview"), ("↵", "Apply"), ("Esc", "Cancel")]))
    }
}

fn key(appearance: Appearance) -> &'static str {
    match appearance {
        Appearance::Light => "light",
        Appearance::Dark => "dark",
    }
}

fn row(label: &'static str, detail: &'static str, control: AnyView, stacked: bool) -> Div {
    view! {
        <div py=Space::X2 flex>
            <div min_w=0.0 flex col>
                <text body primary>{label}</text>
                <text small muted>{detail}</text>
            </div>
            {control}
        </div>
    }
    .when(stacked, |row| row.col().items_start().gap(Space::X2))
    .when(!stacked, |row| row.items_center().justify_between().gap(Space::X4))
}

const INTERFACE_SIZE_WIDTH: f32 = 30.0 * UNIT;
const DISPLAY_NAME_WIDTH: f32 = 48.0 * UNIT;
