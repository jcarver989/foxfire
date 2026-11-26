use foxfire_shell::relay;
use rouge::{App, WindowOptions};
use rouge_widgets::bundled_fonts;
use wasm_bindgen::prelude::{JsValue, wasm_bindgen};

/// Mounts the live session of a `foxfire-relay` in the given browser canvas.
#[wasm_bindgen]
pub fn connect(canvas: &str, url: &str, name: &str) -> Result<(), JsValue> {
    let (url, name) = (url.to_owned(), name.to_owned());
    App::new()
        .fonts(bundled_fonts())
        .window(WindowOptions::default().title("Foxfire").canvas(canvas), move || relay::connect(&url, &name))
        .run()
        .map_err(|error| JsValue::from_str(&error.to_string()))
}
