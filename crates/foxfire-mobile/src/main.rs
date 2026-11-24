#[cfg(target_os = "ios")]
use foxfire_ui::hosted::demo_view;
#[cfg(target_os = "ios")]
use rouge::persist::app_storage;
#[cfg(target_os = "ios")]
use rouge::{App, AppError, WindowOptions};
#[cfg(target_os = "ios")]
use rouge_widgets::bundled_fonts;

#[cfg(target_os = "ios")]
fn main() -> Result<(), AppError> {
    App::new()
        .fonts(bundled_fonts())
        .window(WindowOptions::default().title("Foxfire"), || demo_view(app_storage("foxfire")))
        .run()
}

#[cfg(not(target_os = "ios"))]
fn main() {
    eprintln!("Run the mobile demo with `just ios-sim` or `just android-emulator`.");
}
