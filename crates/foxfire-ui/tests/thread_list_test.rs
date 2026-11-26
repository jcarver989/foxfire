use foxfire_core::composer::submission::Submission;
use foxfire_ui::backend::{BackendError, HostedBackend, HostedState, HostedThread, Project, ThreadId, ThreadTitle};
use foxfire_ui::memory::MemoryBackend;
use foxfire_ui::thread::{Thread, ThreadStatus};
use foxfire_ui::thread_list::ThreadList;
use rouge::reactive::{Entered, Runtime, poll_tasks};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

#[tokio::test]
async fn it_lists_projects_and_the_threads_in_them() -> Result<(), TestError> {
    let mut test = ListBuilder::default().build();
    let first = test.launched().await?;
    let second = test.launched().await?;
    test.list.refresh();
    settle();

    assert!(test.list.is_loaded());
    assert_eq!(test.list.projects().len(), 1);
    assert_eq!(test.list.project_threads(PROJECT), [first.clone(), second]);
    assert_eq!(test.list.project_threads("another"), [] as [ThreadId; 0]);
    let info = test.info(&first)?;
    assert_eq!((info.state, info.created_by.name.as_str(), info.title), (HostedState::Ready, "You", None));
    assert_eq!(info.expires_at, Some(info.created_at + LIFETIME));
    Ok(())
}

#[tokio::test]
async fn launching_twice_at_once_starts_one_thread() -> Result<(), TestError> {
    let test = ListBuilder::default().build();
    let launched = Rc::new(Cell::new(0));
    for _ in 0..2 {
        let launched = launched.clone();
        test.list.launch(PROJECT.to_owned(), move |_| launched.set(launched.get() + 1));
        assert!(test.list.is_launching(PROJECT));
    }
    settle();
    assert_eq!((launched.get(), test.list.threads().len()), (1, 1));
    assert!(!test.list.is_launching(PROJECT));
    Ok(())
}

#[tokio::test]
async fn each_state_says_whether_a_thread_can_be_opened() -> Result<(), TestError> {
    let mut test = ListBuilder::default().build();
    let id = test.launched().await?;
    let cases = [
        (HostedState::Starting, ThreadStatus::Connecting, false),
        (HostedState::Ready, ThreadStatus::Idle, true),
        (HostedState::Suspended, ThreadStatus::Dormant, false),
        (HostedState::Ended, ThreadStatus::Dormant, false),
        (HostedState::Failed("no capacity".to_owned()), ThreadStatus::Errored, false),
    ];
    for (state, status, can_open) in cases {
        test.set_state(&id, state.clone())?;
        assert_eq!((test.list.status(&id), test.list.can_open(&id)), (status, can_open), "{state:?}");
        if !can_open {
            assert!(test.list.open(&id).is_none(), "{state:?}");
        }
    }
    Ok(())
}

#[tokio::test]
async fn a_project_is_launching_until_its_thread_is_ready() -> Result<(), TestError> {
    let mut test = ListBuilder::default().starting().build();
    let id = test.launched().await?;
    assert!(test.list.is_launching(PROJECT));
    test.set_state(&id, HostedState::Ready)?;
    assert!(!test.list.is_launching(PROJECT));
    Ok(())
}

#[tokio::test]
async fn an_open_thread_keeps_its_pane_across_refreshes_and_selections() -> Result<(), TestError> {
    let mut test = ListBuilder::default().build();
    let (first, second) = (test.launched().await?, test.launched().await?);
    let (thread, pane) = test.open(&first)?;
    test.list.select(&second);
    test.list.refresh();
    settle();
    test.list.select(&first);
    assert!(test.list.open(&first) == Some((thread, pane)), "the same thread and pane");
    assert_eq!(test.panes.get(), 1);
    Ok(())
}

#[tokio::test]
async fn a_thread_is_titled_by_its_first_prompt_until_someone_renames_it() -> Result<(), TestError> {
    let mut test = ListBuilder::default().build();
    let id = test.launched().await?;
    assert_eq!(test.list.title(&id), None);
    test.prompt(&id, "fix the login")?;
    assert_eq!(test.list.title(&id).as_deref(), Some("fix the login"));
    assert_eq!(test.listed(&id).await?, Some("fix the login".to_owned()), "everyone sees it");

    test.list.rename(id.clone(), "Login".to_owned());
    assert!(test.list.is_changing(&id));
    settle();
    test.prompt(&id, "and the logout")?;
    assert_eq!((test.list.title(&id), test.listed(&id).await?), (Some("Login".to_owned()), Some("Login".to_owned())));
    Ok(())
}

#[tokio::test]
async fn a_first_prompt_does_not_replace_a_title_someone_else_chose() -> Result<(), TestError> {
    let mut test = ListBuilder::default().build();
    let id = test.launched().await?;
    test.backend.rename(id.clone(), ThreadTitle::Chosen("Login".to_owned())).await?;
    test.prompt(&id, "fix the login")?;
    assert_eq!(test.listed(&id).await?, Some("Login".to_owned()));
    assert_eq!(test.list.title(&id).as_deref(), Some("Login"));
    Ok(())
}

#[tokio::test]
async fn mutations_replace_the_entire_row_from_the_backend() -> Result<(), TestError> {
    let mut test = ListBuilder::default().build();
    let id = test.launched().await?;
    test.backend.set_state(&id, HostedState::Starting)?;
    test.list.rename(id.clone(), "Chosen".to_owned());
    settle();
    assert_eq!(test.info(&id)?.state, HostedState::Starting);

    test.backend.rename(id.clone(), ThreadTitle::Chosen("Another client".to_owned())).await?;
    test.list.suspend(id.clone());
    settle();
    assert_eq!(test.info(&id)?.title.as_deref(), Some("Another client"));
    Ok(())
}

#[tokio::test]
async fn a_suspended_thread_resumes_with_its_history_when_it_is_selected() -> Result<(), TestError> {
    let mut test = ListBuilder::default().build();
    let id = test.launched().await?;
    let (thread, _) = test.open(&id)?;
    test.prompt(&id, "remember this")?;
    test.list.suspend(id.clone());
    settle();
    assert_eq!((test.info(&id)?.state, thread.status()), (HostedState::Suspended, ThreadStatus::Dormant));
    assert!(!test.list.can_open(&id));

    test.list.select(&id);
    assert!(test.list.can_open(&id), "it is shown while it resumes");
    settle();
    assert_eq!((test.info(&id)?.state, thread.status()), (HostedState::Ready, ThreadStatus::Idle));
    assert!(thread.with_core(|core| !core.conversation().items().is_empty()).unwrap_or(false));
    Ok(())
}

#[tokio::test]
async fn a_thread_someone_else_suspends_is_not_resumed_by_watching_it() -> Result<(), TestError> {
    let mut test = ListBuilder::default().build();
    let id = test.launched().await?;
    let (thread, _) = test.open(&id)?;
    test.backend.suspend(id.clone()).await?;
    settle();
    assert_eq!((test.info(&id)?.state, thread.status()), (HostedState::Suspended, ThreadStatus::Dormant));
    Ok(())
}

#[tokio::test]
async fn a_thread_that_ends_while_it_is_open_keeps_its_transcript() -> Result<(), TestError> {
    let mut test = ListBuilder::default().build();
    let id = test.launched().await?;
    let (thread, _) = test.open(&id)?;
    test.prompt(&id, "hello")?;
    test.backend.set_state(&id, HostedState::Ended)?;
    settle();
    assert_eq!(test.info(&id)?.state, HostedState::Ended);
    assert!(test.list.open(&id).is_some());
    assert!(thread.with_core(|core| !core.conversation().items().is_empty()).unwrap_or(false));
    Ok(())
}

#[tokio::test]
async fn stopping_a_thread_removes_it_and_what_was_open() -> Result<(), TestError> {
    let mut test = ListBuilder::default().build();
    let id = test.launched().await?;
    let (thread, _) = test.open(&id)?;
    test.list.select(&id);
    test.list.stop(id.clone(), |_| {});
    settle();
    assert!(test.list.threads().is_empty() && test.list.selected().is_none() && !thread.is_alive());
    Ok(())
}

#[tokio::test]
async fn a_thread_the_backend_no_longer_knows_is_removed() -> Result<(), TestError> {
    let mut test = ListBuilder::default().build();
    let id = test.launched().await?;
    test.backend.stop(id.clone()).await?;
    test.list.rename(id.clone(), "Gone".to_owned());
    settle();
    assert_eq!(test.list.threads(), []);
    assert_eq!(test.list.error(), Some(BackendError::UnknownThread(id)));
    Ok(())
}

#[tokio::test]
async fn being_signed_out_is_reported_by_listing_and_by_opening() -> Result<(), TestError> {
    let mut test = ListBuilder::default().build();
    let id = test.launched().await?;
    test.backend.fail_with(Some(BackendError::Unauthenticated));
    test.list.refresh();
    settle();
    assert_eq!(test.list.error(), Some(BackendError::Unauthenticated));
    assert_eq!(test.list.threads().len(), 1, "what was listed is kept");

    test.list.dismiss_error();
    test.open(&id)?;
    settle();
    assert_eq!(test.list.error(), Some(BackendError::Unauthenticated));
    Ok(())
}

#[tokio::test]
async fn a_listing_that_a_later_change_overtakes_is_ignored() -> Result<(), TestError> {
    let test = ListBuilder::default().build();
    settle();
    test.list.refresh();
    test.list.launch(PROJECT.to_owned(), |_| {});
    settle();
    assert_eq!(test.list.threads().len(), 1);
    Ok(())
}

#[derive(Debug, thiserror::Error)]
enum TestError {
    #[error(transparent)]
    Backend(#[from] BackendError),
    #[error("no such thread: {0:?}")]
    Missing(ThreadId),
}

const PROJECT: &str = "demo";
const LIFETIME: Duration = Duration::from_hours(4);

#[derive(Default)]
struct ListBuilder {
    starting: bool,
}

struct TestList {
    list: ThreadList<usize>,
    backend: MemoryBackend,
    panes: Rc<Cell<usize>>,
    _entered: Entered,
}

impl ListBuilder {
    fn starting(mut self) -> Self {
        self.starting = true;
        self
    }

    fn build(self) -> TestList {
        let entered = Runtime::new().enter();
        let backend = MemoryBackend::new(vec![Project { id: PROJECT.to_owned(), name: "Demo project".to_owned() }])
            .with_reply(vec!["done".to_owned()], Duration::ZERO)
            .with_lifetime(LIFETIME);
        let backend = if self.starting { backend.with_startup() } else { backend };
        let panes = Rc::new(Cell::new(0));
        let built = panes.clone();
        let list = ThreadList::new(Rc::new(backend.clone()), move |_| {
            built.set(built.get() + 1);
            built.get()
        });
        settle();
        TestList { list, backend, panes, _entered: entered }
    }
}

impl TestList {
    /// A thread someone launched, once the list shows it.
    async fn launched(&mut self) -> Result<ThreadId, TestError> {
        let id = self.backend.launch(PROJECT.to_owned()).await?.id;
        self.list.refresh();
        settle();
        Ok(id)
    }

    fn open(&self, id: &ThreadId) -> Result<(Thread, usize), TestError> {
        let opened = self.list.open(id).ok_or_else(|| TestError::Missing(id.clone()))?;
        settle();
        Ok(opened)
    }

    fn prompt(&self, id: &ThreadId, text: &str) -> Result<(), TestError> {
        let (thread, _) = self.open(id)?;
        thread.send(Submission::from(text));
        settle();
        Ok(())
    }

    fn set_state(&self, id: &ThreadId, state: HostedState) -> Result<(), TestError> {
        self.backend.set_state(id, state)?;
        self.list.refresh();
        settle();
        Ok(())
    }

    fn info(&self, id: &ThreadId) -> Result<HostedThread, TestError> {
        self.list.info(id).ok_or_else(|| TestError::Missing(id.clone()))
    }

    /// The title the backend lists, for anyone else using it.
    async fn listed(&self, id: &ThreadId) -> Result<Option<String>, TestError> {
        let threads = self.backend.list_threads().await?;
        let thread = threads.into_iter().find(|thread| thread.id == *id);
        thread.map(|thread| thread.title).ok_or_else(|| TestError::Missing(id.clone()))
    }
}

/// Runs every task that is ready, which nothing here leaves waiting on time.
fn settle() {
    while poll_tasks() {}
}
