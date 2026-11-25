//! The whole chain on one screen: every block in signal order, each editable in place —
//! pedals with their knobs, the amp panel, the cab mic mixer and the racks.
use crate::{
    dom::*,
    editor::{attr, button, on, pedal_element},
    host::sleep,
    store,
};
use gc_model::{
    catalog::{list, text},
    EQUIPMENT,
};
use serde_json::{json, Value};
use std::cell::Cell;
use wasm_bindgen::prelude::*;
use web_sys::Element;

pub(crate) const OVERVIEW: &str = "overview";
// 0 = fit the screen, otherwise a fixed zoom in percent.
thread_local! {static ZOOM:Cell<u32>=const{Cell::new(0)};}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}
fn zoom() -> u32 {
    ZOOM.with(Cell::get)
}
fn load_zoom() {
    thread_local! {static LOADED:Cell<bool>=const{Cell::new(false)};}
    if !LOADED.with(|l| l.replace(true)) {
        if let Some(z) = storage()
            .and_then(|s| s.get_item("gc.overviewZoom").ok().flatten())
            .and_then(|z| z.parse::<u32>().ok())
        {
            ZOOM.with(|c| c.set(z));
        }
    }
}
fn save_zoom(z: u32) {
    ZOOM.with(|c| c.set(z));
    if let Some(s) = storage() {
        let _ = s.set_item("gc.overviewZoom", &z.to_string());
    }
}
/// Chain-strip button that opens this view.
pub(crate) fn entry(app: &JsValue, root: &Element) {
    let on_now = string(app, "selected") == OVERVIEW;
    let b = button(
        root,
        if on_now { "chain-overview on" } else { "chain-overview" },
        "Вся цепочка на одном экране: педали, усилитель, кабинет и эффекты",
        "▦ Вся цепочка",
    );
    b.set_id("btn-overview");
    on(&b, "click", app, |a, _| invoke(a, "select", &[OVERVIEW.into()]).map(drop));
}
/// Small metal name plate with a jewel power lamp under a piece of gear.
fn plate(app: &JsValue, root: &Element, uid: &str, title: &str, active: bool) {
    let p = child(root, "div", "ov-plate", "");
    let lamp = button(&p, if active { "ov-jewel on" } else { "ov-jewel" }, "Вкл / выкл (bypass)", "");
    let id = uid.to_owned();
    on(&lamp, "click", app, move |a, _| invoke(a, "toggleBlock", &[id.clone().into()]).map(drop));
    child(&p, "b", "", title);
    let id = uid.to_owned();
    on(
        &button(&p, "ov-open", "Открыть полный редактор", "↗"),
        "click",
        app,
        move |a, _| invoke(a, "select", &[id.clone().into()]).map(drop),
    );
}
fn wire(root: &Element) {
    child(root, "i", "ov-wire", "");
}
fn amp(app: &JsValue, root: &Element, s: &Value) -> Result<(), JsValue> {
    let ch = text(&s["amp"], "channel").to_owned();
    let gear = child(root, "div", "ov-gear ov-amp", "");
    attr(&gear, "data-ov", "amp");
    let wrap = child(&gear, "div", "ov-amp-skin", "");
    let (a, w) = (app.clone(), wrap.clone());
    wasm_bindgen_futures::spawn_local(async move {
        crate::editor::report(crate::skins::render_mini(a, w, 620.).await);
    });
    let name = format!("{} · {}", crate::i18n::text("Усилитель"), text(&EQUIPMENT["channels"][&ch], "name"));
    plate(app, &gear, "amp", &name, s["amp"]["on"] != false);
    Ok(())
}
/// A rack unit like on the rack scene: faceplate, name, knobs and power switch.
fn rack(app: &JsValue, stack: &Element, s: &Value, kind: &'static str) -> Result<(), JsValue> {
    let (title, sub, model, params, active) = match kind {
        "gate" => (
            "NOISE GATE",
            "GC-1 · INPUT",
            &s["input"]["gate"],
            vec![
                json!({"id":"threshold","name":"Threshold","min":-96,"max":-10,"def":-62,"unit":"дБ"}),
                json!({"id":"release","name":"Release","min":5,"max":1000,"def":80,"unit":"мс","log":true}),
            ],
            s["input"]["gate"]["on"] == true,
        ),
        _ => (
            match kind {
                "eq" => "POST EQ",
                "delay" => "ECHO",
                _ => "REVERB",
            },
            match kind {
                "eq" => "GC-7 · 7-BAND",
                "delay" => "GC-D · DELAY",
                _ => "GC-R · HALL / PLATE",
            },
            &s[kind],
            list(&EQUIPMENT["params"][kind]).to_vec(),
            s[kind]["on"] != false,
        ),
    };
    let unit = child(stack, "div", &format!("rack-unit u-{kind} ov-rack {}", if active { "on" } else { "off" }), "");
    attr(&unit, "data-ov", kind);
    let face = child(&unit, "div", "ru-face", "");
    let brand = child(&face, "div", "ru-brand", "");
    child(&brand, "b", "", title);
    child(&brand, "small", "", sub);
    let open = button(&brand, "ov-open", "Открыть полный редактор", "↗");
    on(&open, "click", app, move |a, _| invoke(a, "select", &[kind.into()]).map(drop));
    let power = button(&face, if active { "ru-power on" } else { "ru-power" }, "Питание (bypass)", "");
    child(&power, "i", "", "");
    child(&power, "span", "", if active { "ON" } else { "OFF" });
    on(&power, "click", app, move |a, _| invoke(a, "toggleBlock", &[kind.into()]).map(drop));
    let knobs = child(&child(&face, "div", "ru-content", ""), "div", "ru-knobs", "");
    crate::racks::controls_sized(&knobs, app, kind, &params, model, 42.)
}
fn stack(root: &Element) -> Element {
    child(root, "div", "rack-stack ov-stack", "")
}
#[wasm_bindgen(js_name=renderOverview)]
pub fn render_overview(app: JsValue, stage: Element) -> Result<Element, JsValue> {
    load_zoom();
    stage.class_list().add_1("scene-board")?;
    stage.class_list().add_1("scene-overview")?;
    let s = store::state(&app)?;
    let root = make("div", "overview");
    let bar = child(&root, "div", "ov-bar", "");
    child(&bar, "b", "", "Вся цепочка");
    child(&bar, "span", "muted", "Сигнал идёт слева направо и сверху вниз. Всё редактируется прямо здесь.");
    child(&bar, "div", "grow", "");
    let z = zoom();
    let fit = button(&bar, if z == 0 { "chip on" } else { "chip" }, "Уместить всё на экране", "Вместить");
    on(&fit, "click", &app, |a, _| {
        save_zoom(0);
        invoke(a, "renderStage", &[]).map(drop)
    });
    for pct in [60u32, 80, 100] {
        let b = button(&bar, if z == pct { "chip on" } else { "chip" }, "", &format!("{pct}%"));
        on(&b, "click", &app, move |a, _| {
            save_zoom(pct);
            invoke(a, "renderStage", &[]).map(drop)
        });
    }
    let flow = child(&root, "div", "ov-flow", "");
    rack(&app, &stack(&flow), &s, "gate")?;
    let mut open_stack: Option<Element> = None;
    for b in list(&s["chain"]) {
        let kind = text(b, "kind");
        if !matches!(kind, "eq" | "delay" | "reverb") {
            open_stack = None;
        }
        match kind {
            "pedal" => {
                wire(&flow);
                let def = invoke(&get(&app, "catalog"), "get", &[text(b, "pedal").into()])?;
                let slot = child(&flow, "div", "ov-pedal", "");
                attr(&slot, "data-ov", text(b, "uid"));
                let pedal = pedal_element(&app, b, &def, "md")?;
                slot.append_child(&pedal)?;
                let id = text(b, "uid").to_owned();
                on(
                    &button(&slot, "ov-open", "Открыть на педалборде", "↗"),
                    "click",
                    &app,
                    move |a, _| invoke(a, "select", &[id.clone().into()]).map(drop),
                );
            }
            "amp" => {
                wire(&flow);
                amp(&app, &flow, &s)?
            }
            "cab" => {
                wire(&flow);
                let gear = child(&flow, "div", "ov-gear ov-cab", "");
                attr(&gear, "data-ov", "cab");
                crate::cab::compact(&app, &gear)?;
                plate(&app, &gear, "cab", crate::i18n::text("Кабинет и микрофоны"), s["cab"]["on"] != false);
            }
            k @ ("eq" | "delay" | "reverb") => {
                let kind: &'static str = match k {
                    "eq" => "eq",
                    "delay" => "delay",
                    _ => "reverb",
                };
                let st = match &open_stack {
                    Some(st) => st.clone(),
                    None => {
                        wire(&flow);
                        let st = stack(&flow);
                        open_stack = Some(st.clone());
                        st
                    }
                };
                rack(&app, &st, &s, kind)?
            }
            _ => {}
        }
    }
    let flow2 = flow.clone();
    let stage2 = stage.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let _ = sleep(0.).await;
        fit_zoom(&stage2, &flow2, z);
    });
    Ok(root)
}
/// Scale the flow so it fits the stage (zoom 0) or to the chosen percentage.
fn fit_zoom(stage: &Element, flow: &Element, z: u32) {
    if z != 0 {
        style(flow, "zoom", &format!("{}", z as f64 / 100.));
        return;
    }
    style(flow, "zoom", "1");
    let avail_h = stage.client_height() as f64 - 90.;
    let (w, h) = (flow.scroll_width() as f64, flow.scroll_height() as f64);
    if w <= 0. || h <= 0. {
        return;
    }
    // The flow wraps to the stage width: a smaller zoom fits more per row, so search for the
    // largest zoom whose wrapped height still fits.
    let fits = |k: f64| {
        style(flow, "zoom", &format!("{k:.3}"));
        flow.scroll_height() as f64 * k <= avail_h
    };
    if fits(1.) {
        return;
    }
    let (mut lo, mut hi) = (0.35, 1.);
    for _ in 0..8 {
        let mid = (lo + hi) / 2.;
        if fits(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let k = lo;
    style(flow, "zoom", &format!("{:.3}", k.max(0.4)));
}
