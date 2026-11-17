use framer::Project;
use framer::github::Step;
use framer_presets::{Ci, Rust, RustMonorepo, Rustfmt};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut project = Project::new();
    project.apply(
        RustMonorepo::new()
            .rust(Rust::new("1.100"))
            .rustfmt(
                Rustfmt::new().set("max_width", 120).set("newline_style", "Unix").set("use_small_heuristics", "Max"),
            )
            .ci(Ci::new().setup(
                Step::run("sudo apt-get update && sudo apt-get install --yes libfontconfig1-dev libxkbcommon-x11-dev")
                    .name("Install system libraries"),
            )),
    );
    project.synth()
}
