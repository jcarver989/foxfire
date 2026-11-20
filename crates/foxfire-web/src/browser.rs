use crate::web_view;
use foxfire_ui::backend::Project;
use foxfire_ui::memory::MemoryBackend;
use foxfire_ui::platform::ChatPlatform;
use rouge::platform::open_url;
use rouge::{App, WindowOptions};
use rouge_widgets::bundled_fonts;
use std::rc::Rc;
use wasm_bindgen::prelude::{JsValue, wasm_bindgen};

struct BrowserPlatform;

impl ChatPlatform for BrowserPlatform {
    fn open(&self, target: &str) {
        if target.starts_with("https://") || target.starts_with("http://") {
            let _ = open_url(target);
        }
    }
}

/// Mounts the self-contained demo in the given browser canvas.
#[wasm_bindgen]
pub fn mount(canvas: &str) -> Result<(), JsValue> {
    let backend = MemoryBackend::new(vec![Project { id: "demo".to_owned(), name: "Demo project".to_owned() }]);
    App::new()
        .fonts(bundled_fonts())
        .window(WindowOptions::default().title("Foxfire · In-memory demo").canvas(canvas), move || {
            web_view(Rc::new(backend), Rc::new(BrowserPlatform))
        })
        .run()
        .map_err(|error| JsValue::from_str(&error.to_string()))
}
