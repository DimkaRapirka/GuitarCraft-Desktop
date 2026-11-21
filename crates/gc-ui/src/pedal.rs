use crate::{dom::*, knob::knob};
use js_sys::{Array, Function, Object};
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::Element;
fn ink(color: &str) -> &'static str {
    let rgb = u32::from_str_radix(color.trim_start_matches('#'), 16).unwrap_or(0);
    if (rgb >> 16) as f64 * 0.299 + ((rgb >> 8) & 255) as f64 * 0.587 + (rgb & 255) as f64 * 0.114
        > 150.
    {
        "#1a1a1a"
    } else {
        "#f5f2ea"
    }
}
#[wasm_bindgen(js_name=pedalVisual)]
pub fn pedal_visual(_app: &JsValue, def: &JsValue, params: &JsValue, opts: &JsValue) -> Element {
    let size = get(opts, "size").as_string().unwrap_or_else(|| "lg".into());
    let uid = get(opts, "uid")
        .as_string()
        .unwrap_or_else(|| string(def, "id"));
    let on = get(opts, "on").as_bool().unwrap_or(true);
    let params_list = get(def, "params");
    let controls = if Array::is_array(&params_list) {
        Array::from(&params_list)
    } else {
        Array::new()
    };
    let count = controls.length();
    let model = string(def, "id");
    let art = string(def, "artwork");
    let illustrated = if art.is_empty() { "" } else { " illustrated" };
    let root = make(
        "div",
        &format!(
            "pedal-real {size} {}{illustrated}",
            if on { "on" } else { "off" }
        ),
    );
    let color = get(def, "color")
        .as_string()
        .unwrap_or_else(|| "#777".into());
    style(&root, "--c", &color);
    style(&root, "--ink", ink(&color));
    root.set_attribute("data-pedal-vis", &uid).unwrap();
    root.set_attribute("data-model", &model).unwrap();
    for class in ["pr-enc", "pr-tint", "pr-sheen"] {
        child(&root, "div", class, "");
    }
    if size != "thumb" {
        match crate::wear::texture(&model) {
            Ok(texture) => {
                let wear = child(&root, "div", "pr-wear", "");
                wear.set_attribute("aria-hidden", "true").unwrap();
                style(&wear, "background-image", &format!(r#"url("{texture}")"#));
            }
            Err(error) => web_sys::console::warn_1(&error),
        }
    }
    let face = child(&root, "div", "pr-face", "");
    let knobs = child(&face, "div", &format!("pr-knobs n{count}"), "");
    if size != "thumb" {
        for param in controls.iter() {
            let key = string(&param, "id");
            let options = Object::new();
            set(&options, "param", &param);
            set(
                &options,
                "value",
                &num(params, &key, num(&param, "def", 0.5)).into(),
            );
            let width = match (size.as_str(), count > 3) {
                ("lg", true) => 40,
                ("lg", false) => 47,
                (_, true) => 32,
                (_, false) => 36,
            };
            set(&options, "size", &width.into());
            set(&options, "style", &"hw".into());
            set(&options, "id", &format!("{uid}.{key}").into());
            if let Ok(change) = get(opts, "onChange").dyn_into::<Function>() {
                let cb = Closure::wrap(Box::new(move |val: f64, done: bool| {
                    if let Err(e) = change.call3(
                        &JsValue::UNDEFINED,
                        &key.clone().into(),
                        &val.into(),
                        &done.into(),
                    ) {
                        web_sys::console::error_1(&e);
                    }
                }) as Box<dyn FnMut(f64, bool)>)
                .into_js_value();
                set(&options, "onChange", &cb);
            }
            knobs.append_child(&knob(&options)).unwrap();
        }
    }
    if !art.is_empty() {
        let img = child(&face, "img", "pr-print", "");
        img.set_attribute("src", &art).unwrap();
        img.set_attribute("alt", "").unwrap();
        img.set_attribute("draggable", "false").unwrap();
    }
    child(&face, "div", "pr-led", "");
    if size == "thumb" {
        child(&face, "i", "pr-switch", "");
    } else {
        let name = string(def, "name");
        child(&face, "div", "pr-name", &name);
        child(&face, "div", "pr-type", &string(def, "type"));
        let switch = child(&face, "button", "pr-switch", "");
        switch
            .set_attribute(
                "aria-label",
                &crate::i18n::message("Включить или выключить {}", &[&name]),
            )
            .unwrap();
        switch
            .set_attribute("aria-pressed", if on { "true" } else { "false" })
            .unwrap();
        crate::editor::attr(&switch, "title", "Вкл / выкл");
        let toggle = get(opts, "onToggle").dyn_into::<Function>().ok();
        listen(&switch, "click", move |ev| {
            ev.stop_propagation();
            if let Some(f) = &toggle {
                if let Err(e) = f.call0(&JsValue::UNDEFINED) {
                    web_sys::console::error_1(&e);
                }
            }
        });
    }
    root
}
