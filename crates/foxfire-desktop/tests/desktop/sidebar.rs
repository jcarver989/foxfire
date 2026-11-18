use crate::support::Edges;
use crate::support::replying;
use foxfire_desktop::testing::{DesktopHarness, DesktopHarnessBuilder};
use std::path::{Path, PathBuf};

#[test]
fn the_sidebar_lists_projects_and_their_live_threads_without_asking_agents_for_saved_sessions() {
    let (mut harness, _) = DesktopHarnessBuilder::default()
        .session("saved", "/alpha", "2026-01-07T00:00:00Z", "saved prompt")
        .session("gamma-session", "/gamma", "2026-01-07T00:00:00Z", "gamma prompt")
        .agent("/alpha", replying("alpha-session", "hello"))
        .project("/beta")
        .start("/alpha");

    let mut row_top = |selector| harness.bounds(selector).expect("row is painted").top();
    assert!(row_top("project-alpha") < row_top("thread-row-0"), "a live thread sits under its project");
    assert!(row_top("thread-row-0") < row_top("project-beta"), "and above the next project");
    assert!(harness.bounds("project-gamma").is_none(), "a project only an agent knows about is not listed");
    assert!(harness.sessions().asked().is_empty(), "no agent is asked for its saved sessions");
}

#[test]
fn opening_a_thread_in_a_project_adds_the_project_to_the_sidebar() {
    let mut harness = DesktopHarnessBuilder::default().script("/beta", replying("beta-session", "hi")).build();
    assert!(harness.bounds("project-beta").is_none());

    harness.new_thread("/beta");

    assert!(harness.bounds("project-beta").is_some());
    assert_eq!(harness.projects(), [PathBuf::from("/beta")]);
}

#[test]
fn removing_a_project_hides_it() {
    let mut harness = DesktopHarnessBuilder::default().project("/alpha").project("/beta").build();

    harness.click("project-alpha-actions").unwrap();
    harness.click("menu-remove-project").unwrap();

    assert!(harness.bounds("project-alpha").is_none());
    assert!(harness.bounds("project-beta").is_some());
    assert_eq!(harness.projects(), [PathBuf::from("/beta")]);
}

#[test]
fn a_rows_archive_button_stops_its_thread_and_removes_it_from_the_sidebar() {
    let (mut harness, _) = DesktopHarness::alpha(replying("alpha-session", "hello"));

    harness.hover("thread-row-0").unwrap();
    harness.click("thread-row-0-archive").unwrap();

    harness.run_until(|harness| harness.connector().live_agents("/alpha") == 0);
    assert!(harness.bounds("thread-row-0").is_none());
}

#[test]
fn adding_a_picked_folder_lists_it() {
    let mut harness = DesktopHarnessBuilder::default()
        .session("beta-session", "/beta", "2026-01-02T00:00:00Z", "prompt")
        .project("/alpha")
        .build();

    harness.click("add-project").unwrap();
    let prompt = harness.platform().answer_path_prompt(Some(vec![PathBuf::from("/beta")]));

    assert!(prompt.is_some_and(|prompt| prompt.directories && !prompt.multiple));
    harness.run_until_painted("project-beta");
    assert!(harness.sessions().asked().is_empty(), "an added project's agents are not asked for sessions");
}

#[test]
fn a_project_folds_its_rows_away_and_back() {
    let (mut harness, _) = DesktopHarness::alpha(replying("alpha-session", "hello"));
    assert!(harness.bounds("thread-row-0").is_some(), "a project starts expanded");

    harness.click("project-alpha").unwrap();
    assert!(harness.bounds("thread-row-0").is_none());

    harness.click("project-alpha").unwrap();
    assert!(harness.bounds("thread-row-0").is_some());
}

#[test]
fn a_projects_plus_starts_a_selected_thread_under_it() {
    let mut harness =
        DesktopHarnessBuilder::default().agent("/alpha", replying("alpha-session", "hello")).project("/beta").build();

    harness.click("project-alpha-new-thread").unwrap();

    let thread = harness.connected_thread();
    assert_eq!(thread.target().cwd().map(Path::to_path_buf), Some("/alpha".into()));
    let [project, live, next] =
        ["project-alpha", "thread-row-0", "project-beta"].map(|id| harness.bounds(id).expect("painted"));
    assert!(project.top() < live.top() && live.top() < next.top());
}

#[test]
fn clicking_a_live_row_selects_its_thread() {
    let mut harness = alpha_and_beta();
    harness.click("project-alpha-new-thread").unwrap();
    harness.click("project-beta-new-thread").unwrap();
    assert_eq!(selected_cwd(&harness), Path::new("/beta"));

    harness.click("thread-row-0").unwrap();

    assert_eq!(selected_cwd(&harness), Path::new("/alpha"));
}

#[test]
fn the_new_thread_shortcut_starts_a_thread_in_the_selected_threads_project() {
    let mut harness = DesktopHarnessBuilder::default()
        .agent("/alpha", replying("alpha-session", "hello"))
        .agent("/beta", replying("beta-session", "hello"))
        .agent("/beta", replying("second-beta-session", "hello"))
        .build();
    harness.click("project-beta-new-thread").unwrap();

    harness.press("secondary-n");

    let threads = harness.threads();
    let cwds: Vec<Option<PathBuf>> =
        threads.threads().iter().map(|t| t.target().cwd().map(Path::to_path_buf)).collect();
    assert_eq!(cwds, [Some("/beta".into()), Some("/beta".into())]);
}

#[test]
fn the_new_thread_shortcut_uses_the_first_project_when_no_thread_is_selected() {
    let mut harness = alpha_and_beta();

    harness.press("secondary-n");

    assert_eq!(selected_cwd(&harness), Path::new("/alpha"));
}

#[test]
fn a_live_threads_age_counts_from_when_the_app_clock_says_it_opened() {
    let (mut harness, _) = DesktopHarnessBuilder::default()
        .now("2031-06-01T12:00:00Z")
        .agent("/alpha", replying("alpha-session", "hello"))
        .start("/alpha");

    assert!(harness.bounds("thread-row-0-age-now").is_some(), "a thread that just opened is `now` old");
}

fn alpha_and_beta() -> DesktopHarness {
    DesktopHarnessBuilder::default()
        .agent("/alpha", replying("alpha-session", "hello"))
        .agent("/beta", replying("beta-session", "hello"))
        .build()
}

fn selected_cwd(harness: &DesktopHarness) -> PathBuf {
    let thread = harness.selected_thread().expect("a thread is selected");
    thread.target().cwd().map(Path::to_path_buf).expect("the thread works locally")
}
