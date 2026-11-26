#[cfg(target_os = "ios")]
mod app;
#[cfg(target_os = "ios")]
use rouge::{App, AppError};

#[cfg(target_os = "ios")]
fn main() -> Result<(), AppError> {
    app::run(App::new())
}

#[cfg(not(target_os = "ios"))]
fn main() {
    eprintln!("Run Foxfire with `cargo rouge run ios` or `cargo rouge run android`.");
}
