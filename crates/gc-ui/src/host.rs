//! Desktop IPC and browser scheduling for the Rust WebView.
use crate::dom::{get, invoke};
use js_sys::{Error, Promise};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

pub fn now() -> f64 {
    web_sys::window().unwrap().performance().unwrap().now()
}
pub async fn resolved(value: JsValue) -> Result<JsValue, JsValue> {
    JsFuture::from(Promise::resolve(&value)).await
}
pub async fn command(name: &str, args: &JsValue) -> Result<JsValue, JsValue> {
    let window = web_sys::window().ok_or_else(|| Error::new("WebView window unavailable"))?;
    let api = get(&get(&window, "__TAURI__"), "core");
    resolved(invoke(&api, "invoke", &[name.into(), args.clone()])?).await
}
pub async fn sleep(ms: f64) -> Result<(), JsValue> {
    let p = Promise::new(&mut |resolve, reject| {
        if let Err(e) = web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                &resolve,
                ms.max(0.).round() as i32,
            )
        {
            let _ = reject.call1(&JsValue::UNDEFINED, &e);
        }
    });
    JsFuture::from(p).await.map(|_| ())
}
