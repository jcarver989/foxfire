use framer::Project;
use framer_presets::{Rust, RustMonorepo};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut project = Project::new();
    project.apply(RustMonorepo::new().rust(Rust::new("1.100")));
    project.synth()
}
