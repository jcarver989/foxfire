//! Android entry point for Foxfire. The iOS app is this package's binary.
#![cfg(target_os = "android")]

mod app;

use rouge::App;
use rouge::platform::AndroidApp;

#[allow(unsafe_code, reason = "NativeActivity looks up android_main by its unmangled name.")]
#[unsafe(no_mangle)]
extern "Rust" fn android_main(android: AndroidApp) {
    if let Err(error) = app::run(App::new().android(android)) {
        eprintln!("foxfire: {error}");
    }
}
