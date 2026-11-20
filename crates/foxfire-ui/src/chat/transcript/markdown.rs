use crate::platform::ChatPlatform;
use crate::ui::ActiveReviewTheme;
use rouge::core::View;
use rouge::markdown::MarkdownStream;
use rouge::reactive::{Memo, Stored, effect, signal, stored};
use rouge::widgets::markdown;
use std::rc::Rc;

pub(crate) fn reply(text: Memo<String>, platform: Stored<Rc<dyn ChatPlatform>>) -> impl View {
    let seen = stored(String::new());
    let stream = signal(MarkdownStream::new());
    effect(move || {
        text.with(|text| {
            seen.update(|seen| {
                match text.strip_prefix(seen.as_str()) {
                    Some("") => return,
                    Some(appended) => stream.update(|stream| {
                        stream.push(appended);
                    }),
                    None => stream.update(|stream| {
                        stream.replace(text.as_str());
                    }),
                }
                text.clone_into(seen);
            });
        });
    });
    let theme = ActiveReviewTheme::current().review_signal();
    markdown(stream)
        .palette(theme.select(|theme| theme.markdown))
        .syntax(theme.select(|theme| theme.syntax.clone()))
        .on_link(move |url: String| platform.with(|platform| platform.open(&url)))
}
