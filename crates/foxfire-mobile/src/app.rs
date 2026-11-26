use foxfire_shell::relay;
use rouge::{App, AppError, WindowOptions};
use rouge_widgets::bundled_fonts;
use rustls::crypto::ring;
use tokio::runtime::Runtime;

pub(crate) fn run(app: App) -> Result<(), AppError> {
    let _ = ring::default_provider().install_default();
    let runtime = Runtime::new().expect("could not start the relay runtime");
    let localhost = if cfg!(target_os = "android") { "ws://10.0.2.2:7777" } else { "ws://127.0.0.1:7777" };
    let url = option_env!("FOXFIRE_RELAY_URL").unwrap_or(localhost);
    let name = option_env!("FOXFIRE_PARTICIPANT").unwrap_or("You");
    app.tokio(runtime.handle().clone())
        .fonts(bundled_fonts())
        .window(WindowOptions::default().title("Foxfire"), move || relay::connect(url, name))
        .run()
}
