use std::path::Path;
use std::process::Command;

/// The branch checked out in `repo`, or the short commit when `HEAD` is detached; `None` outside a repository.
pub(crate) fn current_ref(repo: &Path) -> Option<String> {
    git_text(repo, &["branch", "--show-current"]).or_else(|| git_text(repo, &["rev-parse", "--short", "HEAD"]))
}

fn git_text(repo: &Path, args: &[&str]) -> Option<String> {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|text| !text.is_empty())
}
