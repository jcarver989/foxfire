use foxfire_shell::hosted::demo_view;
use rouge::persist::app_storage;
use rouge::{App, WindowOptions};
use rouge_widgets::bundled_fonts;
use wasm_bindgen::prelude::{JsValue, wasm_bindgen};

/// Mounts the self-contained demo in the given browser canvas.
#[wasm_bindgen]
pub fn mount(canvas: &str) -> Result<(), JsValue> {
    App::new()
        .fonts(bundled_fonts())
        .window(WindowOptions::default().title("Foxfire · In-memory demo").canvas(canvas), || {
            demo_view(app_storage("foxfire"))
        })
        .run()
        .map_err(|error| JsValue::from_str(&error.to_string()))
}
