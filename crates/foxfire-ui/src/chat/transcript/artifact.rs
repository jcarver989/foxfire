use super::Transcript;
use crate::thread::Thread;
use crate::ui::{IconName, UiStyles, web_card};
use foxfire_acp::artifact::{Artifact, ArtifactContent, ArtifactId, ArtifactKind, ArtifactSlot};
use foxfire_acp::conversation::{ConversationContent, ConversationItem};
use rouge::core::{AnyView, View, dynamic, text};
use rouge::reactive::{Scope, Stored, memo, spawn, stored, untrack};
use rouge::webview::{WebContent, WebPage, web_page};
use std::collections::HashMap;

#[derive(Clone, Copy)]
pub(super) struct ArtifactPages(Stored<HashMap<ArtifactSlot, ArtifactPage>>);

struct ArtifactPage {
    scope: Scope,
    page: WebPage,
    showing: ArtifactId,
}

impl Transcript {
    pub(super) fn artifact(self, index: usize) -> AnyView {
        let thread = self.thread;
        let revision = memo(move || {
            thread.with_core(|core| core.conversation().items().get(index).map(ConversationItem::revision)).flatten()
        });
        dynamic(move || {
            revision.get();
            let shown = untrack(|| {
                thread.with_core(|core| match core.conversation().items().get(index)?.content() {
                    ConversationContent::Artifact(artifact) => Some(artifact.clone()),
                    _ => None,
                })
            });
            let Some(artifact) = shown.flatten() else { return ().into_any() };
            let tag = format!("v{}", artifact.version);
            let label = || text(format!("{} ({tag})", artifact.name)).small().into_any();
            if self.preview.with(Option::is_some) || self.full_diff.with(Option::is_some) {
                return label();
            }
            let icon = match artifact.kind {
                ArtifactKind::Html => IconName::FileText,
                ArtifactKind::Image(_) => IconName::Image,
                _ => return label(),
            };
            let page = self.pages.show(&artifact, thread, self.scope);
            web_card(page, artifact.name).icon(icon).tag(tag).test_id(format!("artifact-{index}")).into_any()
        })
        .into_any()
    }
}

impl ArtifactPages {
    pub(super) fn new() -> Self {
        Self(stored(HashMap::new()))
    }

    pub(super) fn show(self, artifact: &Artifact, thread: Thread, parent: Scope) -> WebPage {
        let shown = self.0.with(|pages| pages.get(&artifact.slot).map(|shown| (shown.page, shown.showing)));
        let page = match shown {
            Some((page, showing)) if showing == artifact.id => return page,
            Some((page, _)) => {
                self.0.update(|pages| pages.get_mut(&artifact.slot).map(|shown| shown.showing = artifact.id));
                page
            }
            None => {
                let scope = parent.child();
                let page = scope.run(|| web_page(WebContent::html(String::new())));
                let shown = ArtifactPage { scope, page, showing: artifact.id };
                self.0.update(|pages| pages.insert(artifact.slot.clone(), shown));
                page
            }
        };
        let id = artifact.id;
        spawn(async move { thread.load_artifact(id) });
        page
    }

    pub(super) fn loaded(self, content: &ArtifactContent) {
        let web = match content {
            ArtifactContent::Html(html) => WebContent::html(html),
            ArtifactContent::Image(..) => WebContent::html(format!(
                r#"<body style="margin:0"><img src="{}" style="display:block;max-width:100%"></body>"#,
                content.data_url()
            )),
            _ => return,
        };
        let id = content.id();
        let pages: Vec<_> =
            self.0.with(|pages| pages.values().filter(|shown| shown.showing == id).map(|shown| shown.page).collect());
        for page in pages {
            page.load(web.clone());
        }
    }

    pub(super) fn clear(self) {
        self.0.update(|pages| pages.drain().for_each(|(_, shown)| shown.scope.dispose()));
    }
}
