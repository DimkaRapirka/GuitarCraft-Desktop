use js_sys::{Function, Reflect};
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::{Element, Event, EventTarget};

pub fn doc() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}
pub fn make(tag: &str, class: &str) -> Element {
    let e = doc().create_element(tag).unwrap();
    if !class.is_empty() {
        e.set_class_name(class)
    }
    e
}
pub fn child(root: &Element, tag: &str, class: &str, txt: &str) -> Element {
    let e = make(tag, class);
    if !txt.is_empty() {
        e.set_text_content(Some(crate::i18n::text(txt)));
    }
    root.append_child(&e).unwrap();
    e
}
pub fn get(o: &JsValue, key: &str) -> JsValue {
    Reflect::get(o, &key.into()).unwrap_or(JsValue::UNDEFINED)
}
pub fn set(o: &JsValue, key: &str, value: &JsValue) {
    Reflect::set(o, &key.into(), value).unwrap();
}
pub fn string(o: &JsValue, key: &str) -> String {
    get(o, key).as_string().unwrap_or_default()
}
pub fn num(o: &JsValue, key: &str, fallback: f64) -> f64 {
    get(o, key).as_f64().unwrap_or(fallback)
}
pub fn invoke(o: &JsValue, name: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
    let f = get(o, name).dyn_into::<Function>()?;
    let arr = js_sys::Array::new();
    for a in args {
        arr.push(a);
    }
    f.apply(o, &arr)
}
pub fn listen(target: &EventTarget, kind: &str, handler: impl FnMut(Event) + 'static) {
    let cb = Closure::wrap(Box::new(handler) as Box<dyn FnMut(Event)>).into_js_value();
    let opts = web_sys::AddEventListenerOptions::new();
    opts.set_passive(false);
    target
        .add_event_listener_with_callback_and_add_event_listener_options(
            kind,
            cb.unchecked_ref(),
            &opts,
        )
        .unwrap();
}
pub fn style(e: &Element, key: &str, value: &str) {
    if let Some(el) = e.dyn_ref::<web_sys::HtmlElement>() {
        el.style().set_property(key, value).unwrap();
    }
}
