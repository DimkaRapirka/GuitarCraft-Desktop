use crate::dom::*;
use js_sys::{Function, Object};
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::{
    CustomEvent, CustomEventInit, Element, Event, KeyboardEvent, MouseEvent, PointerEvent,
    WheelEvent,
};

fn unit(p: &JsValue, v: f64) -> f64 {
    let (min, max) = (num(p, "min", 0.), num(p, "max", 1.));
    if max <= min {
        return 0.;
    }
    if get(p, "log").is_truthy() && min > 0. {
        (v / min).ln() / (max / min).ln()
    } else {
        (v - min) / (max - min)
    }
}
fn value(p: &JsValue, t: f64) -> f64 {
    let (min, max) = (num(p, "min", 0.), num(p, "max", 1.));
    let t = t.clamp(0., 1.);
    let v = if get(p, "log").is_truthy() && min > 0. {
        min * (max / min).powf(t)
    } else {
        min + (max - min) * t
    };
    let step = num(p, "step", 0.);
    if step > 0. {
        (v / step).round() * step
    } else {
        v
    }
}

#[wasm_bindgen]
pub fn fmt(p: &JsValue, v: f64) -> String {
    if let Some(f) = get(p, "display").dyn_ref::<Function>() {
        if let Ok(out) = f.call1(p, &v.into()) {
            if let Some(s) = out.as_string() {
                return s;
            }
        }
    }
    match string(p, "format").as_str() {
        "cab-position" => {
            return crate::i18n::message("{}% от центра", &[&(v * 100.).round().to_string()])
        }
        "cab-distance" => {
            return if v < 0.15 {
                crate::i18n::text("вплотную").into()
            } else {
                crate::i18n::message("{}% (отн.)", &[&(v * 100.).round().to_string()])
            }
        }
        "mic-distance" => {
            return format!("{} {}", (1. + v * 60.).round(), crate::i18n::text("см"))
        }
        "mic-angle" => return format!("{}°", (v * 90.).round()),
        "pan" => {
            let p = (v * 100.).round();
            return if p == 0. {
                "C".into()
            } else if p < 0. {
                format!("L{}", -p)
            } else {
                format!("R{p}")
            };
        }
        "room-distance" => {
            return crate::i18n::message("{}% к стене", &[&(v * 100.).round().to_string()])
        }
        "room-width" => {
            return format!("{:.1} {}", 0.1 + v * 1.9, crate::i18n::text("м"))
        }
        "cab-level" => {
            let db = (v - 0.5) * 24.;
            return format!(
                "{}{db:.1} {}",
                if db > 0. { "+" } else { "" },
                crate::i18n::text("дБ")
            );
        }
        "delay-tone" => {
            return format!(
                "{} {}",
                (1500. * 8_f64.powf(v.clamp(0., 1.)) / 100.).round() / 10.,
                crate::i18n::text("кГц")
            )
        }
        _ => {}
    }
    let sign = if v > 0. { "+" } else { "" };
    match string(p, "unit").as_str() {
        "%" => format!("{}%", (v * 100.).round()),
        "st" => format!("{sign}{} st", v.round()),
        "Гц" if v >= 10000. => format!("{:.1} {}", v / 1000., crate::i18n::text("кГц")),
        "Гц" if v >= 1000. => format!("{:.2} {}", v / 1000., crate::i18n::text("кГц")),
        "Гц" => format!("{} {}", v.round(), crate::i18n::text("Гц")),
        "дБ" => format!("{sign}{v:.1} {}", crate::i18n::text("дБ")),
        "мс" => format!("{} {}", v.round(), crate::i18n::text("мс")),
        "с" => format!("{v:.1} {}", crate::i18n::text("с")),
        _ if num(p, "min", 0.) == 0. && num(p, "max", 1.) == 1. => format!("{:.1}", v * 10.),
        _ if num(p, "max", 0.) > 2. && num(p, "max", 0.).fract() == 0. => format!("{}", v.round()),
        _ => format!("{v:.2}"),
    }
}
fn render(root: &Element, p: &JsValue, v: f64) {
    let t = unit(p, v).clamp(0., 1.);
    style(
        &root.query_selector(".knob-rotor").unwrap().unwrap(),
        "transform",
        &format!("rotate({}deg)", -135. + t * 270.),
    );
    root.query_selector(".ring-fg")
        .unwrap()
        .unwrap()
        .set_attribute(
            "style",
            &format!(
                "stroke-dasharray:212 212;stroke-dashoffset:{}",
                212. * (1. - t)
            ),
        )
        .unwrap();
    let display = fmt(p, v);
    root.query_selector(".knob-value")
        .unwrap()
        .unwrap()
        .set_text_content(Some(&display));
    let body = root.query_selector(".knob-body").unwrap().unwrap();
    body.set_attribute("aria-valuenow", &v.to_string()).unwrap();
    body.set_attribute("aria-valuetext", &display).unwrap();
}
thread_local! {static DRAG:std::cell::RefCell<Option<String>>=const{std::cell::RefCell::new(None)};}
pub(crate) fn gesture_key(key: &str) -> String {
    DRAG.with(|s| {
        s.borrow()
            .as_ref()
            .map(|id| format!("drag/{id}/{key}"))
            .unwrap_or_else(|| key.into())
    })
}
fn handle(ev: Event) {
    let Some(body) = ev
        .current_target()
        .and_then(|t| t.dyn_into::<Element>().ok())
    else {
        return;
    };
    let Some(root) = body.parent_element() else {
        return;
    };
    let opts = get(&root, "gcOptions");
    let p = get(&opts, "param");
    let v = num(&root, "gcValue", num(&p, "def", 0.5));
    let mut next = v;
    let mut final_value = true;
    match ev.type_().as_str() {
        "pointerdown" => {
            let e = ev.unchecked_ref::<PointerEvent>();
            e.prevent_default();
            let _ = body.set_pointer_capture(e.pointer_id());
            set(&root, "gcDragId", &gc_model::new_uid().into());
            set(&root, "gcPointer", &e.pointer_id().into());
            set(&root, "gcStartY", &e.client_y().into());
            set(&root, "gcStartT", &unit(&p, v).into());
            let _ = root.class_list().add_1("active");
            return;
        }
        "pointermove" | "pointerup" | "pointercancel" | "lostpointercapture" => {
            let e = ev.unchecked_ref::<PointerEvent>();
            if get(&root, "gcPointer").as_f64() != Some(e.pointer_id() as f64) {
                return;
            }
            if ev.type_() == "pointermove" {
                let fine = if e.shift_key() || e.alt_key() {
                    0.15
                } else {
                    1.
                };
                next = value(
                    &p,
                    num(&root, "gcStartT", 0.)
                        + (num(&root, "gcStartY", 0.) - e.client_y() as f64) / 180. * fine,
                );
                final_value = false;
            } else {
                set(&root, "gcPointer", &JsValue::UNDEFINED);
                let _ = root.class_list().remove_1("active");
            }
        }
        "contextmenu" => {
            let e = ev.unchecked_ref::<MouseEvent>();
            e.prevent_default();
            let detail = Object::new();
            set(&detail, "id", &get(&opts, "id"));
            set(&detail, "param", &p);
            set(&detail, "x", &e.client_x().into());
            set(&detail, "y", &e.client_y().into());
            let init = CustomEventInit::new();
            init.set_bubbles(true);
            init.set_detail(&detail);
            let event = CustomEvent::new_with_event_init_dict("knob-menu", &init).unwrap();
            let _ = root.dispatch_event(&event);
            return;
        }
        "dblclick" => next = num(&p, "def", value(&p, 0.5)),
        "wheel" => {
            let e = ev.unchecked_ref::<WheelEvent>();
            e.prevent_default();
            next = value(
                &p,
                unit(&p, v) - e.delta_y() / 1500. * if e.shift_key() { 0.2 } else { 1. },
            );
        }
        "keydown" => {
            let e = ev.unchecked_ref::<KeyboardEvent>();
            let step = if e.shift_key() { 0.01 } else { 0.05 };
            let d = match e.key().as_str() {
                "ArrowUp" | "ArrowRight" => step,
                "ArrowDown" | "ArrowLeft" => -step,
                _ => return,
            };
            e.prevent_default();
            next = value(&p, unit(&p, v) + d);
        }
        _ => return,
    }
    set(&root, "gcValue", &next.into());
    render(&root, &p, next);
    if let Some(f) = get(&opts, "onChange").dyn_ref::<Function>() {
        let dragging = matches!(
            ev.type_().as_str(),
            "pointermove" | "pointerup" | "pointercancel" | "lostpointercapture"
        );
        DRAG.with(|s| {
            s.replace(if dragging {
                get(&root, "gcDragId").as_string()
            } else {
                None
            })
        });
        if let Err(e) = f.call2(&JsValue::UNDEFINED, &next.into(), &final_value.into()) {
            web_sys::console::error_1(&e)
        }
        DRAG.with(|s| s.take());
    }
}
#[wasm_bindgen]
pub fn knob(options: &JsValue) -> Element {
    let p = get(options, "param");
    let skin = get(options, "style")
        .as_string()
        .unwrap_or_else(|| "dark".into());
    let root = make("div", &format!("knob knob-{skin}"));
    set(&root, "gcOptions", options);
    if let Some(id) = get(options, "id").as_string() {
        root.set_attribute("data-knob", &id).unwrap();
    }
    style(&root, "--size", &format!("{}px", num(options, "size", 56.)));
    let body = child(&root, "div", "knob-body", "");
    let name = string(&p, "name");
    for (k, v) in [
        ("tabindex", "0".into()),
        ("role", "slider".into()),
        ("aria-label", name.clone()),
        ("aria-valuemin", num(&p, "min", 0.).to_string()),
        ("aria-valuemax", num(&p, "max", 1.).to_string()),
    ] {
        body.set_attribute(k, &v).unwrap();
    }
    // Only this fixed SVG is parsed as markup. Labels and imported pedal names are text nodes.
    let mut ticks = String::new();
    for i in 0..11 {
        let a = ((-135 + i * 27) as f64).to_radians();
        let r = if i % 5 == 0 { 40. } else { 43. };
        ticks += &format!(
            r#"<line x1="{}" y1="{}" x2="{}" y2="{}"/>"#,
            50. + 47. * a.sin(),
            50. - 47. * a.cos(),
            50. + r * a.sin(),
            50. - r * a.cos()
        );
    }
    body.set_inner_html(&format!(r#"<svg class="knob-ring" viewBox="0 0 100 100"><path class="ring-bg" d="M 18.2 81.8 A 45 45 0 1 1 81.8 81.8"/><path class="ring-fg" d="M 18.2 81.8 A 45 45 0 1 1 81.8 81.8"/><g class="ticks">{ticks}</g></svg><div class="knob-cap"><div class="knob-rotor"><div class="knob-pointer"></div></div></div>"#));
    if get(options, "label").as_bool() != Some(false) {
        child(&root, "div", "knob-label", &name);
    }
    child(&root, "div", "knob-value", "");
    let v = num(options, "value", num(&p, "def", 0.5));
    set(&root, "gcValue", &v.into());
    render(&root, &p, v);
    for kind in [
        "pointerdown",
        "pointermove",
        "pointerup",
        "pointercancel",
        "lostpointercapture",
        "wheel",
        "keydown",
        "dblclick",
        "contextmenu",
    ] {
        listen(&body, kind, handle);
    }
    root
}
