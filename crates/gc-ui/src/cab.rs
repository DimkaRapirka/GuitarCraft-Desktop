//! Cabinet room: two close mics and a stereo pair of room mics, a mixer for all three, the cab
//! front with both close mics draggable, and a top-down room plan with the room pair.
use crate::{
    dom::*,
    editor::{attr, button, knob_for, on, report, update_views},
    host::command,
    js, store,
};
use gc_model::{
    cab::{migrate, room_presets},
    catalog::text,
    EQUIPMENT,
};
use serde_json::{json, Value};
use std::cell::Cell;
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::{Element, PointerEvent, WheelEvent};

// View on stage: 0 cabinet front, 1 speaker close-up, 2 room plan.
thread_local! {static VIEW:Cell<u8>=const{Cell::new(0)};}

#[derive(Copy, Clone)]
struct Art {
    file: &'static str,
    aspect: f64,
    /// Speaker for mic A (centre x, y and radius as parts of the picture) and for mic B.
    x: f64,
    y: f64,
    r: f64,
    bx: f64,
    by: f64,
    zoom: bool,
}
impl Art {
    fn new(id: &str, zoom: bool) -> Self {
        let (file, aspect, x, y, r, b) = if zoom {
            ("cab_speaker.png", 1., 0.5, 0.5, 0.36, None)
        } else {
            match id {
                "1x12-open" => ("cab_1x12.png", 1., 0.5, 0.53, 0.25, None),
                "2x12-chime" => ("cab_2x12.png", 1.5, 0.33, 0.52, 0.15, Some((0.67, 0.52))),
                "4x12-g" => ("cab_4x12g.png", 1., 0.31, 0.33, 0.13, Some((0.31, 0.69))),
                "1x10-tiny" => ("cab_1x10.png", 1., 0.5, 0.56, 0.2, None),
                _ => ("cab_4x12v.png", 1., 0.30, 0.31, 0.13, Some((0.30, 0.69))),
            }
        };
        // one speaker: mic B works the same cone, drawn a little lower so the two never overlap
        let (bx, by) = b.unwrap_or((x, y + r * 0.62));
        Self { file, aspect, x, y, r, bx, by, zoom }
    }
    fn centre(self, g: &str) -> (f64, f64) {
        if g == "b" {
            (self.bx, self.by)
        } else {
            (self.x, self.y)
        }
    }
    /// Where the head of mic `g` is: `pos` 0 = cone centre, 1 = cone edge.
    fn head(self, g: &str, pos: f64) -> (f64, f64) {
        let (x, y) = self.centre(g);
        (x + pos * self.r / self.aspect, y)
    }
    fn width(self, d: f64) -> f64 {
        (if self.zoom { 0.62 } else { 0.4 }) * (1.15 - d * 0.45) / self.aspect * 100.
    }
}
fn n(c: &Value, key: &str, d: f64) -> f64 {
    c[key].as_f64().unwrap_or(d)
}
/// The cab of the current rig in the mics / room layout.
fn cab_now(app: &JsValue) -> Result<Value, JsValue> {
    let mut c = store::state(app)?["cab"].clone();
    migrate(&mut c);
    Ok(c)
}
/// Change the cab: always migrated first, `mic` mirrors mic A for older readers. `group` merges
/// a drag or a knob turn into one undo step; the views on stage follow without a full redraw.
fn edit(
    app: &JsValue,
    label: &str,
    group: Option<&str>,
    change: impl FnOnce(&mut Value),
) -> Result<(), JsValue> {
    store::edit(app, label, group, |s| {
        migrate(&mut s["cab"]);
        change(&mut s["cab"]);
        let a = s["cab"]["mics"]["a"]["mic"].clone();
        s["cab"]["mic"] = a;
    })?;
    refresh(app)
}
/// Discrete change (on, solo, mic, room type): edit and redraw the whole editor.
fn switch(app: &JsValue, label: &str, change: impl FnOnce(&mut Value)) -> Result<(), JsValue> {
    edit(app, label, None, change)?;
    update_views(app)
}
fn refresh(app: &JsValue) -> Result<(), JsValue> {
    let c = cab_now(app)?;
    let pics = doc().query_selector_all("#stage .cab-pic")?;
    for i in 0..pics.length() {
        let pic: Element = pics.get(i).unwrap().unchecked_into();
        place(&pic, Art::new(text(&c, "model"), pic.class_list().contains("close")), &c);
    }
    if let Some(plan) = doc().query_selector("#stage .room-plan")? {
        plan.set_inner_html(&plan_svg(&c));
    }
    Ok(())
}
fn place(pic: &Element, art: Art, c: &Value) {
    for g in ["a", "b"] {
        let m = &c["mics"][g];
        let shown = m["on"] == true;
        let pos = n(m, "position", 0.3);
        let (hx, hy) = art.head(g, pos);
        let left = format!("{}%", 100. * hx);
        let tilt = n(m, "angle", 0.) * 40.;
        for sel in [format!(".mic-{g}"), format!(".aim-{g}")] {
            if let Some(el) = pic.query_selector(&sel).ok().flatten() {
                style(&el, "display", if shown { "" } else { "none" });
                style(&el, "left", &left);
                style(&el, "top", &format!("{}%", 100. * hy));
            }
        }
        if let Some(st) = pic.query_selector(&format!(".stand-{g}")).ok().flatten() {
            let d = n(m, "distance", 0.2);
            style(&st, "display", if shown { "" } else { "none" });
            style(&st, "left", &format!("{}%", 100. * hx + art.width(d) * 0.86));
            style(&st, "top", &format!("{}%", 100. * hy - 1.5));
            style(&st, "height", &format!("{}%", 100. * (1.12 - hy)));
        }
        if let Some(el) = pic.query_selector(&format!(".mic-{g}")).ok().flatten() {
            let d = n(m, "distance", 0.2);
            style(&el, "width", &format!("{}%", art.width(d)));
            style(&el, "opacity", &(1. - d * 0.2).to_string());
            style(&el, "--mic", &format!("var(--img-mic-{})", text(m, "mic")));
            style(
                &el,
                "transform",
                &format!("translate(-3%, -50%) rotate({}deg)", -tilt),
            );
        }
    }
}
/// Which close mic the pointer means: the one whose head is nearer (B only when it is on).
fn pick(c: &Value, art: Art, fx: f64, fy: f64) -> &'static str {
    if c["mics"]["b"]["on"] != true {
        return "a";
    }
    let d = |g: &str| {
        let (x, y) = art.head(g, n(&c["mics"][g], "position", 0.3));
        ((x - fx) * art.aspect).powi(2) + (y - fy).powi(2)
    };
    if d("b") < d("a") {
        "b"
    } else {
        "a"
    }
}
fn drag(app: &JsValue, art: Art, pic: &Element, e: &PointerEvent, g: &str) -> Result<(), JsValue> {
    let r = pic.get_bounding_client_rect();
    if r.width() <= 0. {
        return Ok(());
    }
    let fx = (e.client_x() as f64 - r.left()) / r.width();
    let pos = ((fx - art.centre(g).0) * art.aspect / art.r).abs().clamp(0., 1.);
    let g = g.to_owned();
    edit(app, "Позиция микрофона", Some(&*format!("cab-{g}-position")), |c| {
        c["mics"][g.as_str()]["position"] = json!(pos)
    })
}

struct Room {
    lx: f64,
    ly: f64,
    lz: f64,
}
fn room_of(c: &Value) -> (Room, &'static Value) {
    let id = text(&c["room"], "room");
    let def = if EQUIPMENT["rooms"][id].is_object() {
        &EQUIPMENT["rooms"][id]
    } else {
        &EQUIPMENT["rooms"]["studio"]
    };
    let d = |i: usize| def["dims"][i].as_f64().unwrap_or(5.);
    (Room { lx: d(0), ly: d(1), lz: d(2) }, def)
}
/// The same geometry as `gc_dsp::room::room_response`, in metres.
fn pair(room: &Room, distance: f64, width: f64) -> (f64, f64, f64) {
    let src = 0.6f64.min(room.ly * 0.3);
    let y = (src + 0.5 + distance * (room.ly * 0.7 - 0.5).max(0.2)).min(room.ly - 0.3);
    (src, y, 0.1 + width * 1.9)
}
fn margin(room: &Room) -> f64 {
    room.lx.max(room.ly) * 0.06
}
fn plan_svg(c: &Value) -> String {
    let (room, def) = room_of(c);
    let r = &c["room"];
    let (src, y, sp) = pair(&room, n(r, "distance", 0.35), n(r, "width", 0.6));
    let (lx, ly) = (room.lx, room.ly);
    let m = margin(&room);
    let u = lx.max(ly) / 100.;
    let mut s = format!(
        r#"<svg viewBox="{} {} {} {}" xmlns="http://www.w3.org/2000/svg">"#,
        -m,
        -m,
        lx + 2. * m,
        ly + 2. * m
    );
    s += &format!(r#"<rect class="rp-floor" x="0" y="0" width="{lx}" height="{ly}" stroke-width="{}"/>"#, u * 0.8);
    s += &format!(r#"<image href="assets/gear/studio/floor_plan.jpg" x="0" y="0" width="{lx}" height="{ly}" preserveAspectRatio="xMidYMid slice" opacity=".9"/>"#);
    s += &format!(r#"<rect class="rp-wall" x="0" y="0" width="{lx}" height="{ly}" fill="none" stroke-width="{}"/>"#, u * 1.6);
    let step = if lx.max(ly) > 15. { 2. } else { 1. };
    let mut g = step;
    while g < lx {
        s += &format!(r#"<line class="rp-grid" x1="{g}" y1="0" x2="{g}" y2="{ly}" stroke-width="{}"/>"#, u * 0.25);
        g += step;
    }
    g = step;
    while g < ly {
        s += &format!(r#"<line class="rp-grid" x1="0" y1="{g}" x2="{lx}" y2="{g}" stroke-width="{}"/>"#, u * 0.25);
        g += step;
    }
    let on = r["on"] == true;
    let cx = lx / 2.;
    let mics = [(cx - sp / 2.).max(0.15), (cx + sp / 2.).min(lx - 0.15)];
    let k = if on { "rp-ray" } else { "rp-ray off" };
    for x in mics {
        s += &format!(r#"<line class="{k}" x1="{cx}" y1="{src}" x2="{x}" y2="{y}" stroke-width="{}" stroke-dasharray="{} {}"/>"#, u * 0.5, u * 1.5, u);
    }
    s += &format!(r#"<rect class="rp-cab" x="{}" y="{}" width="0.76" height="0.36" rx="0.04"/>"#, cx - 0.38, src - 0.36);
    for (i, x) in [cx - 0.16, cx + 0.16].iter().enumerate() {
        let g = if i == 0 { "a" } else { "b" };
        if c["mics"][g]["on"] == true {
            s += &format!(r#"<circle class="rp-close rp-{g}" cx="{x}" cy="{}" r="{}"/>"#, src + 0.08, u * 1.1);
        }
    }
    let rad = (u * 2.2).max(0.12);
    for (x, l) in [(mics[0], "L"), (mics[1], "R")] {
        s += &format!(r#"<circle class="rp-mic{}" cx="{x}" cy="{y}" r="{rad}" stroke-width="{}"/>"#, if on { "" } else { " off" }, u * 0.5);
        s += &format!(r#"<text class="rp-label" x="{x}" y="{}" font-size="{}" text-anchor="middle">{l}</text>"#, y + rad * 0.4, rad * 1.1);
    }
    s += &format!(
        r#"<text class="rp-dist" x="{}" y="{}" font-size="{}">{:.1} {}</text>"#,
        cx + sp / 2. + rad * 1.6,
        (src + y) / 2.,
        u * 3.2,
        y - src,
        crate::i18n::text("м")
    );
    s += &format!(
        r#"<text class="rp-name" x="0" y="{}" font-size="{}">{} · {}×{}×{} {}</text>"#,
        -m * 0.3,
        u * 3.4,
        text(def, "name"),
        room.lx,
        room.ly,
        room.lz,
        crate::i18n::text("м")
    );
    s + "</svg>"
}
fn plan_drag(app: &JsValue, el: &Element, e: &PointerEvent, first: bool) -> Result<(), JsValue> {
    let c = cab_now(app)?;
    let (room, _) = room_of(&c);
    let m = margin(&room);
    let (vw, vh) = (room.lx + 2. * m, room.ly + 2. * m);
    let r = el.get_bounding_client_rect();
    if r.width() <= 0. || r.height() <= 0. {
        return Ok(());
    }
    let s = (r.width() / vw).min(r.height() / vh);
    let x = (e.client_x() as f64 - r.left() - (r.width() - vw * s) / 2.) / s - m;
    let y = (e.client_y() as f64 - r.top() - (r.height() - vh * s) / 2.) / s - m;
    let (src, _, _) = pair(&room, 0., 0.);
    let distance = ((y - src - 0.5) / (room.ly * 0.7 - 0.5).max(0.2)).clamp(0., 1.);
    let width = ((2. * (x - room.lx / 2.).abs() - 0.1) / 1.9).clamp(0., 1.);
    edit(app, "Комнатные микрофоны", Some("cab-room-pair"), |c| {
        c["room"]["distance"] = json!((distance * 1000.).round() / 1000.);
        c["room"]["width"] = json!((width * 1000.).round() / 1000.);
        if first {
            c["room"]["on"] = json!(true);
        }
    })
}

fn front(app: &JsValue, stage: &Element, c: &Value, view: u8, ir: bool) -> Result<(), JsValue> {
    if view == 2 {
        let plan = child(stage, "div", "room-plan", "");
        attr(
            &plan,
            "title",
            "Тяните пару комнатных микрофонов: вверх-вниз — дальше от кабинета, в стороны — шире",
        );
        plan.set_inner_html(&plan_svg(c));
        for kind in ["pointerdown", "pointermove", "pointerup", "pointercancel"] {
            on(&plan, kind, app, move |app, e| {
                let el: Element = e.current_target().unwrap().unchecked_into();
                let e: PointerEvent = e.unchecked_into();
                let pointer = e.pointer_id();
                match kind {
                    "pointerdown" => {
                        set(&el, "gcDrag", &pointer.into());
                        let _ = el.set_pointer_capture(pointer);
                        plan_drag(app, &el, &e, true)
                    }
                    _ if get(&el, "gcDrag").as_f64() != Some(pointer as f64) => Ok(()),
                    "pointermove" => plan_drag(app, &el, &e, false),
                    _ => {
                        set(&el, "gcDrag", &JsValue::NULL);
                        let _ = el.release_pointer_capture(pointer);
                        update_views(app)
                    }
                }
            });
        }
        return Ok(());
    }
    let art = Art::new(text(c, "model"), view == 1);
    let pic = child(stage, "div", "cab-pic", "");
    pic.class_list().toggle_with_force("ir", ir)?;
    pic.class_list().toggle_with_force("close", art.zoom)?;
    style(&pic, "aspect-ratio", &art.aspect.to_string());
    style(&pic, "--cab-img", &format!("url(assets/gear/{})", art.file));
    for g in ["a", "b"] {
        child(&pic, "i", &format!("mic-aim aim-{g}"), &g.to_uppercase());
        child(&pic, "i", &format!("mic-stand stand-{g}"), "");
        child(&pic, "div", &format!("mic-side mic-{g}"), "");
    }
    attr(
        &pic,
        "title",
        "Тяните микрофон: центр ↔ край диффузора. Колесо: дистанция, Shift+колесо: угол",
    );
    place(&pic, art, c);
    if ir {
        child(
            &child(&pic, "div", "ir-badge", "IR"),
            "small",
            "",
            text(c, "irName"),
        );
        return Ok(());
    }
    for kind in ["pointerdown", "pointermove", "pointerup", "pointercancel"] {
        on(&pic, kind, app, move |app, e| {
            let el: Element = e.current_target().unwrap().unchecked_into();
            let e: PointerEvent = e.unchecked_into();
            let pointer = e.pointer_id();
            match kind {
                "pointerdown" => {
                    let r = el.get_bounding_client_rect();
                    let fx = (e.client_x() as f64 - r.left()) / r.width().max(1.);
                    let fy = (e.client_y() as f64 - r.top()) / r.height().max(1.);
                    let g = pick(&cab_now(app)?, art, fx, fy);
                    set(&el, "gcDrag", &pointer.into());
                    set(&el, "gcMic", &g.into());
                    let _ = el.set_pointer_capture(pointer);
                    drag(app, art, &el, &e, g)
                }
                _ if get(&el, "gcDrag").as_f64() != Some(pointer as f64) => Ok(()),
                "pointermove" => drag(app, art, &el, &e, &string(&el, "gcMic")),
                _ => {
                    set(&el, "gcDrag", &JsValue::NULL);
                    let _ = el.release_pointer_capture(pointer);
                    update_views(app)
                }
            }
        });
    }
    on(&pic, "wheel", app, move |app, e| {
        let el: Element = e.current_target().unwrap().unchecked_into();
        let e: WheelEvent = e.unchecked_into();
        e.prevent_default();
        let c = cab_now(app)?;
        let r = el.get_bounding_client_rect();
        let g = pick(
            &c,
            art,
            (e.client_x() as f64 - r.left()) / r.width().max(1.),
            (e.client_y() as f64 - r.top()) / r.height().max(1.),
        );
        let delta = if e.delta_y() != 0. { e.delta_y() } else { e.delta_x() };
        let key = if e.shift_key() { "angle" } else { "distance" };
        let v = (n(&c["mics"][g], key, 0.) + delta / 800.).clamp(0., 1.);
        let label = if key == "angle" {
            "Угол микрофона"
        } else {
            "Дистанция микрофона"
        };
        edit(app, label, Some(&*format!("cab-{g}-{key}")), |c| {
            c["mics"][g][key] = json!(v)
        })
    });
    Ok(())
}

fn db_text(v: f64) -> String {
    if v <= -48. {
        "−∞".into()
    } else {
        format!(
            "{}{v:.1} {}",
            if v > 0. { "+" } else { "" },
            crate::i18n::text("дБ")
        )
    }
}
#[allow(dead_code)]
fn pan_text(v: f64) -> String {
    let p = (v * 100.).round();
    if p == 0. {
        "C".into()
    } else if p < 0. {
        format!("L{}", -p)
    } else {
        format!("R{p}")
    }
}
/// A range input with a live readout; `apply` gets the raw input value.
fn slider(
    app: &JsValue,
    root: &Element,
    class: &str,
    (min, max, step, value): (f64, f64, f64, f64),
    show: fn(f64) -> String,
    apply: impl Fn(&JsValue, f64) -> Result<(), JsValue> + 'static,
) -> Element {
    let l = child(root, "label", class, "");
    let input = child(&l, "input", "", "");
    for (k, v) in [
        ("type", "range".to_string()),
        ("min", min.to_string()),
        ("max", max.to_string()),
        ("step", step.to_string()),
    ] {
        attr(&input, k, &v);
    }
    set(&input, "value", &value.to_string().into());
    child(&l, "output", "", &show(value));
    on(&input, "input", app, move |a, e| {
        let el: Element = e.current_target().unwrap().unchecked_into();
        let v = string(&el, "value").parse::<f64>().unwrap_or_default();
        if let Some(out) = el.next_element_sibling() {
            out.set_text_content(Some(&show(v)));
        }
        apply(a, v)
    });
    on(&input, "dblclick", app, |_, e| {
        e.prevent_default();
        Ok(())
    });
    l
}
fn select(root: &Element, options: &[(&str, &str)], value: &str) -> Element {
    let sel = child(root, "select", "", "");
    for (id, name) in options {
        attr(&child(&sel, "option", "", name), "value", id);
    }
    set(&sel, "value", &value.into());
    sel
}
fn names(kind: &str) -> Vec<(&'static str, &'static str)> {
    EQUIPMENT[kind]
        .as_object()
        .unwrap()
        .iter()
        .map(|(id, m)| (id.as_str(), text(m, "name")))
        .collect()
}
/// One mixer channel: Mic A, Mic B or the room pair.
fn strip(app: &JsValue, root: &Element, c: &Value, g: &'static str, ir: bool) -> Result<(), JsValue> {
    let room = g == "room";
    let m = if room { &c["room"] } else { &c["mics"][g] };
    let active = m["on"] == true;
    let solo = c["solo"].as_str() == Some(g);
    let dead = ir && g == "b";
    let el = child(
        root,
        "div",
        &format!(
            "mix-strip s-{g}{}{}{}",
            if active { "" } else { " off" },
            if solo { " solo" } else { "" },
            if dead { " dead" } else { "" }
        ),
        "",
    );
    let head = child(&el, "div", "ms-head", "");
    let led = button(&head, if active { "ms-on on" } else { "ms-on" }, "Включить / выключить", "");
    let title = match g {
        "a" if ir => "IR",
        "a" => "MIC A",
        "b" => "MIC B",
        _ => "ROOM",
    };
    child(&head, "b", "", title);
    let s = button(&head, if solo { "ms-solo on" } else { "ms-solo" }, "Соло: слушать только этот канал", "S");
    if dead {
        set(&led, "disabled", &true.into());
        set(&s, "disabled", &true.into());
        child(&el, "p", "ms-note", "Свой IR заменяет оба близких микрофона");
        return Ok(());
    }
    on(&led, "click", app, move |a, _| {
        switch(a, "Канал кабинета", |c| {
            let m = if room { &mut c["room"] } else { &mut c["mics"][g] };
            m["on"] = json!(m["on"] != true);
        })
    });
    on(&s, "click", app, move |a, _| {
        switch(a, "Соло канала", |c| {
            c["solo"] = if c["solo"].as_str() == Some(g) { Value::Null } else { json!(g) };
        })
    });
    if room {
        let sel = select(&el, &names("rooms"), text(m, "room"));
        attr(&sel, "title", "Помещение");
        on(&sel, "change", app, |a, e| {
            let v = string(&e.current_target().unwrap(), "value");
            switch(a, "Помещение", |c| {
                c["room"]["room"] = json!(v);
                c["room"]["on"] = json!(true);
            })
        });
    }
    if !(ir && !room) {
        let sel = select(&el, &names("mics"), text(m, "mic"));
        attr(&sel, "title", "Модель микрофона");
        on(&sel, "change", app, move |a, e| {
            let v = string(&e.current_target().unwrap(), "value");
            switch(a, "Микрофон", |c| {
                if room {
                    c["room"]["mic"] = json!(v)
                } else {
                    c["mics"][g]["mic"] = json!(v)
                }
            })
        });
        let knobs = child(&el, "div", "ms-knobs", "");
        let list: Vec<(&'static str, &'static str, Value, f64)> = if room {
            vec![
                ("distance", "ДАЛЕКО", json!({"min":0,"max":1,"format":"room-distance"}), 0.35),
                ("width", "ШИРИНА", json!({"min":0,"max":1,"format":"room-width"}), 0.6),
                ("lowcut", "СРЕЗ НЧ", json!({"min":20,"max":600,"unit":"Гц","log":true}), 120.),
            ]
        } else {
            vec![
                ("position", "ПОЗИЦИЯ", json!({"min":0,"max":1,"format":"cab-position"}), 0.3),
                ("distance", "ДИСТ.", json!({"min":0,"max":1,"format":"mic-distance"}), 0.2),
                ("angle", "УГОЛ", json!({"min":0,"max":1,"format":"mic-angle"}), 0.),
            ]
        };
        for (key, name, p, d) in list {
            let mut p = p;
            p["id"] = json!(key);
            p["name"] = json!(name);
            p["def"] = json!(d);
            let a = app.clone();
            let k = knob_for(p, n(m, key, d), &format!("cab.{g}.{key}"), 34., "hw", move |v| {
                edit(&a, "Микрофоны кабинета", Some(&*format!("cab-{g}-{key}")), |c| {
                    if room {
                        c["room"][key] = json!(v)
                    } else {
                        c["mics"][g][key] = json!(v)
                    }
                })
            })?;
            knobs.append_child(&k)?;
        }
    }
    if !room {
        let a = app.clone();
        let pan = knob_for(
            json!({"id":"pan","name":"PAN","min":-1,"max":1,"def":0,"format":"pan"}),
            n(m, "pan", 0.),
            &format!("cab.{g}.pan"),
            30.,
            "hw",
            move |v| {
                edit(&a, "Панорама микрофона", Some(&*format!("cab-{g}-pan")), |c| {
                    c["mics"][g]["pan"] = json!(v)
                })
            },
        )?;
        pan.class_list().add_1("ms-pan")?;
        el.append_child(&pan)?;
    }
    let lvl = n(m, "level", if room { -8. } else { 0. });
    slider(app, &el, "ms-fader", (-48., 12., 0.5, lvl.max(-48.)), db_text, move |a, v| {
        let v = if v <= -48. { -60. } else { v };
        edit(a, "Уровень канала", Some(&*format!("cab-{g}-level")), |c| {
            if room {
                c["room"]["level"] = json!(v)
            } else {
                c["mics"][g]["level"] = json!(v)
            }
        })
    });
    if !room {
        let phase = m["phase"] == true;
        let b = button(&el, if phase { "ms-phase on" } else { "ms-phase" }, "Инвертировать полярность", "Ø");
        attr(&b, "aria-pressed", if phase { "true" } else { "false" });
        on(&b, "click", app, move |a, _| {
            switch(a, "Полярность микрофона", |c| {
                c["mics"][g]["phase"] = json!(c["mics"][g]["phase"] != true)
            })
        });
    } else {
        let (_, def) = room_of(c);
        child(&el, "p", "ms-note", text(def, "desc"));
    }
    Ok(())
}

/// Cab front with draggable mics, cab model and the three-channel mic mixer, for the whole-chain view.
pub(crate) fn compact(app: &JsValue, root: &Element) -> Result<(), JsValue> {
    let c = cab_now(app)?;
    let ir = c["mode"] == "ir";
    front(app, root, &c, 0, ir)?;
    let sel = select(root, &names("cabs"), text(&c, "model"));
    attr(&sel, "title", "Кабинет");
    on(&sel, "change", app, |a, e| {
        let v = string(&e.current_target().unwrap(), "value");
        switch(a, "Кабинет", |c| {
            c["model"] = json!(v);
            c["mode"] = json!("model");
        })
    });
    let mixer = child(root, "div", "cab-mixer", "");
    for g in ["a", "b", "room"] {
        strip(app, &mixer, &c, g, ir)?;
    }
    Ok(())
}
#[wasm_bindgen(js_name=renderCab)]
pub fn render_cab(app: JsValue, stage: Element) -> Result<Element, JsValue> {
    stage.class_list().add_1("scene-studio")?;
    stage.class_list().add_1("scene-liveroom")?;
    let c = cab_now(&app)?;
    let view = VIEW.with(Cell::get);
    let ir = c["mode"] == "ir";
    let root = make("div", "editor cab-editor");
    let left = child(&root, "div", "cab-stage", "");
    let tabs = child(&left, "div", "src-switch cab-views", "");
    for (i, label) in [(0u8, "Кабинет"), (1, "Динамик крупно"), (2, "План комнаты")] {
        let b = button(&tabs, if view == i { "on" } else { "" }, "", label);
        if i == 1 {
            b.set_id("btn-closeup");
        }
        on(&b, "click", &app, move |a, _| {
            VIEW.with(|v| v.set(i));
            invoke(a, "renderStage", &[]).map(drop)
        });
    }
    front(&app, &left, &c, view, ir)?;
    let under = child(&left, "div", "cab-under", "");
    let caption = if view == 2 {
        let (_, def) = room_of(&c);
        format!("{} · {}", text(def, "name"), crate::i18n::text(text(def, "desc")))
    } else if ir {
        crate::i18n::message("Свой IR · {}", &[text(&c, "irName")])
    } else {
        let mic = |g: &str| text(&EQUIPMENT["mics"][text(&c["mics"][g], "mic")], "name");
        let mut s = format!(
            "{} · A: {}",
            text(&EQUIPMENT["cabs"][text(&c, "model")], "name"),
            mic("a")
        );
        if c["mics"]["b"]["on"] == true {
            s += &format!(" · B: {}", mic("b"));
        }
        s
    };
    child(&under, "span", "cab-caption", &caption);

    let side = child(&root, "div", "cab-side", "");
    let head = child(&side, "div", "side-head", "");
    child(&head, "h3", "", "Кабинет и микрофоны");
    let level = {
        let a = app.clone();
        knob_for(
            json!({"id":"level","name":"ВЫХОД","min":0,"max":1,"def":0.5,"format":"cab-level"}),
            n(&c, "level", 0.5),
            "cab.level",
            36.,
            "hw",
            move |v| edit(&a, "Уровень кабинета", Some("cablvl"), |c| c["level"] = json!(v)),
        )?
    };
    head.append_child(&level)?;
    let active = c["on"] != false;
    let power = button(&head, if active { "power on" } else { "power" }, "Вкл/выкл (bypass)", "");
    child(&power, "i", "", "");
    power.append_child(&doc().create_text_node(if active { "ON" } else { "OFF" }))?;
    on(&power, "click", &app, |a, _| {
        invoke(a, "toggleBlock", &["cab".into()]).map(drop)
    });
    let tiles = child(&side, "div", "cab-tiles", "");
    for (id, m) in EQUIPMENT["cabs"].as_object().unwrap() {
        let b = button(
            &tiles,
            if c["model"] == *id && !ir { "cab-tile on" } else { "cab-tile" },
            text(m, "desc"),
            "",
        );
        attr(&b, "data-cab", id);
        style(
            &child(&b, "i", "cab-thumb", ""),
            "background-image",
            &format!("url(assets/gear/{})", Art::new(id, false).file),
        );
        child(&b, "b", "", text(m, "name"));
        on(&b, "click", &app, move |a, _| {
            switch(a, &format!("Кабинет {}", text(m, "name")), |c| {
                c["model"] = json!(id);
                c["mode"] = json!("model");
            })
        });
    }
    let rooms = child(&side, "div", "room-chips", "");
    child(&rooms, "span", "muted", "Комната:");
    let room_on = c["room"]["on"] == true;
    let b = button(&rooms, if room_on { "chip" } else { "chip on" }, "", "Без комнаты");
    on(&b, "click", &app, |a, _| switch(a, "Без комнаты", |c| c["room"]["on"] = json!(false)));
    for p in room_presets().as_array().unwrap().clone() {
        let same = room_on && c["room"]["room"] == p["room"]["room"];
        let b = button(&rooms, if same { "chip on" } else { "chip" }, "", text(&p, "name"));
        on(&b, "click", &app, move |a, _| {
            let p = p.clone();
            switch(a, &format!("Комната: {}", text(&p, "name")), |c| {
                for (k, v) in p["room"].as_object().unwrap() {
                    c["room"][k] = v.clone();
                }
            })
        });
    }
    let mixer = child(&side, "div", "cab-mixer", "");
    for g in ["a", "b", "room"] {
        strip(&app, &mixer, &c, g, ir)?;
    }
    child(&side, "h3", "", "Свой импульс (IR)");
    let row = child(&side, "div", "row", "");
    let b = button(&row, "", "", "Загрузить IR…");
    b.set_id("btn-ir");
    on(&b, "click", &app, |a, _| {
        let a = a.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let r: Result<(), JsValue> = async {
                let file = command(
                    "file_open",
                    &js(&json!({"title":crate::i18n::text("Загрузить IR"),"filters":[{"name":"Impulse response","extensions":["wav","flac","mp3","ogg"]}]}))?,
                )
                .await?;
                let Some(file) = file.as_string() else { return Ok(()) };
                let dest = command("import_to", &js(&json!({"file":file,"kind":"irs"}))?)
                    .await?
                    .as_string()
                    .unwrap_or_default();
                let file = dest.rsplit(['/', char::from(92)]).next().unwrap_or(&dest);
                let name = file.rsplit_once('.').map(|(n, _)| n).unwrap_or(file);
                switch(&a, &format!("IR {file}"), |c| {
                    c["mode"] = json!("ir");
                    c["irPath"] = json!(dest);
                    c["irName"] = json!(name);
                })
            }
            .await;
            if let Err(e) = r {
                let _ = invoke(&a, "toast", &[e.clone()]);
                report(Err(e));
            }
        });
        Ok(())
    });
    if ir {
        on(&button(&row, "", "", "Вернуть модель"), "click", &app, |a, _| {
            switch(a, "Модель кабинета", |c| c["mode"] = json!("model"))
        });
    }
    Ok(root)
}
