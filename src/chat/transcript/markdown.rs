use crate::platform::Platform;
use crate::ui::ActiveReviewTheme;
use rouge::core::{View, WindowHandle};
use rouge::markdown::MarkdownStream;
use rouge::reactive::{Memo, Stored, effect, signal, stored};
use rouge::widgets::{MarkdownAction, MarkdownPaint, MarkdownRows, markdown};
use std::rc::Rc;

pub(crate) fn reply(text: Memo<String>, window: WindowHandle, platform: Stored<Rc<dyn Platform>>) -> impl View {
    let stream = stored((String::new(), MarkdownStream::new()));
    let rows = signal(MarkdownRows::default());
    effect(move || {
        text.with(|text| {
            stream.update(|(seen, stream)| {
                match text.strip_prefix(seen.as_str()) {
                    Some("") => return,
                    Some(appended) => drop(stream.push(appended)),
                    None => drop(stream.replace(text.as_str())),
                }
                text.clone_into(seen);
                rows.update(|rows| rows.sync(stream));
            });
        });
    });
    let theme = ActiveReviewTheme::current().review_signal();
    let paint = MarkdownPaint::new(
        theme.select(|theme| theme.markdown),
        theme.select(|theme| theme.syntax.clone()),
        move |action| match action {
            MarkdownAction::CopyCode(code) => window.copy_text(code),
            MarkdownAction::OpenLink(url) => platform.with(|platform| platform.open(&url)),
        },
    );
    markdown(rows.read_only(), paint)
}
