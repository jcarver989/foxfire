use framer::Project;
use framer::github::Step;
use framer_presets::{Ci, Dist, Release, ReleasePlz, Rust, RustMonorepo, Rustfmt, Workspace};
use rouge_framer::{Android, Ios, PlayInternal, RougePublish, TestFlight};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut project = Project::new();
    project.apply(
        RustMonorepo::new()
            .rust(Rust::new("1.100"))
            .workspace(Workspace::new().repository("https://github.com/jcarver989/foxfire"))
            .rustfmt(
                Rustfmt::new().set("max_width", 120).set("newline_style", "Unix").set("use_small_heuristics", "Max"),
            )
            .ci(Ci::new().setup(
                Step::run("sudo apt-get update && sudo apt-get install --yes libfontconfig1-dev libxkbcommon-x11-dev mesa-vulkan-drivers")
                    .name("Install system libraries"),
            ))
            .release(
                Release::new()
                    .release_plz(
                        ReleasePlz::new()
                            .publish_to_crates_io()
                            .set("release", false)
                            .set("publish", false)
                            .set("semver_check", false),
                    )
                    .dist(
                        Dist::new()
                            .set("dependencies.apt.libfontconfig1-dev", "*")
                            .set("dependencies.apt.libxkbcommon-x11-dev", "*")
                            .set("github-custom-runners.aarch64-apple-darwin", "macos-26"),
                    ),
            ),
    );
    project.apply(
        RougePublish::new("dev.foxfire.app")
            .name("Foxfire")
            .package("foxfire-mobile")
            .icon("assets/app-icon.png")
            // Replace the placeholder with the App Store Connect Apple ID before publishing.
            .ios(Ios::new(TestFlight::new("0")).info("NSLocalNetworkUsageDescription", "Connect to your Foxfire relay on the local network."))
            .android(Android::new(PlayInternal).permission("android.permission.INTERNET")),
    );
    rouge_framer::run(&project)
}
