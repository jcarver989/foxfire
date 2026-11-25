mod completion;
mod queued;

use super::attachment::{chip, preview};
use crate::platform::ChatPlatform;
use crate::thread::{Thread, ThreadEvent, ThreadStatus};
use crate::ui::tokens::{COMPOSER_POPUP_HEIGHT, CONTROL_HEIGHT_LARGE, ICON_SIZE, ROW_HEIGHT, Radius, Space};
use crate::ui::typography::READING;
use crate::ui::{
    IconButton, IconName, Material, MaterialExt, PickerItem, SearchPickerEvent, SearchPickerHandle, UI_FONT,
    icon_button, picker_closed, popping, presence, search_picker,
};
use crate::ui::{chord, spawn_in};
use completion::{CompletionItem, CompletionMenu, active_token};
use foxfire_core::composer::attachment::{AttachedFile, PromptAttachment, attach_missing};
use foxfire_core::composer::completion_token::Trigger;
use foxfire_core::composer::prompt_history::PromptHistory;
use foxfire_core::composer::prompt_search::PromptSearchStatus;
use foxfire_core::composer::submission::Submission;
use foxfire_core::session_core::SessionCore;
use foxfire_core::session_model::WorkspaceAccess;
use queued::queued_prompts;
use rouge::core::{
    AnyView, Apply, ClipboardMedia, Color, Cursor, Editable, Fill, FocusHandle, FocusPolicy, Key, KeyEvent, Modifiers,
    Role, Styled, View, div, dynamic, editable, focus_handle, presentation, px, show, view,
};
use rouge::reactive::{Callback, Scope, Signal, Stored, batch, effect, memo, signal, stored, untrack};
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SearchPrompts;

#[derive(Clone, Copy)]
pub struct Composer {
    thread: Thread,
    text: Signal<String>,
    selection: Signal<Range<usize>>,
    mentions: Signal<Vec<PromptAttachment>>,
    media: Signal<Vec<PromptAttachment>>,
    history: Stored<PromptHistory>,
    recalled: Stored<bool>,
    popup: Signal<Popup>,
    search_status: Signal<Option<String>>,
    focus: FocusHandle,
    platform: Stored<Rc<dyn ChatPlatform>>,
    scope: Scope,
}

#[derive(Clone)]
enum Popup {
    Closed,
    Completion(CompletionMenu),
    Dismissed { token_start: usize },
    Search(SearchPickerHandle),
}

impl Composer {
    pub fn new(thread: Thread, platform: Rc<dyn ChatPlatform>) -> Self {
        let composer = Self {
            thread,
            text: signal(String::new()),
            selection: signal(0..0),
            mentions: signal(Vec::new()),
            media: signal(Vec::new()),
            history: stored(PromptHistory::default()),
            recalled: stored(false),
            popup: signal(Popup::Closed),
            search_status: signal(None),
            focus: focus_handle(),
            platform: stored(platform),
            scope: Scope::new(),
        };
        thread.on_event(move |event| match event {
            ThreadEvent::SubmissionRestored(submission) => composer.restore(&submission),
            ThreadEvent::PromptSearchUpdated => composer.show_prompt_results(),
            _ => {}
        });
        effect(move || {
            composer.text.get();
            untrack(|| {
                if !composer.recalled.update(std::mem::take) {
                    composer.history.update(PromptHistory::reset);
                }
            });
        });
        effect(move || {
            composer.text.get();
            composer.selection.get();
            untrack(|| composer.refresh_completion());
        });
        let listing = memo(move || thread.listed_files());
        effect(move || {
            listing.with(|_| ());
            untrack(|| {
                if composer.completion().is_some_and(|menu| menu.token().trigger == Trigger::File) {
                    composer.refresh_completion();
                }
            });
        });
        composer
    }

    pub fn focus(self) {
        self.focus.focus();
    }

    /// Types `typed` at the caret, replacing any selection, and focuses the composer.
    pub fn type_text(self, typed: &str) {
        self.replace(self.selection.get_untracked(), typed);
        self.focus();
    }

    pub fn attach(self, paths: &[PathBuf]) {
        if !self.platform.with(|platform| platform.can_attach_local_files()) {
            return;
        }
        let remote =
            self.thread.with_core(|core| core.session().workspace_access() == WorkspaceAccess::Remote).unwrap_or(false);
        let mut refused = Vec::new();
        let root = self.root().unwrap_or_default();
        for file in self.platform.with(|platform| platform.classify_local_paths(paths, &root)) {
            match file {
                AttachedFile::Media(media) => self.media.update(|attached| attach_missing(attached, [media])),
                AttachedFile::Mention(mention) => self.insert_mention(self.selection.get_untracked(), mention),
                AttachedFile::Unsupported(file) if remote => refused.push(file.display_name),
                AttachedFile::Unsupported(file) => {
                    self.replace(self.selection.get_untracked(), &format!("{} ", file.path.display()));
                }
            }
        }
        if !refused.is_empty() {
            let message = format!("Can't attach folders or binary files to a remote agent: {}", refused.join(", "));
            self.thread.report_error(&message);
        }
    }

    pub fn view(self) -> impl View {
        let thread = self.thread;
        let queued =
            memo(move || thread.with_core(|core| core.queued_prompts().to_vec()).filter(|prompts| !prompts.is_empty()));
        let completion = memo(move || self.completion());
        let search = memo(move || self.popup.with(|popup| matches!(popup, Popup::Search(_))));
        let editor = view! {
            <div w_full flex col gap=Space::X1 focusable_glass={move || self.focus.is_focused()}>
                <dynamic>{self.attachments()}</dynamic>
                {self.input()}
                {self.actions()}
            </div>
        };
        view! {
            <div test_id="composer" mt=Space::X2 flex col gap=Space::X2>
                {popping(queued, |prompts| queued_prompts(&prompts))}
                {presence(move || search.get(), move || untrack(|| self.search_view()))}
                {popping(completion, move |menu| self.completion_view(&menu))}
                {editor}
            </div>
        }
    }

    fn input(self) -> impl View {
        let touch = presentation().get_untracked().is_touch();
        let field = editable(self.text)
            .test_id("composer-text")
            .multiline()
            .min_rows(MIN_ROWS)
            .max_rows(MAX_ROWS)
            .submit_on_enter(!touch)
            .selection(self.selection)
            .track_focus(self.focus)
            .when(!touch, Editable::autofocus)
            .font(UI_FONT)
            .text_size(READING.size)
            .line_height(READING.line_height)
            .color(Role::Text)
            .placeholder(PLACEHOLDER, Role::TextMuted)
            .w_full()
            .on_key_down(move |event| {
                if self.intercept(event) {
                    event.stop_propagation();
                }
            })
            .on_paste(move |media| self.paste(media))
            .on_submit(move |_| self.submit());
        div()
            .layer()
            .relative()
            .w_full()
            .min_w(0.0)
            .px(Space::X3)
            .pt(Space::X2)
            .cursor(Cursor::Text)
            .bind_key(chord(SEARCH_PROMPTS_KEYS), SearchPrompts)
            .on_action(move |_: &SearchPrompts| self.open_search())
            .child(field)
    }

    fn actions(self) -> impl View {
        let thread = self.thread;
        let touch = presentation().get_untracked().is_touch();
        let empty = memo(move || self.text.with(|text| text.trim().is_empty()) && self.media.with(Vec::is_empty));
        let running = memo(move || running(thread.status()));
        let stopping = memo(move || running.get() && empty.get());
        let (side, glyph) = if touch { (CONTROL_HEIGHT_LARGE, Space::X3) } else { (ROW_HEIGHT, Space::X2) };
        let stop = move || {
            let cancel = Callback::from(move || self.cancel());
            let button =
                round_button("stop", IconName::Square, ("Cancel response", "escape"), Face::Busy, touch, cancel);
            view! {
                <div absolute inset=0.0>
                    {button}
                    <div absolute inset=0.0 flex items_center justify_center pass_through>
                        <div test_id="stop-glyph" flex_none size=glyph bg=Role::Text pass_through/>
                    </div>
                </div>
            }
        };
        let send = move || {
            view! {
                <div absolute inset=0.0><dynamic>{
                    let face = if empty.get() { Face::Idle } else { Face::Filled };
                    round_button("send", IconName::ArrowUp, ("Send", "enter"), face, touch, move || self.send())
                }</dynamic></div>
            }
        };
        view! {
            <div px=Space::X2 pb=Space::X2 flex items_center gap=Space::X1>
                <show when={move || self.platform.with(|platform| platform.can_attach_local_files())}>
                    <icon_button test_id="attach-files" icon=IconName::Paperclip label="Attach files"
                                 on_click={move || self.pick_files()}/>
                </show>
                <div flex_1/>
                <div flex_none relative size=side preserve_focus>
                    {presence(move || stopping.get(), stop)}
                    {presence(move || !stopping.get(), send)}
                </div>
            </div>
        }
    }

    fn attachments(self) -> Option<AnyView> {
        let media = self.media.get();
        if media.is_empty() {
            return None;
        }
        let chips = media.iter().enumerate().map(|(index, attachment)| {
            let id = format!("composer-attachment-{index}");
            let thumbnail = self
                .platform
                .with(|platform| preview(format!("{id}-thumbnail"), attachment, ICON_SIZE, platform.as_ref()));
            let remove = view! {
                <icon_button test_id={format!("{id}-remove")} icon=IconName::X label="Remove"
                             on_click={move || self.remove_media(index)}/>
            };
            chip(id, thumbnail, &attachment.display_name).py(0.0).pr(0.0).child(remove)
        });
        Some(view! { <div px=Space::X2 pt=Space::X2 flex flex_wrap gap=Space::X1>{..chips}</div> }.into_any())
    }

    fn completion_view(self, menu: &CompletionMenu) -> AnyView {
        let choose = Callback::from(move |position: usize| self.accept_completion(position));
        menu.render(choose).into_any()
    }

    fn search_view(self) -> Option<AnyView> {
        let Popup::Search(handle) = self.popup.get_untracked() else {
            return None;
        };
        let status = self.search_status;
        Some(
            view! {
                <search_picker test_id="prompt-search" handle material={Material::Glass}
                               searchable="Search past prompts…" empty="No matching prompts"
                               max_list_height={px(COMPOSER_POPUP_HEIGHT)} status=status/>
            }
            .into_any(),
        )
    }

    fn intercept(self, event: &KeyEvent) -> bool {
        let plain = event.modifiers == Modifiers::NONE;
        match event.key {
            Key::ArrowUp if plain => self.step_or_recall(-1),
            Key::ArrowDown if plain => self.step_or_recall(1),
            Key::Tab if plain => self.completion().is_some_and(|menu| {
                self.accept_completion(menu.highlighted());
                true
            }),
            Key::Escape => self.escape(),
            Key::Backspace if plain => self.pop_media(),
            _ => false,
        }
    }

    fn submit(self) {
        match self.completion().filter(|menu| menu.item(menu.highlighted()).is_some()) {
            Some(menu) => self.accept_completion(menu.highlighted()),
            None => self.send(),
        }
    }

    fn send(self) {
        let text = self.text.get_untracked();
        let media = self.media.get_untracked();
        if text.trim().is_empty() && media.is_empty() {
            return;
        }
        let mentions = mentioned(&text, &self.mentions.get_untracked());
        let submission = Submission { text: text.clone(), mentions, media, ..Submission::default() };
        self.history.update(|history| history.push(&text));
        batch(|| {
            self.close_completion();
            self.text.set(String::new());
            self.selection.set(0..0);
            self.media.set(Vec::new());
            self.mentions.set(Vec::new());
        });
        self.thread.send(submission);
    }

    fn accept_completion(self, position: usize) {
        let Some(menu) = self.completion() else {
            return;
        };
        self.popup.set(Popup::Closed);
        let Some(item) = menu.item(position).cloned() else {
            return;
        };
        let range = menu.token().range.clone();
        match item {
            CompletionItem::File(file) => self.insert_mention(range, file),
            CompletionItem::Command(command) => {
                let mut replacement = format!("/{}", command.name);
                if command.has_input {
                    replacement.push(' ');
                }
                self.replace(range, &replacement);
                if !command.has_input {
                    self.send();
                }
            }
        }
    }

    fn insert_mention(self, range: Range<usize>, file: PromptAttachment) {
        self.replace(range, &format!("{} ", mention_text(&file)));
        self.mentions.update(|mentions| attach_missing(mentions, [file]));
    }

    fn replace(self, range: Range<usize>, replacement: &str) {
        let mut text = self.text.get_untracked();
        let end = floor_boundary(&text, range.end);
        let start = floor_boundary(&text, range.start.min(end));
        text.replace_range(start..end, replacement);
        let caret = start + replacement.len();
        batch(|| {
            self.text.set(text);
            self.selection.set(caret..caret);
        });
    }

    fn paste(self, media: ClipboardMedia) {
        if !self.platform.with(|platform| platform.can_attach_local_files()) {
            return;
        }
        match media {
            ClipboardMedia::Files(paths) => self.attach(&paths),
            ClipboardMedia::Png(png) => match self.platform.with(|platform| platform.save_pasted_image(&png)) {
                Ok(image) => self.media.update(|media| attach_missing(media, [image])),
                Err(message) => self.thread.report_error(&message),
            },
        }
    }

    fn pick_files(self) {
        let picked = self.platform.with(|platform| platform.pick_files());
        spawn_in(self.scope, async move {
            if let Some(paths) = picked.await {
                self.attach(&paths);
                self.focus();
            }
        });
    }

    fn refresh_completion(self) {
        let text = self.text.get_untracked();
        let cursor = self.selection.get_untracked().end.min(text.len());
        let spans = mention_spans(&text, &self.mentions.get_untracked());
        let Some(token) = active_token(&text, cursor, &spans) else {
            self.close_completion();
            return;
        };
        let skip = self.popup.with_untracked(|popup| match popup {
            Popup::Dismissed { token_start } => *token_start == token.range.start,
            Popup::Search(_) => true,
            Popup::Closed | Popup::Completion(_) => false,
        });
        if skip {
            return;
        }
        let menu = match token.trigger {
            Trigger::Command => {
                let commands = self.thread.with_core(|core| core.available_commands().to_vec()).unwrap_or_default();
                (!commands.is_empty()).then(|| CompletionMenu::commands(token, &text, &commands))
            }
            Trigger::File => Some(CompletionMenu::files(token, &text, self.thread.listed_files())),
        };
        self.popup.set(menu.map_or(Popup::Closed, Popup::Completion));
    }

    fn root(self) -> Option<PathBuf> {
        self.thread.listed_files().map(|files| files.root).ok().or_else(|| self.thread.fallback_cwd())
    }

    fn cancel(self) {
        self.thread.update_core(SessionCore::cancel);
    }

    fn completion(self) -> Option<CompletionMenu> {
        self.popup.with(|popup| match popup {
            Popup::Completion(menu) => Some(menu.clone()),
            _ => None,
        })
    }

    fn close_completion(self) {
        if self.popup.with_untracked(|popup| matches!(popup, Popup::Completion(_) | Popup::Dismissed { .. })) {
            self.popup.set(Popup::Closed);
        }
    }

    fn escape(self) -> bool {
        if let Some(menu) = self.completion() {
            self.popup.set(Popup::Dismissed { token_start: menu.token().range.start });
            return true;
        }
        if running(self.thread.status()) {
            self.cancel();
            return true;
        }
        false
    }

    fn pop_media(self) -> bool {
        self.text.with_untracked(String::is_empty) && self.media.try_update(Vec::pop).flatten().is_some()
    }

    fn remove_media(self, index: usize) {
        self.media.update(|media| {
            if index < media.len() {
                media.remove(index);
            }
        });
        self.focus();
    }

    fn step_or_recall(self, by: isize) -> bool {
        if let Some(mut menu) = self.completion() {
            menu.step(by);
            self.popup.set(Popup::Completion(menu));
            return true;
        }
        let text = self.text.get_untracked();
        let recalled = if by < 0 {
            if !text.is_empty() && !self.history.with(PromptHistory::is_recalling) {
                return false;
            }
            self.history.update(|history| history.older(&text).map(str::to_owned))
        } else {
            if !self.history.with(PromptHistory::is_recalling) {
                return false;
            }
            self.history.update(PromptHistory::newer)
        };
        let Some(prompt) = recalled else {
            return false;
        };
        self.set_text(prompt);
        true
    }

    fn set_text(self, value: String) {
        let caret = value.len();
        batch(|| {
            self.recalled.set(true);
            self.close_completion();
            self.text.set(value);
            self.selection.set(caret..caret);
        });
    }

    fn open_search(self) {
        if self.popup.with_untracked(|popup| matches!(popup, Popup::Search(_))) || !self.thread.can_search_prompts() {
            return;
        }
        let handle = self.scope.run(|| {
            SearchPickerHandle::new(Vec::new(), move |event| {
                if let SearchPickerEvent::Query(query) = &event {
                    self.search_prompts(query);
                }
                let closed = picker_closed(&event, |index| {
                    let prompt = self.thread.with_core(|core| {
                        core.prompt_search().results().get(index).map(|result| result.prompt.clone())
                    });
                    if let Some(prompt) = prompt.flatten() {
                        self.set_text(prompt);
                    }
                });
                if closed {
                    self.popup.set(Popup::Closed);
                    self.focus();
                }
            })
            .unfiltered()
        });
        self.popup.set(Popup::Search(handle));
        self.search_prompts("");
    }

    fn search_prompts(self, query: &str) {
        self.thread.update_core(|core| core.search_prompts(query));
        self.show_prompt_results();
    }

    fn show_prompt_results(self) {
        let Popup::Search(handle) = self.popup.get_untracked() else {
            return;
        };
        let Some((items, status)) = self.thread.with_core(|core| {
            let search = core.prompt_search();
            let items: Vec<PickerItem> = search
                .results()
                .iter()
                .enumerate()
                .map(|(n, result)| {
                    PickerItem::new(
                        format!("prompt-search-result-{n}"),
                        result.prompt.lines().next().unwrap_or_default().to_owned(),
                    )
                })
                .collect();
            let status = match search.status() {
                PromptSearchStatus::Failed(error) => Some(format!("Search failed: {error}")),
                PromptSearchStatus::Searching => Some("Searching…".to_owned()),
                PromptSearchStatus::Blank => Some("Type to search the prompts you have sent".to_owned()),
                PromptSearchStatus::NoMatches | PromptSearchStatus::Results => None,
            };
            (items, status)
        }) else {
            return;
        };
        handle.set_items(items);
        self.search_status.set_if_changed(status);
    }

    fn restore(self, submission: &Submission) {
        if !submission.text.is_empty() {
            let draft = self.text.get_untracked();
            self.set_text(submission.restore_onto(&draft));
        }
        self.mentions.update(|mentions| attach_missing(mentions, submission.mentions.iter().cloned()));
        self.media.update(|media| attach_missing(media, submission.media.iter().cloned()));
    }
}

fn running(status: ThreadStatus) -> bool {
    matches!(status, ThreadStatus::Running | ThreadStatus::Background | ThreadStatus::NeedsInput)
}

fn round_button(
    id: &'static str,
    icon: IconName,
    (label, keys): (&'static str, &'static str),
    face: Face,
    touch: bool,
    on_click: impl Into<Callback>,
) -> IconButton {
    let (background, hovered, foreground) = match face {
        Face::Filled => (Role::Text, Role::TextSecondary, Fill::Role(Role::Canvas)),
        Face::Idle => (Role::SurfaceHover, Role::SurfaceSelected, Fill::Role(Role::TextMuted)),
        Face::Busy => (Role::SurfaceSelected, Role::SurfaceHover, Fill::Color(Color::TRANSPARENT)),
    };
    icon_button(icon, label.to_owned())
        .test_id(id)
        .size(if touch { CONTROL_HEIGHT_LARGE } else { ROW_HEIGHT })
        .rounded(Radius::Full)
        .bg(background)
        .color(foreground)
        .hover(move |style| style.bg(hovered))
        .when(!touch, |button| button.shortcut(chord(keys)))
        .focus_policy(FocusPolicy::None)
        .on_click(on_click)
}

#[derive(Clone, Copy)]
enum Face {
    Filled,
    Idle,
    Busy,
}

fn mention_text(file: &PromptAttachment) -> String {
    format!("@{}", file.display_name)
}

fn mention_spans(text: &str, mentions: &[PromptAttachment]) -> Vec<Range<usize>> {
    mentions
        .iter()
        .flat_map(|mention| {
            let needle = mention_text(mention);
            text.match_indices(&needle).map(|(start, _)| start..start + needle.len()).collect::<Vec<_>>()
        })
        .collect()
}

fn mentioned(text: &str, mentions: &[PromptAttachment]) -> Vec<PromptAttachment> {
    mentions.iter().filter(|mention| text.contains(&mention_text(mention))).cloned().collect()
}

fn floor_boundary(text: &str, index: usize) -> usize {
    text.floor_char_boundary(index.min(text.len()))
}

const MIN_ROWS: usize = 1;
const MAX_ROWS: usize = 12;
const SEARCH_PROMPTS_KEYS: &str = "secondary-r";
const PLACEHOLDER: &str = "Message the agent…  / for commands, @ for files";
