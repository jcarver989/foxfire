//! Android entry point for the Foxfire demo. The iOS app is this package's binary.
#![cfg(target_os = "android")]

use foxfire_ui::hosted::demo_view;
use rouge::persist::app_storage;
use rouge::platform::AndroidApp;
use rouge::{App, WindowOptions};
use rouge_widgets::bundled_fonts;

#[allow(unsafe_code, reason = "NativeActivity looks up android_main by its unmangled name.")]
#[unsafe(no_mangle)]
extern "Rust" fn android_main(android: AndroidApp) {
    let run = App::new()
        .android(android)
        .fonts(bundled_fonts())
        .window(WindowOptions::default().title("Foxfire"), || demo_view(app_storage("foxfire")))
        .run();
    if let Err(error) = run {
        eprintln!("foxfire: {error}");
    }
}
