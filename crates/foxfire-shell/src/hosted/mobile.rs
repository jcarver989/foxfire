#![allow(clippy::large_types_passed_by_value, reason = "HostedApp carries Copy handles for the view callbacks.")]

use super::{HostedApp, Shown, waiting};
use crate::ui::thread_status_icon;
use crate::workspace::{Header, Workspace};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use foxfire_ui::backend::{Project, ThreadId};
use foxfire_ui::thread_list::ThreadList;
use foxfire_ui::ui::tokens::Space;
use foxfire_ui::ui::{IconName, UiStyles, button, empty_state, icon_button, list_row, row_action};
use rouge::core::{
    AnyView, Div, Role, ScrollState, Scrollable, Styled, View, WindowHandle, div, dynamic, each, keyboard_inset, route,
    scroll, show, text, view, window_handle,
};
use rouge::reactive::{ReadSignal, memo, signal, stored, untrack};
use rouge::widgets::{alert, list_group, nav_bar, nav_stack};

pub(super) fn view(app: HostedApp) -> impl View {
    let nav = MobileNav { window: window_handle(), route: route() };
    let ending = signal(None::<ThreadId>);
    let confirming = signal(false);
    let confirm_end = move |id| {
        ending.set(Some(id));
        confirming.set(true);
    };
    let stack =
        nav_stack().size_full().page("/", move |_| threads(app, nav, confirm_end)).page("/thread/:id", move |params| {
            let id = params.get("id").and_then(decode_id);
            conversation(app, id, confirm_end)
        });
    let confirmation = app.list.catalog().map(|list| {
        alert(confirming, "End thread?".to_owned())
            .test_id("end-thread-confirmation")
            .message("This ends the thread and stops any response in progress.")
            .cancel("Keep thread")
            .action(
                row_action("End thread", move || {
                    if let Some(id) = ending.get_untracked() {
                        list.stop(id, move |id| nav.leave(id));
                    }
                })
                .destructive(),
            )
    });
    view! { <div relative size_full>{stack}{confirmation}</div> }
}

#[derive(Clone, Copy)]
struct MobileNav {
    window: WindowHandle,
    route: ReadSignal<String>,
}

impl MobileNav {
    fn open(self, id: &ThreadId) {
        self.window.navigate(format!("/thread/{}", URL_SAFE_NO_PAD.encode(&id.0)));
    }

    fn leave(self, id: &ThreadId) {
        if self.route.with_untracked(|path| thread_segment(path).and_then(decode_id).as_ref() == Some(id)) {
            self.window.navigate("/");
        }
    }
}

fn thread_segment(route: &str) -> Option<&str> {
    let mut segments = route.split('/').filter(|segment| !segment.is_empty());
    (segments.next()? == "thread").then(|| segments.next()).flatten()
}

fn decode_id(encoded: &str) -> Option<ThreadId> {
    String::from_utf8(URL_SAFE_NO_PAD.decode(encoded).ok()?).ok().map(ThreadId)
}

fn threads(app: HostedApp, nav: MobileNav, confirm_end: impl Fn(ThreadId) + Copy + 'static) -> impl View {
    let state = signal(ScrollState::default());
    let header = nav_bar("Threads".to_owned()).test_id("threads-nav").large().scroll(state).trailing(app.controls());
    let projects = view! {
        <div w_full max_w=INBOX_MAX_WIDTH flex col gap=Space::X4 p=Space::X4>
            <each items={move || app.list.projects()} key={|project: &Project| project.id.clone()} let:project>
                {project_group(app, nav, project, confirm_end)}
            </each>
            <show when={move || app.list.catalog().is_none_or(ThreadList::is_loaded) && app.list.projects().is_empty()}>
                <empty_state icon=IconName::Cloud title="No projects" detail="Refresh to check for available projects."/>
            </show>
        </div>
    };
    view! {
        <div test_id="threads-screen" size_full flex col>
            {header}
            <scroll test_id="threads-list" flex_1 min_h=0.0 w_full scroll_state=state items_center>{projects}</scroll>
        </div>
    }
}

fn project_group(
    app: HostedApp,
    nav: MobileNav,
    project: ReadSignal<Project>,
    confirm_end: impl Fn(ThreadId) + Copy + 'static,
) -> impl View {
    let id = move || project.with_untracked(|project| project.id.clone());
    let ids = memo(move || app.list.project_threads(&id()));
    let title = memo(move || project.with(|project| project.name.clone()));
    dynamic(move || {
        let add = app.list.catalog().map(|list| {
            let button = button("New thread".to_owned())
                .test_id(format!("launch-{}", id()))
                .disabled(move || list.is_launching(&id()))
                .on_click(move || list.launch(id(), move |id| nav.open(id)));
            view! { <div px=Space::X3 py=Space::X2 flex items_center>{button}</div> }
        });
        list_group()
            .test_id(format!("project-{}", id()))
            .title(title.get())
            .child(add)
            .children(ids.get().into_iter().map(|id| thread_row(app, nav, id, confirm_end)))
    })
}

fn thread_row(
    app: HostedApp,
    nav: MobileNav,
    id: ThreadId,
    confirm_end: impl Fn(ThreadId) + Copy + 'static,
) -> impl View {
    let id = stored(id);
    let row_id = format!("thread-{}", id.get().0);
    let title = move || app.title(&id.get());
    let status = memo(move || app.list.status(&id.get()));
    let icon = dynamic({
        let row_id = row_id.clone();
        move || thread_status_icon(&row_id, status.get())
    });
    let row = list_row()
        .test_id(row_id.clone())
        .leading(icon)
        .chevron()
        .on_click(move || {
            app.list.select(&id.get());
            nav.open(&id.get());
        })
        .child(view! {
            <div test_id={format!("{row_id}-title")} flex_1 min_w=0.0 overflow_hidden>
                <text body no_wrap>{title}</text>
            </div>
        });
    if app.list.catalog().is_some() {
        row.action(
            row_action("End thread", move || confirm_end(id.get()))
                .icon(IconName::Square)
                .destructive()
                .test_id(format!("end-{}", id.get().0)),
        )
    } else {
        row
    }
}

fn conversation(app: HostedApp, id: Option<ThreadId>, confirm_end: impl Fn(ThreadId) + Copy + 'static) -> impl View {
    let id = stored(id);
    let shown = memo(move || id.with(|id| app.shown(id.as_ref()?)));
    let has_id = id.with(Option::is_some);
    let title = move || {
        id.with(|id| {
            id.as_ref().filter(|_| shown.get().is_some()).map_or_else(|| "Conversation".to_owned(), |id| app.title(id))
        })
    };
    let room = move || match shown.get() {
        Some(Shown::Conversation(id)) => app.list.open(&id).map(|(_, workspace)| workspace.chat().room()),
        _ => None,
    };
    let people = dynamic(move || room().map(|room| untrack(|| room.toggle_button(IconName::MessageCircle))));
    let sheet = dynamic(move || room().map(|room| untrack(|| room.sheet())));
    let end = app.list.catalog().map(|list| {
        show(
            move || shown.get().is_some(),
            move || {
                icon_button(IconName::Square, "End thread".to_owned())
                    .test_id("end-thread")
                    .disabled(move || id.with(|id| id.as_ref().is_some_and(|id| list.is_changing(id))))
                    .on_click(move || {
                        if let Some(id) = id.get() {
                            confirm_end(id);
                        }
                    })
            },
        )
    });
    let main = dynamic(move || match shown.get() {
        Some(Shown::Conversation(id)) => {
            let opened = untrack(|| app.list.open(&id));
            opened.map(|(_, workspace)| tabs(workspace)).into_any()
        }
        Some(Shown::Waiting(info)) => centered(waiting(app.list, info)),
        None if has_id && !app.list.catalog().is_none_or(ThreadList::is_loaded) => {
            centered(empty_state(IconName::Cloud, "Loading thread…".to_owned(), String::new()))
        }
        None => centered(empty_state(
            IconName::MessageSquareText,
            "Thread unavailable".to_owned(),
            "Go back to Threads to start or reopen a conversation.".to_owned(),
        )),
    });
    let keyboard = keyboard_inset();
    view! {
        <div test_id="conversation-screen" size_full flex col focus_scope bg=Role::Canvas>
            {nav_bar(title).test_id("conversation-nav").trailing(view! { <div flex items_center gap=Space::X1>{people}{end}</div> })}
            <div flex_1 min_h=0.0 pb={move || keyboard.get()}>{main}</div>
            {sheet}
        </div>
    }
}

fn tabs(workspace: Workspace) -> Div {
    view! {
        <div size_full flex col>
            <div flex_none py=Space::X1 flex justify_center>{workspace.switch()}</div>
            <div flex_1 min_h=0.0>{workspace.view(Header::Above, move |reveal| workspace.dock(reveal))}</div>
        </div>
    }
}

fn centered(content: impl View) -> AnyView {
    view! { <div size_full flex items_center justify_center p=Space::X4>{content}</div> }.into_any()
}

const INBOX_MAX_WIDTH: f32 = 600.0;
