//! Gate, seven-band EQ with live spectrum, tempo-synchronised delay and reverb racks.
use crate::{
    dom::*,
    editor::{attr, button, knob_for, on, report},
    host::{resolved, sleep},
    store,
};
use gc_model::{
    catalog::{list, text},
    rig::sync_time,
    EQUIPMENT,
};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::spawn_local;
use web_sys::{
    CanvasRenderingContext2d as Canvas, Element, HtmlCanvasElement, ScrollIntoViewOptions,
    ScrollLogicalPosition,
};
fn unit(
    root: &Element,
    a: &JsValue,
    kind: &'static str,
    title: &str,
    sub: &str,
    active: bool,
    selected: &str,
) -> Result<Element, JsValue> {
    let rack = child(
        root,
        "div",
        &format!(
            "rack-unit u-{kind} {}{}",
            if active { "on" } else { "off" },
            if selected == kind { " sel" } else { "" }
        ),
        "",
    );
    attr(&rack, "data-rack", kind);
    on(&rack, "pointerdown", a, move |a, e| {
        if string(a, "selected") == kind {
            return Ok(());
        }
        set(a, "selected", &kind.into());
        invoke(a, "renderChain", &[])?;
        let t: Element = e.current_target().unwrap().unchecked_into();
        let units = t.parent_element().unwrap().children();
        for i in 0..units.length() {
            let el = units.item(i).unwrap();
            el.class_list().toggle_with_force("sel", el == t)?;
        }
        Ok(())
    });
    let face = child(&rack, "div", "ru-face", "");
    let brand = child(&face, "div", "ru-brand", "");
    child(&brand, "b", "", title);
    child(&brand, "small", "", sub);
    let b = button(
        &face,
        if active { "ru-power on" } else { "ru-power" },
        "Питание (bypass)",
        "",
    );
    child(&b, "i", "", "");
    child(&b, "span", "", if active { "ON" } else { "OFF" });
    on(&b, "click", a, move |a, e| {
        e.stop_propagation();
        invoke(a, "toggleBlock", &[kind.into()]).map(drop)
    });
    Ok(child(&face, "div", "ru-content", ""))
}
fn lcd(app: &JsValue, kind: &str) -> Result<(), JsValue> {
    let Some(el) = doc().query_selector(&format!("[data-rack='{kind}'] .lcd-text"))? else {
        return Ok(());
    };
    let s = get(&get(&get(app, "store"), "state"), kind);
    let (big, small) = if kind == "delay" {
        let bpm = num(&get(app, "settings"), "bpm", 120.);
        let sync = string(&s, "sync");
        let t = sync_time(&sync, bpm);
        (
            format!("{:.0} ms", t.unwrap_or(num(&s, "time", 400.))),
            if t.is_some() {
                format!("{} · {bpm} BPM", sync.to_uppercase())
            } else {
                "FREE".into()
            },
        )
    } else {
        (
            format!("{:.1} s", num(&s, "decay", 2.2)),
            format!("PRE {:.0} ms", num(&s, "predelay", 15.)),
        )
    };
    el.first_element_child()
        .unwrap()
        .set_text_content(Some(&big));
    el.last_element_child()
        .unwrap()
        .set_text_content(Some(&small));
    Ok(())
}
fn controls(
    parent: &Element,
    app: &JsValue,
    kind: &'static str,
    params: &[Value],
    model: &Value,
) -> Result<(), JsValue> {
    controls_sized(parent, app, kind, params, model, 58.)
}
/// Knobs of a rack (`gate`, `eq`, `delay`, `reverb`) at a given knob size.
pub(crate) fn controls_sized(
    parent: &Element,
    app: &JsValue,
    kind: &'static str,
    params: &[Value],
    model: &Value,
    size: f64,
) -> Result<(), JsValue> {
    for def in params {
        let mut p = def.clone();
        let key = text(def, "id").to_owned();
        let name = text(def, "name");
        let label = if kind == "eq" {
            match key.as_str() {
                "lowcut" => "LOW CUT",
                "low" => "100",
                "lowmid" => "400",
                "mid" => "1K",
                "highmid" => "2.5K",
                "high" => "6K",
                "highcut" => "HIGH CUT",
                _ => name,
            }
            .to_owned()
        } else {
            name.to_uppercase()
        };
        p["name"] = json!(label);
        match (kind, key.as_str()) {
            ("delay", "feedback" | "mix" | "mod") | ("reverb", "mix" | "tone") => {
                p["unit"] = json!("%")
            }
            ("delay", "tone") => p["format"] = json!("delay-tone"),
            _ => {}
        }
        let disabled =
            kind == "delay" && key == "time" && sync_time(text(model, "sync"), 120.).is_some();
        let id = format!("{kind}.{key}");
        let group = id.clone();
        let a = app.clone();
        let v = model[&key].as_f64().or(def["def"].as_f64()).unwrap_or(0.5);
        let name = if kind == "gate" {
            if key == "threshold" {
                "Порог гейта".into()
            } else {
                "Release гейта".into()
            }
        } else {
            format!("{}: {name}", kind.to_uppercase())
        };
        let k = knob_for(p, v, &id, size, "hw", move |v| {
            store::edit(&a, &name, Some(&group), |s| {
                if kind == "gate" {
                    s["input"]["gate"][&key] = json!(v)
                } else {
                    s[kind][&key] = json!(v)
                }
            })?;
            if matches!(kind, "delay" | "reverb") {
                lcd(&a, kind)?;
            }
            Ok(())
        })?;
        if disabled {
            k.class_list().add_1("disabled")?;
        }
        parent.append_child(&k)?;
    }
    Ok(())
}
#[derive(Default)]
struct Response {
    key: RefCell<String>,
    values: RefCell<Vec<f64>>,
    busy: Cell<bool>,
}
struct Chart {
    cv: HtmlCanvasElement,
    g: Canvas,
    app: JsValue,
    response: Rc<Response>,
    smooth: RefCell<Vec<f32>>,
    freqs: JsValue,
}
impl Chart {
    fn schedule(self: Rc<Self>) -> Result<(), JsValue> {
        let f = Closure::once_into_js(move |_: f64| {
            if self.cv.is_connected() {
                report(self.draw());
                report(self.schedule());
            }
        });
        web_sys::window()
            .unwrap()
            .request_animation_frame(f.unchecked_ref())?;
        Ok(())
    }
    fn draw(&self) -> Result<(), JsValue> {
        let g = &self.g;
        let w = self.cv.width() as f64;
        let h = self.cv.height() as f64;
        let x = |hz: f64| (hz / 20.).ln() / 1000f64.ln() * w;
        let y = |db: f64| h * 0.5 - db.clamp(-15., 15.) / 15. * (h * 0.5 - 18.);
        g.clear_rect(0., 0., w, h);
        g.set_font("18px ui-monospace, monospace");
        g.set_line_width(1.5);
        for db in [-12, -6, 0, 6, 12] {
            let at = y(db as f64);
            g.set_stroke_style_str(if db == 0 {
                "rgba(255,190,90,.45)"
            } else {
                "rgba(255,190,90,.12)"
            });
            g.begin_path();
            g.move_to(0., at);
            g.line_to(w, at);
            g.stroke();
            g.set_fill_style_str("rgba(255,200,120,.55)");
            g.fill_text(
                &if db > 0 {
                    format!("+{db}")
                } else {
                    db.to_string()
                },
                6.,
                at - 4.,
            )?;
        }
        for hz in [50, 100, 200, 500, 1000, 2000, 5000, 10000] {
            let at = x(hz as f64);
            g.set_stroke_style_str("rgba(255,190,90,.08)");
            g.begin_path();
            g.move_to(at, 0.);
            g.line_to(at, h);
            g.stroke();
            g.set_fill_style_str("rgba(255,200,120,.45)");
            g.fill_text(
                &if hz >= 1000 {
                    format!("{}k", hz / 1000)
                } else {
                    hz.to_string()
                },
                at + 4.,
                h - 6.,
            )?;
        }
        let engine = get(&self.app, "engine");
        if get(&self.app, "eqAnalyzer") != false {
            let spec = js_sys::Float32Array::new(&invoke(&engine, "spectrumDb", &[])?).to_vec();
            let n = spec.len().saturating_sub(1) * 2;
            let sr = num(&get(&engine, "ctx"), "sampleRate", 48000.);
            let mut smooth = self.smooth.borrow_mut();
            smooth.resize(spec.len(), -120.);
            g.begin_path();
            g.move_to(0., h);
            for (i, v) in spec.iter().enumerate().skip(2) {
                let hz = i as f64 * sr / n as f64;
                if hz < 20. {
                    continue;
                }
                if hz > 20000. {
                    break;
                }
                smooth[i] = (v + 6.).max(smooth[i] - 1.2);
                g.line_to(
                    x(hz),
                    h - (f64::from(smooth[i] + 100.) / 80.).clamp(0., 1.) * h,
                );
            }
            g.line_to(w, h);
            g.close_path();
            g.set_fill_style_str("rgba(120,200,255,.18)");
            g.fill();
        }
        let eq = get(&get(&get(&self.app, "store"), "state"), "eq");
        let key = js_sys::JSON::stringify(&eq)?
            .as_string()
            .unwrap_or_default();
        if *self.response.key.borrow() != key && !self.response.busy.replace(true) {
            let resp = self.response.clone();
            let freqs = self.freqs.clone();
            spawn_local(async move {
                let r: Result<Vec<f64>, JsValue> = async {
                    crate::read(resolved(invoke(&engine, "eqCurve", &[eq, freqs])?).await?)
                }
                .await;
                if let Ok(curve) = r {
                    resp.key.replace(key);
                    resp.values.replace(curve);
                }
                resp.busy.set(false);
            });
        }
        let vals = self.response.values.borrow();
        g.begin_path();
        if vals.is_empty() {
            g.move_to(0., y(0.));
            g.line_to(w, y(0.));
        } else {
            for (i, v) in vals.iter().enumerate() {
                let at = i as f64 / (vals.len() - 1).max(1) as f64 * w;
                if i == 0 {
                    g.move_to(at, y(*v));
                } else {
                    g.line_to(at, y(*v));
                }
            }
        }
        g.set_stroke_style_str("#ffb347");
        g.set_line_width(4.);
        g.set_shadow_color("#ffb347");
        g.set_shadow_blur(10.);
        g.stroke();
        g.set_shadow_blur(0.);
        Ok(())
    }
}
#[wasm_bindgen(js_name=renderRacks)]
pub fn render_racks(app: JsValue, selected: &str, stage: Element) -> Result<Element, JsValue> {
    stage.class_list().add_1("scene-rack")?;
    let wrap = make("div", "rack-stack");
    let s = store::state(&app)?;
    let g = &s["input"]["gate"];
    let gate = unit(
        &wrap,
        &app,
        "gate",
        "NOISE GATE",
        "GC-1 · INPUT",
        g["on"] == true,
        selected,
    )?;
    let led = child(&gate, "div", "ru-led-group", "");
    child(&led, "i", "gate-led", "");
    child(&led, "small", "", "OPEN");
    controls(
        &gate,
        &app,
        "gate",
        &[
            json!({"id":"threshold","name":"Threshold","min":-96,"max":-10,"def":-62,"unit":"дБ"}),
            json!({"id":"release","name":"Release","min":5,"max":1000,"def":80,"unit":"мс","log":true}),
        ],
        g,
    )?;
    let eq = unit(
        &wrap,
        &app,
        "eq",
        "POST EQ",
        "GC-7 · 7-BAND",
        s["eq"]["on"] != false,
        selected,
    )?;
    let screen = child(&eq, "div", "lcd", "");
    let cv: HtmlCanvasElement = child(&screen, "canvas", "eq-lcd", "").dyn_into()?;
    cv.set_width(1000);
    cv.set_height(340);
    let g = cv.get_context("2d")?.unwrap().dyn_into::<Canvas>()?;
    let freqs = (0..240)
        .map(|i| 20. * 1000f64.powf(i as f64 / 239.))
        .collect::<Vec<_>>();
    Rc::new(Chart {
        app: app.clone(),
        cv,
        g,
        response: Default::default(),
        smooth: Default::default(),
        freqs: crate::js(&freqs)?,
    })
    .schedule()?;
    let b = button(
        &screen,
        if get(&app, "eqAnalyzer") == false {
            "lcd-btn"
        } else {
            "lcd-btn on"
        },
        "",
        "АНАЛИЗАТОР",
    );
    on(&b, "click", &app, |a, e| {
        e.stop_propagation();
        let next = get(a, "eqAnalyzer") == false;
        set(a, "eqAnalyzer", &next.into());
        let b: Element = e.current_target().unwrap().unchecked_into();
        b.class_list().toggle_with_force("on", next)?;
        Ok(())
    });
    controls(
        &child(&eq, "div", "ru-knobs", ""),
        &app,
        "eq",
        list(&EQUIPMENT["params"]["eq"]),
        &s["eq"],
    )?;
    for (kind, title, sub) in [
        ("delay", "DELAY", "GC-D2 · ECHO"),
        ("reverb", "REVERB", "GC-R1 · HALL"),
    ] {
        let u = unit(
            &wrap,
            &app,
            kind,
            title,
            sub,
            s[kind]["on"] != false,
            selected,
        )?;
        let d = child(&child(&u, "div", "lcd small", ""), "div", "lcd-text", "");
        child(&d, "b", "", "");
        child(&d, "small", "", "");
        if kind == "delay" {
            let btns = child(&u, "div", "sync-btns", "");
            let current = s["delay"]["sync"].as_str().unwrap_or("off");
            for (id, tip) in [
                ("off", "Свободное время в мс"),
                ("1/4", "Четверть"),
                ("1/4.", "Пунктирная четверть"),
                ("1/8", "Восьмая"),
                ("1/8.", "Пунктирная восьмая"),
                ("1/8t", "Триоль восьмых"),
                ("1/16", "Шестнадцатая"),
            ] {
                let b = button(
                    &btns,
                    if current == id { "on" } else { "" },
                    tip,
                    &if id == "off" {
                        "FREE".to_owned()
                    } else {
                        id.to_uppercase()
                    },
                );
                attr(&b, "data-sync", id);
                on(&b, "click", &app, move |a, e| {
                    e.stop_propagation();
                    store::edit(
                        a,
                        &format!("Delay: синхронизация {id}"),
                        None,
                        |s| s["delay"]["sync"] = json!(id),
                    )?;
                    invoke(a, "renderStage", &[]).map(drop)
                });
            }
        }
        controls(
            &child(&u, "div", "ru-knobs", ""),
            &app,
            kind,
            list(&EQUIPMENT["params"][kind]),
            &s[kind],
        )?;
    }
    let sel = wrap.query_selector(&format!("[data-rack='{selected}']"))?;
    spawn_local(async move {
        let _ = sleep(0.).await;
        if let Some(el) = sel {
            if el.is_connected() {
                let opts = ScrollIntoViewOptions::new();
                opts.set_block(ScrollLogicalPosition::Nearest);
                el.scroll_into_view_with_scroll_into_view_options(&opts);
                report(lcd(&app, "delay"));
                report(lcd(&app, "reverb"));
            }
        }
    });
    Ok(wrap)
}
