use framer::Project;
use framer::github::Step;
use framer_presets::{Ci, Dist, Release, Rust, RustMonorepo, Rustfmt, Workspace};
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
                Release::new().dist(
                    Dist::new()
                        .set("dependencies.apt.libfontconfig1-dev", "*")
                        .set("dependencies.apt.libxkbcommon-x11-dev", "*"),
                ),
            ),
    );
    project.synth()
}
