use crate::relay_view;
use foxfire_acp::chat::Participant;
use foxfire_acp::websocket::Endpoint;
use foxfire_shell::hosted::demo_view;
use foxfire_ui::platform::ChatPlatform;
use rouge::core::View;
use rouge::persist::app_storage;
use rouge::platform::open_url;
use rouge::{App, WindowOptions};
use rouge_widgets::bundled_fonts;
use std::rc::Rc;
use wasm_bindgen::prelude::{JsValue, wasm_bindgen};

/// Mounts the self-contained demo in the given browser canvas.
#[wasm_bindgen]
pub fn mount(canvas: &str) -> Result<(), JsValue> {
    run(canvas, "Foxfire · In-memory demo", || demo_view(app_storage("foxfire")))
}

/// Mounts the live session of a `foxfire-relay` in the given browser canvas.
#[wasm_bindgen]
pub fn connect(canvas: &str, url: &str, name: &str) -> Result<(), JsValue> {
    let endpoint = Endpoint::new(url);
    let participant = Participant { id: name.to_lowercase(), name: name.to_owned() };
    run(canvas, "Foxfire", move || {
        relay_view(endpoint.clone(), participant.clone(), Rc::new(BrowserPlatform), app_storage("foxfire"))
    })
}

struct BrowserPlatform;

impl ChatPlatform for BrowserPlatform {
    fn open(&self, target: &str) {
        if target.starts_with("https://") || target.starts_with("http://") {
            let _ = open_url(target);
        }
    }
}

fn run<T: View>(canvas: &str, title: &str, view: impl Fn() -> T + 'static) -> Result<(), JsValue> {
    App::new()
        .fonts(bundled_fonts())
        .window(WindowOptions::default().title(title).canvas(canvas), view)
        .run()
        .map_err(|error| JsValue::from_str(&error.to_string()))
}
