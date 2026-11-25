use crate::thread::{Thread, ThreadEvent};
use crate::ui::{ActiveReviewTheme, IconName, Wording, empty_state};
use clankerdiff_core::{DiffReviewCommand, DiffReviewEvent, DiffSide, FilesMode, RepoPath, SourcePath, TargetLocation};
use clankerdiff_rouge::{DiffClientView, DiffViewerOptions, ReviewHandle, ReviewOutput};
use rouge::core::{Styled, View, WindowHandle, div, dynamic, view, window_handle};
use rouge::reactive::{Callback, Memo, Scope, Signal, effect, memo, signal, untrack};
use std::sync::Arc;

/// A thread's changed files and the rest of its repository, with review comments on them.
#[derive(Clone, Copy)]
pub struct Files {
    thread: Thread,
    client: Signal<Option<DiffClientView>>,
    comments: Memo<usize>,
    on_dismissed: Callback,
    window: WindowHandle,
    on_copied: Callback,
    scope: Scope,
}

impl Files {
    /// Calls `on_dismissed` when the user leaves the files, and `on_copied` when their review is on the clipboard.
    pub fn new(thread: Thread, on_dismissed: impl Fn() + 'static, on_copied: impl Fn() + 'static) -> Self {
        let client: Signal<Option<DiffClientView>> = signal(None);
        let comments = memo(move || {
            client.get().map_or(0, |client| {
                client.review().comments_revision().get();
                client.review().with(|review| review.session().review().len())
            })
        });
        let files = Self {
            thread,
            client,
            comments,
            on_dismissed: Callback::from(on_dismissed),
            window: window_handle(),
            on_copied: Callback::from(on_copied),
            scope: Scope::new(),
        };
        let review = memo(move || thread.review().map(|state| Arc::as_ptr(&state) as usize));
        effect(move || {
            review.get();
            untrack(|| files.sync());
        });
        effect(move || {
            let theme = ActiveReviewTheme::current().review();
            untrack(|| {
                if let Some(client) = files.client.get_untracked() {
                    client.review().set_theme(theme);
                }
            });
        });
        thread.on_event(move |event| files.on_thread_event(&event));
        files
    }

    pub fn review(self) -> Option<ReviewHandle> {
        self.client.get_untracked().map(|client| *client.review())
    }

    pub fn comments(self) -> usize {
        self.comments.get()
    }

    pub fn submit(self) {
        self.run(DiffReviewCommand::SubmitReview);
    }

    pub fn copy(self) {
        self.run(DiffReviewCommand::CopyReview);
    }

    pub fn open(self, path: RepoPath, line: Option<u32>) {
        self.run(DiffReviewCommand::SetFilesMode(FilesMode::Diff));
        let line_number = line.map_or(1, |line| line.max(1) as usize);
        let target = TargetLocation { path: SourcePath::Repo(path), side: DiffSide::New, line_number };
        self.run(DiffReviewCommand::JumpTo(target));
    }

    pub fn view(self) -> impl View {
        let (thread, wording) = (self.thread, Wording::current());
        let waiting = memo(move || {
            if thread.status().is_stopped() {
                return (wording.files_stopped, wording.files_stopped_detail.to_owned());
            }
            (wording.files_waiting, thread.listed_files().err().unwrap_or_default())
        });
        let client = self.client;
        let shown = memo(move || client.with(Option::is_some));
        dynamic(move || {
            if shown.get() {
                let client = untrack(|| client.get_untracked());
                return view! { <div test_id="changes-viewer" size_full>{client.map(|client| client.view())}</div> }
                    .into_any();
            }
            let (title, detail) = waiting.get();
            view! {
                <div test_id="files-status" size_full flex items_center justify_center>
                    <empty_state icon=IconName::FileDiff title detail/>
                </div>
            }
            .into_any()
        })
    }

    fn run(self, command: DiffReviewCommand) {
        if let Some(client) = self.client.get_untracked() {
            let _ = client.handle_command(command);
        }
    }

    fn sync(self) {
        let Some(state) = self.thread.review() else {
            self.client.set(None);
            return;
        };
        let client = self.client.get_untracked().unwrap_or_else(|| {
            let theme = ActiveReviewTheme::current().review();
            let options = DiffViewerOptions { themes: false, review_bar: false, ..DiffViewerOptions::default() };
            let client = self.scope.run(|| DiffClientView::mount(theme, options, move |output| self.on_output(output)));
            self.client.set(Some(client));
            client
        });
        client.set_state(&state);
    }

    fn on_output(self, output: ReviewOutput) {
        let ReviewOutput::Event(event) = output else {
            return;
        };
        match event {
            DiffReviewEvent::SubmitReview(submission) => {
                self.thread.update_core(|core| core.submit_review(&submission.formatted));
            }
            DiffReviewEvent::Cancel => self.on_dismissed.call(),
            DiffReviewEvent::CopyFormattedReview(text) => {
                self.window.copy_text(text);
                self.on_copied.call();
            }
            event => self.thread.update_core(|core| core.send_review_event(event)),
        }
    }

    fn on_thread_event(self, event: &ThreadEvent) {
        let Some(client) = self.client.get_untracked() else {
            return;
        };
        match event {
            ThreadEvent::ReviewActionFinished(result) => {
                client.command_finished(result.clone().map_err(|error| error.to_string()));
            }
            ThreadEvent::ReviewLookupFinished(reply) => client.lookup_finished(reply.clone()),
            ThreadEvent::ReviewSubmitted => client.review().clear_review(),
            _ => {}
        }
    }
}
