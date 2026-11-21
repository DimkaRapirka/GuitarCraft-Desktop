use crate::{
    browser::open_catalog,
    desktop::{app, call, element, menu, menu_item},
    dom::*,
    editor::{attr, button, on},
    js, store,
};
use gc_model::{
    catalog::{list, text},
    EQUIPMENT,
};
use serde_json::{json, Value};
use std::{cell::Cell, rc::Rc};
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::{Element, HtmlElement, KeyboardEvent, PointerEvent};
struct Info {
    name: String,
    sub: String,
    color: String,
    on: bool,
    art: String,
}
fn info(b: &Value, s: &Value) -> Result<Info, JsValue> {
    let k = text(b, "kind");
    let (name, sub, color, art, on) = match k {
        "gate" => (
            "Gate".into(),
            format!(
                "{:.0} дБ",
                s["input"]["gate"]["threshold"].as_f64().unwrap_or(-62.)
            ),
            "#666".into(),
            String::new(),
            s["input"]["gate"]["on"] == true,
        ),
        "pedal" => {
            let d = invoke(&get(&app(), "catalog"), "get", &[text(b, "pedal").into()])?;
            (
                get(&d, "name")
                    .as_string()
                    .unwrap_or_else(|| text(b, "pedal").into()),
                string(&d, "type"),
                get(&d, "color")
                    .as_string()
                    .unwrap_or_else(|| "#555".into()),
                string(&d, "artwork"),
                b["on"] != false,
            )
        }
        "amp" => {
            let ch = &EQUIPMENT["channels"][text(&s["amp"], "channel")];
            (
                text(ch, "name").into(),
                "Усилитель".into(),
                text(ch, "color").into(),
                String::new(),
                s["amp"]["on"] != false,
            )
        }
        "cab" => {
            let c = &s["cab"];
            (
                if c["mode"] == "ir" {
                    c["irName"].as_str().unwrap_or("IR").into()
                } else {
                    text(c, "model").replacen('-', " ", 1).to_uppercase()
                },
                "Кабинет".into(),
                "#8a7a66".into(),
                String::new(),
                c["on"] != false,
            )
        }
        _ => {
            let (name, sub, color) = match k {
                "eq" => ("EQ", "Эквалайзер".into(), "#6d8fb3"),
                "delay" => {
                    let sync = text(&s["delay"], "sync");
                    (
                        "Delay",
                        if !sync.is_empty() && sync != "off" {
                            format!("Эхо · {sync}")
                        } else {
                            "Эхо".into()
                        },
                        "#6aa38a",
                    )
                }
                _ => ("Reverb", "Пространство".into(), "#8f7ab5"),
            };
            (
                name.into(),
                sub,
                color.into(),
                String::new(),
                s[k]["on"] != false,
            )
        }
    };
    Ok(Info {
        name,
        sub,
        color,
        art,
        on,
    })
}
fn thumb(root: &Element, kind: &str, color: &str, art: &str) {
    let el = child(root, "i", &format!("th th-{kind}"), "");
    style(&el, "--c", color);
    if !art.is_empty() {
        let img = child(&el, "img", "", "");
        attr(&img, "src", art);
        attr(&img, "alt", "");
        attr(&img, "draggable", "false");
    }
}
#[derive(Clone, Copy)]
struct Drag {
    pointer: i32,
    start_x: i32,
    start_y: i32,
    moved: bool,
}

fn drop_slot(chain: &Element, x: i32, y: i32) -> Result<Option<Element>, JsValue> {
    let rect = chain.get_bounding_client_rect();
    if f64::from(y) < rect.top() - 12. || f64::from(y) > rect.bottom() + 12. {
        return Ok(None);
    }
    let x = f64::from(x);
    let gaps = chain.query_selector_all("[data-chain-slot]")?;
    Ok((0..gaps.length())
        .filter_map(|index| gaps.get(index))
        .map(|node| node.unchecked_into::<Element>())
        .min_by(|left, right| {
            let distance = |el: &Element| {
                let bounds = el.get_bounding_client_rect();
                (x - (bounds.left() + bounds.right()) / 2.).abs()
            };
            distance(left).total_cmp(&distance(right))
        }))
}

fn draggable(el: &Element, uid: &str) {
    let gesture = Rc::new(Cell::<Option<Drag>>::new(None));
    let down = gesture.clone();
    on(el, "pointerdown", &app(), move |_, event| {
        let e: PointerEvent = event.unchecked_into();
        let block: Element = e.current_target().unwrap().unchecked_into();
        let on_button = e
            .target()
            .and_then(|node| node.dyn_into::<Element>().ok())
            .and_then(|node| node.closest("button").ok().flatten())
            .is_some();
        if e.button() == 0 && !on_button {
            let pointer = e.pointer_id();
            down.set(Some(Drag {
                pointer,
                start_x: e.client_x(),
                start_y: e.client_y(),
                moved: false,
            }));
            let _ = block.set_pointer_capture(pointer);
        }
        Ok(())
    });
    for kind in [
        "pointermove",
        "pointerup",
        "pointercancel",
        "lostpointercapture",
    ] {
        let gesture = gesture.clone();
        let uid = uid.to_owned();
        on(el, kind, &app(), move |a, event| {
            let e: PointerEvent = event.unchecked_into();
            let Some(mut drag) = gesture
                .get()
                .filter(|state| state.pointer == e.pointer_id())
            else {
                return Ok(());
            };
            let block: Element = e.current_target().unwrap().unchecked_into();
            let Some(chain) = block.parent_element() else {
                return Ok(());
            };
            let previous = chain.query_selector(".drop-before")?;
            if kind == "pointermove" {
                if !drag.moved
                    && (e.client_x() - drag.start_x)
                        .abs()
                        .max((e.client_y() - drag.start_y).abs())
                        < 6
                {
                    return Ok(());
                }
                drag.moved = true;
                gesture.set(Some(drag));
                e.prevent_default();
                block.class_list().add_1("dragging")?;
                let next = drop_slot(&chain, e.client_x(), e.client_y())?;
                if previous != next {
                    if let Some(old) = previous {
                        old.class_list().remove_1("drop-before")?;
                    }
                    if let Some(target) = next {
                        target.class_list().add_1("drop-before")?;
                    }
                }
                return Ok(());
            }
            gesture.set(None);
            block.class_list().remove_1("dragging")?;
            if let Some(old) = previous {
                old.class_list().remove_1("drop-before")?;
            }
            if kind == "pointerup" && drag.moved {
                e.prevent_default();
                if let Some(index) = drop_slot(&chain, e.client_x(), e.client_y())?
                    .and_then(|slot| slot.get_attribute("data-chain-slot"))
                    .and_then(|index| index.parse::<u32>().ok())
                {
                    invoke(a, "moveBlock", &[uid.clone().into(), index.into()])?;
                }
            }
            Ok(())
        });
    }
}

fn shift_block(uid: &str, direction: i32, focus: bool) -> Result<(), JsValue> {
    let a = app();
    let state = store::state(&a)?;
    let chain = list(&state["chain"]);
    let Some(from) = chain.iter().position(|b| b["uid"] == uid) else {
        return Ok(());
    };
    let to = from as i32 + direction;
    if !(0..chain.len() as i32).contains(&to) {
        return Ok(());
    }
    invoke(
        &a,
        "moveBlock",
        &[uid.into(), (to + i32::from(direction > 0)).into()],
    )?;
    if focus {
        let nodes = element("chain")?.query_selector_all("[data-block]")?;
        for i in 0..nodes.length() {
            let node: HtmlElement = nodes.get(i).unwrap().unchecked_into();
            if node.get_attribute("data-block").as_deref() == Some(uid) {
                node.focus()?;
                break;
            }
        }
    }
    Ok(())
}

fn gap(root: &Element, i: usize, playing: &JsValue) -> Result<(), JsValue> {
    let g = child(root, "div", "gap", "");
    child(&g, "i", "wire", "");
    attr(&g, "data-chain-slot", &i.to_string());
    if playing.is_object() && num(playing, "index", -1.) == i as f64 {
        let d = invoke(&get(&app(), "catalog"), "get", &[get(playing, "pedalId")])?;
        let ghost = child(&g, "div", "block pedal ghost", "");
        style(&ghost, "--c", &string(&d, "color"));
        thumb(
            &ghost,
            "pedal",
            &string(&d, "color"),
            &string(&d, "artwork"),
        );
        let bt = child(&ghost, "div", "bt", "");
        child(&bt, "b", "", &string(&d, "name"));
        child(
            &bt,
            "small",
            "",
            if get(playing, "active").is_truthy() {
                "прослушивание"
            } else {
                "выкл (A/B)"
            },
        );
    } else {
        let b = button(&g, "plus", "Добавить педаль сюда", "+");
        attr(&b, "data-plus", &i.to_string());
        on(&b, "click", &app(), move |a, _| {
            open_catalog(a.clone(), js(&json!({"index":i}))?)
        });
    }
    Ok(())
}
fn block(root: &Element, b: &Value, s: &Value) -> Result<(), JsValue> {
    let k = text(b, "kind");
    let uid = text(b, "uid").to_owned();
    let info = info(b, s)?;
    let el = child(
        root,
        "div",
        if k == "pedal" {
            "block pedal"
        } else {
            "block core"
        },
        "",
    );
    attr(&el, "data-block", &uid);
    let movable = k != "gate";
    if movable {
        attr(&el, "data-chain-movable", "");
        attr(&el, "tabindex", "0");
        attr(&el, "aria-keyshortcuts", "Alt+ArrowLeft Alt+ArrowRight");
        attr(&el, "title", "Перетащите блок или нажмите Alt + ← / →");
    }
    style(&el, "--c", &info.color);
    for (class, active) in [
        ("gate", k == "gate"),
        ("off", !info.on),
        ("sel", string(&app(), "selected") == uid),
    ] {
        el.class_list().toggle_with_force(class, active)?;
    }
    thumb(&el, k, &info.color, &info.art);
    let bt = child(&el, "div", "bt", "");
    child(&bt, "b", "", &info.name);
    child(&bt, "small", "", &info.sub);
    let power = button(
        &el,
        if info.on { "bpow on" } else { "bpow" },
        if info.on {
            "Выключить (bypass)"
        } else {
            "Включить"
        },
        "",
    );
    attr(&power, "data-power", &uid);
    let id = uid.clone();
    on(&power, "click", &app(), move |a, e| {
        e.stop_propagation();
        invoke(a, "toggleBlock", &[id.clone().into()]).map(drop)
    });
    let id = uid.clone();
    on(&el, "click", &app(), move |a, _| {
        invoke(a, "select", &[id.clone().into()]).map(drop)
    });
    let block = js(b)?;
    let is_gate = k == "gate";
    on(&el, "contextmenu", &app(), move |_, e| {
        e.prevent_default();
        if !is_gate {
            menu_for(&block, num(&e, "clientX", 0.), num(&e, "clientY", 0.))?;
        }
        Ok(())
    });
    if movable {
        draggable(&el, &uid);
        on(&el, "keydown", &app(), move |_, event| {
            let e: KeyboardEvent = event.unchecked_into();
            if !e.alt_key() || e.target() != e.current_target() {
                return Ok(());
            }
            let direction = match e.key().as_str() {
                "ArrowLeft" => -1,
                "ArrowRight" => 1,
                _ => return Ok(()),
            };
            e.prevent_default();
            shift_block(&uid, direction, true)
        });
    }
    Ok(())
}
pub fn render() -> Result<(), JsValue> {
    let el = element("chain")?;
    el.set_text_content(None);
    let s = store::state(&app())?;
    let skin = crate::skins::active(&s);
    style(
        &doc().document_element().unwrap(),
        "--img-skin-thumb",
        &match skin {
            "ember" | "midnight" | "seafoam" | "walnut" => {
                format!("url(assets/gear/thumb_{skin}.png)")
            }
            _ => "linear-gradient(#333, #111)".into(),
        },
    );
    child(&el, "div", "jack", "IN");
    child(&el, "i", "wire", "");
    block(&el, &json!({"uid":"gate","kind":"gate"}), &s)?;
    let playing = get(&get(&app(), "engine"), "audition");
    let chain = list(&s["chain"]);
    for (i, b) in chain.iter().enumerate() {
        gap(&el, i, &playing)?;
        block(&el, b, &s)?;
    }
    gap(&el, chain.len(), &playing)?;
    child(&el, "div", "jack", "OUT");
    Ok(())
}
pub fn menu_for(b: &JsValue, x: f64, y: f64) -> Result<(), JsValue> {
    let b: Value = crate::read(b.clone())?;
    let state = store::state(&app())?;
    let info = info(&b, &state)?;
    let uid = text(&b, "uid").to_owned();
    let m = menu(x, y, "menu", None)?;
    let id = uid.clone();
    menu_item(
        &m,
        if info.on {
            "Выключить (bypass)"
        } else {
            "Включить"
        },
        false,
        move || call("toggleBlock", &[id.clone().into()]).map(drop),
    );
    let data = js(&json!({"type":"bypass","name":info.name,"id":uid}))?;
    menu_item(&m, "🎛 MIDI Learn: вкл/выкл", false, move || {
        invoke(&get(&app(), "midi"), "learn", &[data.clone()]).map(drop)
    });
    let chain = list(&state["chain"]);
    if let Some(index) = chain.iter().position(|b| b["uid"] == uid) {
        for (label, direction, disabled) in [
            ("Сдвинуть влево", -1, index == 0),
            ("Сдвинуть вправо", 1, index + 1 == chain.len()),
        ] {
            let uid = uid.clone();
            menu_item(&m, label, disabled, move || {
                shift_block(&uid, direction, true)
            });
        }
    }
    if b["kind"] == "pedal" {
        let id = uid.clone();
        menu_item(&m, "Заменить…", false, move || {
            let i = invoke(&get(&app(), "store"), "index", &[id.clone().into()])?.as_f64();
            open_catalog(app(), js(&json!({"index":i,"replaceUid":id}))?)
        });
        menu_item(&m, "Удалить", false, move || {
            call("removePedal", &[uid.clone().into()]).map(drop)
        });
    }
    Ok(())
}
