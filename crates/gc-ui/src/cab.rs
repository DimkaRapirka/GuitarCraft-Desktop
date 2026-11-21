//! Cabinet, two microphones, impulse-response import and the draggable speaker view.
use crate::{
    dom::*,
    editor::{attr, button, knob_for, on, report, update_views},
    host::command,
    js, store,
};
use gc_model::{catalog::text, EQUIPMENT};
use serde_json::{json, Value};
use std::cell::Cell;
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::{Element, PointerEvent, WheelEvent};
thread_local! {static CLOSE:Cell<bool>=const{Cell::new(false)};}
#[derive(Copy, Clone)]
struct Art {
    file: &'static str,
    aspect: f64,
    x: f64,
    y: f64,
    r: f64,
    zoom: bool,
}
impl Art {
    fn new(id: &str, zoom: bool) -> Self {
        let (file, aspect, x, y, r) = if zoom {
            ("cab_speaker.png", 1., 0.5, 0.5, 0.36)
        } else {
            match id {
                "1x12-open" => ("cab_1x12.png", 1., 0.5, 0.53, 0.25),
                "2x12-chime" => ("cab_2x12.png", 1.5, 0.33, 0.52, 0.15),
                "4x12-g" => ("cab_4x12g.png", 1., 0.31, 0.33, 0.13),
                "1x10-tiny" => ("cab_1x10.png", 1., 0.5, 0.56, 0.2),
                _ => ("cab_4x12v.png", 1., 0.30, 0.31, 0.13),
            }
        };
        Self {
            file,
            aspect,
            x,
            y,
            r,
            zoom,
        }
    }
    fn left(self, pos: f64) -> String {
        format!("{}%", 100. * (self.x + pos * self.r / self.aspect))
    }
    fn width(self, d: f64) -> String {
        format!(
            "{}%",
            (if self.zoom { 0.62 } else { 0.4 }) * (1.15 - d * 0.45) / self.aspect * 100.
        )
    }
    fn apply(self, pic: &Element, key: &str, v: f64) {
        if let Some(m) = pic.query_selector(".mic-side").ok().flatten() {
            match key {
                "position" => {
                    style(&m, "left", &self.left(v));
                    if let Some(aim) = pic.query_selector(".mic-aim").ok().flatten() {
                        style(&aim, "left", &self.left(v));
                    }
                }
                "distance" => {
                    style(&m, "width", &self.width(v));
                    style(&m, "opacity", &(1. - v * 0.2).to_string());
                }
                _ => {}
            }
        }
    }
}
fn n(c: &Value, key: &str, d: f64) -> f64 {
    c[key].as_f64().unwrap_or(d)
}
fn patch(app: &JsValue, label: &str, change: impl FnOnce(&mut Value)) -> Result<(), JsValue> {
    store::edit(app, label, None, |s| change(&mut s["cab"]))?;
    update_views(app)
}
fn mic_edit(app: &JsValue, art: Art, pic: &Element, key: &str, v: f64) -> Result<(), JsValue> {
    art.apply(pic, key, v);
    let (label, group) = match key {
        "position" => ("Позиция микрофона", "cabpos"),
        "distance" => ("Дистанция микрофона", "cabdist"),
        _ => ("Уровень кабинета", "cablvl"),
    };
    store::edit(app, label, Some(group), |s| s["cab"][key] = json!(v))
}
fn drag(app: &JsValue, art: Art, pic: &Element, e: &PointerEvent) -> Result<(), JsValue> {
    let r = pic.get_bounding_client_rect();
    if r.width() <= 0. {
        return Ok(());
    }
    let pos =
        (((e.client_x() as f64 - r.left()) / r.width() - art.x) * art.aspect / art.r).clamp(0., 1.);
    mic_edit(app, art, pic, "position", pos)
}
#[wasm_bindgen(js_name=renderCab)]
pub fn render_cab(app: JsValue, stage: Element) -> Result<Element, JsValue> {
    stage.class_list().add_1("scene-studio")?;
    let c = store::state(&app)?["cab"].clone();
    let zoom = CLOSE.with(Cell::get);
    let art = Art::new(text(&c, "model"), zoom);
    let ir = c["mode"] == "ir";
    let root = make("div", "editor cab-editor");
    let front = child(&root, "div", "cab-stage", "");
    let pic = child(&front, "div", "cab-pic", "");
    pic.class_list().toggle_with_force("ir", ir)?;
    pic.class_list().toggle_with_force("close", zoom)?;
    style(&pic, "aspect-ratio", &art.aspect.to_string());
    style(&pic, "--cab-img", &format!("url(assets/gear/{})", art.file));
    let aim = child(&pic, "i", "mic-aim", "");
    let mic = child(&pic, "div", "mic-side", "");
    style(
        &mic,
        "--mic",
        &format!("var(--img-mic-{})", text(&c, "mic")),
    );
    attr(
        &mic,
        "title",
        "Тяните микрофон: центр ↔ край диффузора. Колесо: дистанция",
    );
    for el in [&aim, &mic] {
        style(el, "top", &format!("{}%", 100. * art.y));
    }
    art.apply(&pic, "position", n(&c, "position", 0.3));
    art.apply(&pic, "distance", n(&c, "distance", 0.2));
    if ir {
        child(
            &child(&pic, "div", "ir-badge", "IR"),
            "small",
            "",
            text(&c, "irName"),
        );
    } else {
        for kind in ["pointerdown", "pointermove", "pointerup", "pointercancel"] {
            on(&pic, kind, &app, move |app, e| {
                let el: Element = e.current_target().unwrap().unchecked_into();
                let e: PointerEvent = e.unchecked_into();
                let pointer = e.pointer_id();
                match kind {
                    "pointerdown" => {
                        set(&el, "gcDrag", &pointer.into());
                        let _ = el.set_pointer_capture(pointer);
                        drag(app, art, &el, &e)?;
                    }
                    _ if get(&el, "gcDrag").as_f64() != Some(pointer as f64) => {}
                    "pointermove" => drag(app, art, &el, &e)?,
                    _ => {
                        set(&el, "gcDrag", &JsValue::NULL);
                        let _ = el.release_pointer_capture(pointer);
                        invoke(app, "renderStage", &[])?;
                    }
                }
                Ok(())
            });
        }
        on(&pic, "wheel", &app, move |app, e| {
            let el: Element = e.current_target().unwrap().unchecked_into();
            let e: WheelEvent = e.unchecked_into();
            e.prevent_default();
            let d = n(&store::state(app)?["cab"], "distance", 0.2);
            mic_edit(
                app,
                art,
                &el,
                "distance",
                (d + e.delta_y() / 800.).clamp(0., 1.),
            )
        });
    }
    let under = child(&front, "div", "cab-under", "");
    let caption = if ir {
        format!("Свой IR · {}", text(&c, "irName"))
    } else {
        format!(
            "{} · {} · {:.0}% от центра",
            text(&EQUIPMENT["cabs"][text(&c, "model")], "name"),
            text(&EQUIPMENT["mics"][text(&c, "mic")], "name"),
            100. * n(&c, "position", 0.3)
        )
    };
    child(&under, "span", "cab-caption", &caption);
    let b = button(
        &under,
        if zoom { "ghost on" } else { "ghost" },
        "",
        if zoom {
            "◱ Весь кабинет"
        } else {
            "◎ Динамик крупно"
        },
    );
    b.set_id("btn-closeup");
    on(&b, "click", &app, |app, _| {
        CLOSE.with(|z| z.set(!z.get()));
        invoke(app, "renderStage", &[]).map(drop)
    });
    let side = child(&root, "div", "cab-side", "");
    let head = child(&side, "div", "side-head", "");
    child(&head, "h3", "", "Кабинет");
    let active = c["on"] != false;
    let power = button(
        &head,
        if active { "power on" } else { "power" },
        "Вкл/выкл (bypass)",
        "",
    );
    child(&power, "i", "", "");
    power.append_child(&doc().create_text_node(if active { "ON" } else { "OFF" }))?;
    on(&power, "click", &app, |a, _| {
        invoke(a, "toggleBlock", &["cab".into()]).map(drop)
    });
    let tiles = child(&side, "div", "cab-tiles", "");
    for (id, m) in EQUIPMENT["cabs"].as_object().unwrap() {
        let b = button(
            &tiles,
            if c["model"] == *id && !ir {
                "cab-tile on"
            } else {
                "cab-tile"
            },
            "",
            "",
        );
        attr(&b, "data-cab", id);
        style(
            &child(&b, "i", "cab-thumb", ""),
            "background-image",
            &format!("url(assets/gear/{})", Art::new(id, false).file),
        );
        child(&b, "b", "", text(m, "name"));
        child(&b, "small", "", text(m, "desc"));
        on(&b, "click", &app, move |a, _| {
            patch(a, &format!("Кабинет {}", text(m, "name")), |c| {
                c["model"] = json!(id);
                c["mode"] = json!("model");
            })
        });
    }
    child(&side, "h3", "", "Микрофон");
    let tiles = child(&side, "div", "mic-tiles", "");
    for (id, m) in EQUIPMENT["mics"].as_object().unwrap() {
        let b = button(
            &tiles,
            if c["mic"] == *id {
                "mic-tile on"
            } else {
                "mic-tile"
            },
            text(m, "desc"),
            "",
        );
        attr(&b, "data-mic", id);
        set(&b, "disabled", &ir.into());
        style(
            &child(&b, "i", "", ""),
            "background-image",
            &format!("var(--img-mic-{id})"),
        );
        b.append_child(&doc().create_text_node(text(m, "name")))?;
        on(&b, "click", &app, move |a, _| {
            patch(a, &format!("Микрофон {}", text(m, "name")), |c| {
                c["mic"] = json!(id)
            })
        });
    }
    let knobs = child(&side, "div", "row knobs hw-row", "");
    for (key, d) in [("position", 0.3), ("distance", 0.2), ("level", 0.5)] {
        let a = app.clone();
        let k = knob_for(
            json!({"id":key,"name":key.to_uppercase(),"min":0,"max":1,"def":d,"format":format!("cab-{key}")}),
            n(&c, key, d),
            &format!("cab.{key}"),
            54.,
            "hw",
            move |v| {
                if let Some(pic) = doc().query_selector("#stage .cab-pic")? {
                    mic_edit(&a, art, &pic, key, v)?;
                }
                Ok(())
            },
        )?;
        knobs.append_child(&k)?;
    }
    if !ir {
        let blend = child(&side, "div", "mic-blend", "");
        let sel = child(&child(&blend, "label", "", "Микрофон 2"), "select", "", "");
        sel.set_id("cab-mic2");
        attr(&child(&sel, "option", "", "Выкл."), "value", "");
        for (id, m) in EQUIPMENT["mics"].as_object().unwrap() {
            attr(&child(&sel, "option", "", text(m, "name")), "value", id);
        }
        set(&sel, "value", &text(&c, "mic2").into());
        on(&sel, "change", &app, |a, e| {
            let v = string(&e.current_target().unwrap(), "value");
            patch(a, "Второй микрофон", |c| c["mic2"] = json!(v))
        });
        if !text(&c, "mic2").is_empty() {
            for (key, title, d) in [
                ("blend", "Доля второго", 0.5),
                ("position2", "Позиция", 0.65),
                ("distance2", "Дистанция", 0.25),
            ] {
                let l = child(&blend, "label", "", title);
                let input = child(&l, "input", "", "");
                for (k, v) in [("type", "range"), ("min", "0"), ("max", "100")] {
                    attr(&input, k, v);
                }
                let value = (n(&c, key, d) * 100.).round();
                set(&input, "value", &value.to_string().into());
                child(&l, "output", "", &format!("{value}%"));
                on(&input, "input", &app, move |a, e| {
                    let el: Element = e.current_target().unwrap().unchecked_into();
                    let v = string(&el, "value").parse::<f64>().unwrap_or_default();
                    if let Some(out) = el.next_element_sibling() {
                        out.set_text_content(Some(&format!("{v}%")));
                    }
                    store::edit(a, title, Some(key), |s| s["cab"][key] = json!(v / 100.))
                });
            }
            let invert = c["invert2"] == true;
            let b = button(
                &blend,
                "",
                "Инвертировать полярность второго микрофона",
                if invert { "Ø вкл." } else { "Ø выкл." },
            );
            attr(&b, "aria-pressed", if invert { "true" } else { "false" });
            on(&b, "click", &app, |a, _| {
                patch(
                    a,
                    "Полярность второго микрофона",
                    |c| c["invert2"] = json!(c["invert2"] != true),
                )
            });
        }
    }
    child(&side, "h3", "", "Свой импульс (IR)");
    let row = child(&side, "div", "row", "");
    let b = button(&row, "", "", "Загрузить IR…");
    b.set_id("btn-ir");
    on(&b, "click", &app, |a, _| {
        let a = a.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let r:Result<(),JsValue>=async{
        let file=command("file_open",&js(&json!({"title":"Загрузить IR","filters":[{"name":"Impulse response","extensions":["wav","flac","mp3","ogg"]}]}))?).await?;
        let Some(file)=file.as_string()else{return Ok(())};let dest=command("import_to",&js(&json!({"file":file,"kind":"irs"}))?).await?.as_string().unwrap_or_default();
        let file=dest.rsplit(['/',char::from(92)]).next().unwrap_or(&dest);let name=file.rsplit_once('.').map(|(n,_)|n).unwrap_or(file);
        patch(&a,&format!("IR {file}"),|c|{c["mode"]=json!("ir");c["irPath"]=json!(dest);c["irName"]=json!(name);})
    }.await;
            if let Err(e) = r {
                let _ = invoke(&a, "toast", &[e.clone()]);
                report(Err(e));
            }
        });
        Ok(())
    });
    if ir {
        on(
            &button(&row, "", "", "Вернуть модель"),
            "click",
            &app,
            |a, _| {
                patch(a, "Модель кабинета", |c| {
                    c["mode"] = json!("model")
                })
            },
        );
    }
    Ok(root)
}
