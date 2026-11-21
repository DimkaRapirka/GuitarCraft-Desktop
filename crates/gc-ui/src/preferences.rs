//! Device, MIDI and UI preferences use the same document for the settings panel and RPC.
use crate::{dom::*, host, js};
use js_sys::{Object, Promise};
use serde_json::json;
use std::cell::RefCell;
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::{future_to_promise, JsFuture};

thread_local! {static SAVE:RefCell<Option<Promise>>=const{RefCell::new(None)};}
fn copy_fields(to: &JsValue, from: &JsValue) {
    if !from.is_object() {
        return;
    }
    for k in Object::keys(from.unchecked_ref::<Object>())
        .iter()
        .filter_map(|v| v.as_string())
    {
        set(to, &k, &get(from, &k));
    }
}
#[wasm_bindgen(js_name=loadSettings)]
pub async fn load_settings(app: JsValue) -> Result<JsValue, JsValue> {
    let s = js(
        &json!({"language":"en","inputMode":"di","inputDevice":"default","inputChannel":0,"outputDevice":"default","bufferSize":128,
        "tunerRef":440,"tunerMute":true,"bpm":120,"metronomeLevel":0.5,"mcpEnabled":false,"showAgentPanel":true}),
    )?;
    let saved = host::command("settings_load", &JsValue::UNDEFINED).await?;
    copy_fields(&s, &saved);
    set(&app, "settings", &s);
    Ok(s)
}
#[wasm_bindgen(js_name=saveSettings)]
pub async fn save_settings(app: JsValue, patch: JsValue) -> Result<JsValue, JsValue> {
    let settings = get(&app, "settings");
    copy_fields(&settings, &patch);
    // Snapshot before awaiting. Overlapping hardware events and UI edits reach disk in input order.
    let snapshot = js_sys::JSON::parse(&js_sys::JSON::stringify(&settings)?.as_string().unwrap())?;
    let args = Object::new();
    set(&args, "data", &snapshot);
    let prev = SAVE.with(|s| s.borrow().clone());
    let save = future_to_promise(async move {
        if let Some(prev) = prev {
            let _ = JsFuture::from(prev).await;
        }
        host::command("settings_save", &args).await
    });
    SAVE.with(|s| s.replace(Some(save.clone())));
    JsFuture::from(save).await?;
    if !get(&patch, "sceneBackgrounds").is_undefined() {
        crate::backgrounds::apply(&app).await?;
    }
    if get(&app, "onSettings").is_function() {
        invoke(&app, "onSettings", &[patch])?;
    }
    Ok(settings)
}
#[wasm_bindgen(js_name=applyAudio)]
pub async fn apply_audio(app: JsValue) -> Result<(), JsValue> {
    let s = get(&app, "settings");
    let e = get(&app, "engine");
    let cfg = Object::new();
    for (from, to) in [
        ("inputMode", "mode"),
        ("inputDevice", "input"),
        ("inputChannel", "channel"),
        ("outputDevice", "output"),
        ("outputPair", "outputPair"),
        ("mono", "mono"),
        ("bufferSize", "buffer"),
        ("sampleRate", "sampleRate"),
    ] {
        set(&cfg, to, &get(&s, from));
    }
    if let Err(error) = host::resolved(invoke(&e, "configure", &[cfg.into()])?).await {
        let why = get(&error, "message")
            .as_string()
            .or_else(|| error.as_string())
            .unwrap_or_default();
        invoke(
            &app,
            "toast",
            &[crate::i18n::message("Аудио: {}", &[&why]).into()],
        )?;
        if string(&s, "inputMode") == "live" {
            save_settings(app.clone(), js(&json!({"inputMode":"di"}))?).await?;
            let _ = host::resolved(invoke(&e, "configure", &[js(&json!({"mode":"di"}))?])?).await;
        }
    }
    let tuner = get(&app, "tuner");
    set(&tuner, "ref", &get(&s, "tunerRef"));
    set(&tuner, "mute", &get(&s, "tunerMute"));
    Ok(())
}
