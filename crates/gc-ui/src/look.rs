//! Pedal looks: enclosure size, paint, finish, knobs, LED and label. A look is layered: the
//! model's default (`look` in pedals.json or a built pedal), the user's default for that model
//! (settings `pedalLooks`), then the pedal on the board (`look` on its chain block).
use crate::{
    dom::*,
    editor::{attr, button, on, report},
    js, read, store,
};
use serde_json::{json, Value};
use std::cell::Cell;
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::Element;

/// URL the WebView can load for a user file.
pub(crate) fn file_src(path: &str) -> String {
    invoke(
        &get(&get(&web_sys::window().unwrap().into(), "__TAURI__"), "core"),
        "convertFileSrc",
        &[path.into()],
    )
    .ok()
    .and_then(|v| v.as_string())
    .unwrap_or_default()
}

thread_local! {static OPEN:Cell<bool>=const{Cell::new(false)};}

pub(crate) const SIZES: [(&str, &str); 4] = [
    ("mini", "Мини"),
    ("standard", "Стандарт"),
    ("wide", "Широкая"),
    ("big", "Большая"),
];
pub(crate) const FINISHES: [(&str, &str); 5] = [
    ("paint", "Краска"),
    ("metal", "Металлик"),
    ("sparkle", "Блёстки"),
    ("matte", "Матовая"),
    ("hammer", "Хаммертон"),
];
const KNOBS: [(&str, &str); 4] = [
    ("black", "Чёрные"),
    ("cream", "Кремовые"),
    ("chrome", "Хром"),
    ("color", "В цвет корпуса"),
];
const LEDS: [&str; 6] = ["#ff3b2f", "#3bff6a", "#3bb4ff", "#ffd23b", "#c04bff", "#ffffff"];
const COLORS: [&str; 14] = [
    "#c62d2d", "#e08a2c", "#e8c33a", "#b2cb46", "#3f8f6f", "#2f7d4d", "#4aa3c9", "#3d5aa8",
    "#5b3fa0", "#d14d8a", "#e9e9e4", "#9aa1a8", "#8b6b4a", "#2b2b2e",
];

pub(crate) fn is_open() -> bool {
    OPEN.with(Cell::get)
}
fn merge(into: &mut Value, from: &Value) {
    if let Some(o) = from.as_object() {
        for (k, v) in o {
            if !v.is_null() && v != "" {
                into[k] = v.clone();
            }
        }
    }
}
fn value(v: JsValue) -> Value {
    if v.is_object() {
        read(v).unwrap_or(Value::Null)
    } else {
        Value::Null
    }
}
/// The user's default look for a model (settings `pedalLooks`).
fn user_default(app: &JsValue, model: &str) -> Value {
    value(get(&get(&get(app, "settings"), "pedalLooks"), model))
}
/// The look a pedal on the board shows.
pub(crate) fn look_of(app: &JsValue, def: &JsValue, block: &Value) -> Value {
    let mut l = json!({});
    merge(&mut l, &value(get(def, "look")));
    merge(&mut l, &user_default(app, &string(def, "id")));
    merge(&mut l, &block["look"]);
    l
}
fn edit_look(app: &JsValue, uid: &str, change: impl FnOnce(&mut Value)) -> Result<(), JsValue> {
    let uid = uid.to_owned();
    store::edit(app, "Вид педали", None, |s| {
        if let Some(b) = s["chain"]
            .as_array_mut()
            .and_then(|bs| bs.iter_mut().find(|b| b["uid"] == uid.as_str()))
        {
            if !b["look"].is_object() {
                b["look"] = json!({});
            }
            change(&mut b["look"]);
            if b["look"].as_object().is_some_and(|o| o.is_empty()) {
                b.as_object_mut().unwrap().remove("look");
            }
        }
    })?;
    invoke(app, "renderStage", &[])?;
    invoke(app, "renderChain", &[]).map(drop)
}
fn set_key(app: &JsValue, uid: &str, key: &'static str, v: Value) -> Result<(), JsValue> {
    edit_look(app, uid, |l| {
        if v.is_null() {
            l.as_object_mut().unwrap().remove(key);
        } else {
            l[key] = v;
        }
    })
}
/// "Вид…" button for the board info bar.
pub(crate) fn toggle_button(app: &JsValue, bar: &Element) {
    let b = button(
        bar,
        if is_open() { "look-btn on" } else { "look-btn" },
        "Размер, цвет, покрытие, ручки и надпись этой педали",
        "🎨 Вид…",
    );
    on(&b, "click", app, |a, _| {
        OPEN.with(|o| o.set(!o.get()));
        invoke(a, "renderStage", &[]).map(drop)
    });
}
/// A hand-written strip of masking tape that names an area of the bench.
fn tape(root: &Element, text: &str) {
    child(root, "span", "ws-tape", text);
}
fn area(root: &Element, class: &str, title: &str) -> Element {
    let a = child(root, "section", &format!("ws-area {class}"), "");
    tape(&a, title);
    child(&a, "div", "ws-items", "")
}
/// Spray the pedal with `color`: a puff of paint over it, then the colour is stored.
fn spray(app: &JsValue, uid: &str, color: String) -> Result<(), JsValue> {
    if let Some(el) = doc().query_selector(&format!("[data-pedal-vis=\"{uid}\"]"))? {
        let mist = child(&el, "div", "spray-mist", "");
        style(&mist, "--paint", &color);
        style(&el, "--c", &color);
    }
    let (app, uid) = (app.clone(), uid.to_owned());
    wasm_bindgen_futures::spawn_local(async move {
        let _ = crate::host::sleep(520.).await;
        report(set_key(&app, &uid, "color", json!(color)));
    });
    Ok(())
}
/// The look editor for the selected pedal: a workbench with real tools, under the board.
pub(crate) fn panel(app: &JsValue, root: &Element, uid: &str, def: &JsValue, block: &Value) -> Result<(), JsValue> {
    if !is_open() {
        return Ok(());
    }
    let look = look_of(app, def, block);
    let model = string(def, "id");
    let name = string(def, "name");
    let p = child(root, "div", "look-panel workshop", "");
    let head = child(&p, "div", "ws-head", "");
    child(&head, "b", "ws-title", &crate::i18n::message("Мастерская · {}", &[&name]));
    on(&button(&head, "ws-close", "Закрыть мастерскую", "✕"), "click", app, |a, _| {
        OPEN.with(|o| o.set(false));
        invoke(a, "renderStage", &[]).map(drop)
    });
    let grid = child(&p, "div", "ws-grid", "");
    let color = look["color"]
        .as_str()
        .map(String::from)
        .unwrap_or_else(|| string(def, "color"));

    // enclosures: blank, unpainted boxes of each size
    let size = look["size"].as_str().unwrap_or("standard");
    let boxes = area(&grid, "ws-sizes", "Корпус");
    for &(id, label) in &SIZES {
        let b = button(&boxes, if size == id { "ws-blank on" } else { "ws-blank" }, label, "");
        child(&b, "i", &format!("blank b-{id}"), "");
        child(&b, "span", "ws-tag", label);
        let uid = uid.to_owned();
        on(&b, "click", app, move |a, _| set_key(a, &uid, "size", json!(id)));
    }

    // paint: a row of spray cans, the last one mixes any colour
    let cans = area(&grid, "ws-paint", "Краска");
    for &c in &COLORS {
        let b = button(&cans, if color.eq_ignore_ascii_case(c) { "ws-can on" } else { "ws-can" }, "", "");
        style(&b, "--paint", c);
        attr(&b, "aria-label", c);
        child(&b, "i", "can-tint", "");
        let uid = uid.to_owned();
        on(&b, "click", app, move |a, _| spray(a, &uid, c.to_owned()));
    }
    let mix = child(&cans, "label", "ws-can mix", "");
    attr(&mix, "title", "Смешать свой цвет");
    style(&mix, "--paint", &color);
    child(&mix, "i", "can-tint", "");
    child(&mix, "em", "", "?");
    let pick = child(&mix, "input", "", "");
    attr(&pick, "type", "color");
    set(&pick, "value", &color.clone().into());
    let id = uid.to_owned();
    on(&pick, "change", app, move |a, e| spray(a, &id, string(&e.current_target().unwrap(), "value")));

    // finish: sample plates sprayed in the current colour
    let finish = look["finish"].as_str().unwrap_or("paint");
    let plates = area(&grid, "ws-finish", "Покрытие");
    for &(id, label) in &FINISHES {
        let b = button(&plates, if finish == id { "ws-plate on" } else { "ws-plate" }, label, "");
        style(&b, "--paint", &color);
        style(&b, "--plate", &format!("url(assets/gear/workshop/finish_{id}.png)"));
        child(&b, "i", "plate-tint", "");
        child(&b, "span", "ws-tag", label);
        let uid = uid.to_owned();
        on(&b, "click", app, move |a, _| set_key(a, &uid, "finish", json!(id)));
    }

    // knobs and LEDs: parts trays
    let knob = look["knob"].as_str().unwrap_or("black");
    let tray = area(&grid, "ws-knobs", "Ручки");
    for &(id, label) in &KNOBS {
        let b = button(&tray, if knob == id { "ws-knob on" } else { "ws-knob" }, label, "");
        style(&b, "--paint", &color);
        child(&b, "i", &format!("part k-{id}"), "");
        let uid = uid.to_owned();
        on(&b, "click", app, move |a, _| set_key(a, &uid, "knob", json!(id)));
    }
    let led = look["led"].as_str().unwrap_or("#ff3b2f").to_owned();
    let leds = area(&grid, "ws-leds", "Светодиод");
    for &c in &LEDS {
        let b = button(&leds, if led.eq_ignore_ascii_case(c) { "ws-led on" } else { "ws-led" }, "", "");
        style(&b, "--led", c);
        attr(&b, "aria-label", c);
        child(&b, "i", "", "");
        let uid = uid.to_owned();
        on(&b, "click", app, move |a, _| set_key(a, &uid, "led", json!(c)));
    }

    // artwork: the model's print, the user's own picture, or none
    let print = look["print"].as_str().unwrap_or_default().to_owned();
    let fit = look["printFit"].as_str().unwrap_or("print").to_owned();
    let art = area(&grid, "ws-art", "Рисунок");
    let sheet = child(&art, "div", "ws-decal", "");
    let shown = if print == "none" {
        String::new()
    } else if !print.is_empty() {
        file_src(&print)
    } else {
        string(def, "artwork")
    };
    if shown.is_empty() {
        child(&sheet, "span", "ws-tag", "без рисунка");
    } else {
        let img = child(&sheet, "img", "", "");
        attr(&img, "src", &shown);
        attr(&img, "alt", "");
    }
    let tools = child(&art, "div", "ws-art-tools", "");
    let id = uid.to_owned();
    on(&button(&tools, "ws-btn", "PNG, JPG или WebP: он напечатается на корпусе", "Загрузить картинку…"), "click", app, move |a, _| {
        let (a, id) = (a.clone(), id.clone());
        wasm_bindgen_futures::spawn_local(async move {
            let r: Result<(), JsValue> = async {
                let file = crate::host::command(
                    "file_open",
                    &js(&json!({"title":crate::i18n::text("Рисунок для педали"),"filters":[{"name":"Image","extensions":["png","jpg","jpeg","webp"]}]}))?,
                )
                .await?;
                let Some(file) = file.as_string() else { return Ok(()) };
                let dest = crate::host::command("import_to", &js(&json!({"file":file,"kind":"pedals"}))?)
                    .await?
                    .as_string()
                    .unwrap_or_default();
                set_key(&a, &id, "print", json!(dest))
            }
            .await;
            if let Err(e) = r {
                let _ = invoke(&a, "toast", &[e.clone()]);
                report(Err(e));
            }
        });
        Ok(())
    });
    for (v, label, title) in [("print", "Наклейкой", "Рисунок посередине корпуса, как шелкография"), ("full", "Во весь корпус", "Рисунок закрывает весь корпус вместо краски")] {
        let b = button(&tools, if fit == v { "ws-btn on" } else { "ws-btn" }, title, label);
        let id = uid.to_owned();
        on(&b, "click", app, move |a, _| set_key(a, &id, "printFit", json!(v)));
    }
    let id = uid.to_owned();
    on(&button(&tools, if print == "none" { "ws-btn on" } else { "ws-btn" }, "", "Без рисунка"), "click", app, move |a, _| {
        set_key(a, &id, "print", json!("none"))
    });
    if !print.is_empty() {
        let id = uid.to_owned();
        on(&button(&tools, "ws-btn", "Вернуть рисунок модели", "Рисунок модели"), "click", app, move |a, _| {
            set_key(a, &id, "print", Value::Null)
        });
    }
    let style_in = child(&tools, "input", "ws-style", "");
    attr(&style_in, "placeholder", "Стиль рисунка: «неоновый кит», «старая гравюра с волком»…");
    let pr = button(&tools, "ws-btn", "Промпт для генератора картинок: вставьте его в ChatGPT, Midjourney или другой", "Скопировать промпт");
    on(&pr, "click", app, |a, e| {
        let el: Element = e.current_target().unwrap().unchecked_into();
        let style_text = el
            .previous_element_sibling()
            .map(|i| string(&i, "value"))
            .unwrap_or_default();
        let text = include_str!("pedal-prompt.txt").trim_end().replace(
            "{style}",
            if style_text.trim().is_empty() { "your style: colours, motif, era" } else { style_text.trim() },
        );
        let (a, args) = (a.clone(), js(&json!({"text":text}))?);
        wasm_bindgen_futures::spawn_local(async move {
            report(
                async {
                    crate::host::command("clipboard_write", &args).await?;
                    invoke(&a, "toast", &[crate::i18n::text("Промпт скопирован: сгенерируйте картинку и загрузите её сюда").into()]).map(drop)
                }
                .await,
            );
        });
        Ok(())
    });

    // label maker tape
    let label = area(&grid, "ws-label", "Надпись");
    let strip = child(&label, "div", "ws-dymo", "");
    let input = child(&strip, "input", "", "");
    attr(&input, "placeholder", &name);
    attr(&input, "maxlength", "24");
    attr(&input, "spellcheck", "false");
    set(&input, "value", &look["name"].as_str().unwrap_or_default().into());
    let id = uid.to_owned();
    on(&input, "change", app, move |a, e| {
        let v = string(&e.current_target().unwrap(), "value").trim().to_owned();
        set_key(a, &id, "name", if v.is_empty() { Value::Null } else { json!(v) })
    });
    let foot = child(&p, "div", "ws-foot", "");
    let id = uid.to_owned();
    on(
        &button(&foot, "", "Убрать изменения вида у этой педали", "Как у модели"),
        "click",
        app,
        move |a, _| edit_look(a, &id, |l| *l = json!({})),
    );
    let saved = !user_default(app, &model).is_null();
    let b = button(
        &foot,
        "",
        "Новые экземпляры этой модели будут выглядеть так же",
        &crate::i18n::message("Сделать видом всех «{}»", &[&name]),
    );
    let id = uid.to_owned();
    let m = model.clone();
    on(&b, "click", app, move |a, _| {
        let look = look_of(a, &invoke(&get(a, "catalog"), "get", &[m.clone().into()])?, &store::state(a)?["chain"]
            .as_array()
            .and_then(|bs| bs.iter().find(|b| b["uid"] == id.as_str()).cloned())
            .unwrap_or_default());
        save_default(a, &m, look)
    });
    if saved {
        let m = model.clone();
        on(
            &button(&foot, "ghost", "Вернуть заводской вид модели", "Сбросить вид модели"),
            "click",
            app,
            move |a, _| save_default(a, &m, Value::Null),
        );
    }
    Ok(())
}
fn save_default(app: &JsValue, model: &str, look: Value) -> Result<(), JsValue> {
    let mut all = value(get(&get(app, "settings"), "pedalLooks"));
    if !all.is_object() {
        all = json!({});
    }
    if look.is_null() {
        all.as_object_mut().unwrap().remove(model);
    } else {
        all[model] = look;
    }
    let patch = js(&json!({ "pedalLooks": all }))?;
    let app = app.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let r = async {
            crate::preferences::save_settings(app.clone(), patch).await?;
            invoke(&app, "toast", &[crate::i18n::text("Вид модели сохранён").into()])?;
            invoke(&app, "renderStage", &[]).map(drop)
        }
        .await;
        report(r);
    });
    Ok(())
}
/// Classes and CSS variables a look puts on a `.pedal-real`.
pub(crate) fn apply(root: &Element, look: &JsValue, thumb: bool) {
    if !look.is_object() {
        return;
    }
    let list = root.class_list();
    let size = string(look, "size");
    if !thumb && matches!(size.as_str(), "mini" | "wide" | "big") {
        let _ = list.add_1(&format!("sz-{size}"));
    }
    for (key, prefix) in [("finish", "fin"), ("knob", "kn")] {
        let v = string(look, key);
        if !v.is_empty() && v.chars().all(|c| c.is_ascii_alphanumeric()) {
            let _ = list.add_1(&format!("{prefix}-{v}"));
        }
    }
    let led = string(look, "led");
    if led.starts_with('#') {
        style(root, "--led", &led);
    }
}
