use crate::threads::Thread;
use crate::ui::tokens::{COMPOSER_POPUP_HEIGHT, ICON_SIZE_SMALL, Space};
use crate::ui::{
    Icon, IconName, Material, PickerItem, SearchPickerHandle, UiStyles, list_row, picker_closed, search_picker,
    selection, with_middots,
};
use agent_client_protocol::v2::schema as acp;
use foxfire_core::session_config_view::LocalConfigOption;
use rouge::core::{AnyView, Role, Styled, View, div, dynamic, px, text, view};
use rouge::reactive::{memo, signal};
use rouge::widgets::{Placement, popover};

#[derive(Clone, Copy)]
pub(crate) struct SessionOptions {
    thread: Thread,
}

impl SessionOptions {
    pub(crate) fn new(thread: Thread) -> Self {
        Self { thread }
    }

    pub(crate) fn view(self) -> impl View {
        let thread = self.thread;
        let options = memo(move || {
            thread.with_connected_core(|core| core.session().config_options().to_vec()).unwrap_or_default()
        });
        view! {
            <div test_id="session-options" min_w=0.0 overflow_hidden flex items_center gap=Space::X1 body>
                <dynamic>{with_middots(options.get().iter().filter_map(|option| self.selector(option)))}</dynamic>
            </div>
        }
    }

    fn selector(self, option: &LocalConfigOption) -> Option<AnyView> {
        let label = option.display_value()?.to_owned();
        let id = format!("config-{}", option.id);
        let open = signal(false);
        let icon = category_icon(option).map(|icon| {
            view! { <div test_id={format!("{id}-icon")} flex>{Icon::new(icon, Role::TextMuted).size(ICON_SIZE_SMALL)}</div> }
        });
        let trigger = view! {
            <list_row test_id={id.clone()} selection={move || selection(open.get())} on_click={move || open.set(true)}>
                {icon}
                <div test_id={format!("{id}-value")}><text>{label}</text></div>
            </list_row>
        };
        let (thread, option) = (self.thread, option.clone());
        let content = move || {
            let Some(select) = option.select() else {
                return ().into_any();
            };
            let values = select.values.clone();
            let current = values.iter().position(|value| value.value == select.current_value).unwrap_or(0);
            let picker = format!("config-{}-picker", option.id);
            let items = values
                .iter()
                .map(|value| {
                    let item = PickerItem::new(format!("{picker}-{}", value.value), value.name.clone())
                        .keywords(value.value.clone())
                        .disabled(value.is_disabled)
                        .checked(value.value == select.current_value);
                    if value.is_disabled { item.detail("unavailable") } else { item }
                })
                .collect();
            let option_id = option.id.clone();
            let handle = SearchPickerHandle::new(items, move |event| {
                let closed = picker_closed(&event, |index| {
                    if let Some(value) = values.get(index) {
                        thread.update_core(|core| core.set_config_option(&option_id, &value.value));
                    }
                });
                if closed {
                    open.set(false);
                }
            })
            .highlighted(current);
            let searchable =
                (select.values.len() > SEARCH_THRESHOLD).then(|| format!("Search {}…", option.name.to_lowercase()));
            view! {
                <search_picker test_id=picker handle material={Material::Bare} searchable?=searchable
                               title={option.name.clone()} w={px(PICKER_WIDTH)}
                               max_list_height={px(COMPOSER_POPUP_HEIGHT)}/>
            }
            .into_any()
        };
        Some(popover(open, trigger, content).placement(Placement::Above).into_any())
    }
}

const SEARCH_THRESHOLD: usize = 8;
const PICKER_WIDTH: f32 = 288.0;

fn category_icon(option: &LocalConfigOption) -> Option<IconName> {
    match option.category.as_ref()? {
        acp::SessionConfigOptionCategory::Mode => Some(IconName::Bot),
        acp::SessionConfigOptionCategory::Model => Some(IconName::Cpu),
        acp::SessionConfigOptionCategory::ThoughtLevel => Some(IconName::Brain),
        _ => None,
    }
}
