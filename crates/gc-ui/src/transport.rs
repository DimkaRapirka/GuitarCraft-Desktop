//! Input/output strips, DI transport, tempo controls and meter animation.
use crate::{
    desktop::*,
    dom::*,
    editor::{attr, knob_for, on},
    host::{command, now, resolved},
    js,
    preferences::{apply_audio, save_settings},
    store,
};
use js_sys::Array;
use serde_json::{json, Value};
use std::cell::RefCell;
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::Element;
thread_local! {static TAPS:RefCell<Vec<f64>>=const{RefCell::new(Vec::new())};}
fn meter(p: &Element, id: &str) -> Result<(), JsValue> {
    let m = child(p, "div", "meter2", "");
    m.set_id(id);
    attr(&m, "title", "Пик. Клик: сбросить индикатор перегрузки");
    listen(&m, "click", |e| {
        let el: Element = e.current_target().unwrap().unchecked_into();
        let _ = el.class_list().remove_1("clipped");
    });
    let scale = child(&m, "div", "m-scale", "");
    for label in ["0", "-6", "-18", "-36"] {
        child(&scale, "span", "", label);
    }
    let bar = child(&m, "div", "m-bar", "");
    child(&bar, "i", "", "");
    child(&bar, "b", "", "");
    child(&m, "div", "m-clip", "");
    Ok(())
}
fn dial(
    p: &Element,
    s: &Value,
    path: &'static str,
    key: &'static str,
    label: &'static str,
    id: &str,
    lo: f64,
    hi: f64,
    default: f64,
    unit: &str,
    name: &'static str,
) -> Result<(), JsValue> {
    let v = s.pointer(path).and_then(Value::as_f64).unwrap_or(default);
    let mut param = json!({"id":key,"name":label,"min":lo,"max":hi,"def":default,"unit":unit});
    if unit == "st" {
        param["step"] = json!(1);
    }
    let k = knob_for(param, v, id, 32., "dark", move |v| {
        store::edit(&app(), name, Some(path), |s| {
            if let Some(x) = s.pointer_mut(path) {
                *x = json!(v);
            }
        })
    })?;
    p.append_child(&k)?;
    Ok(())
}
fn group(
    p: &Element,
    on_val: bool,
    id: &str,
    label: &str,
    click: impl Fn() -> Result<(), JsValue> + 'static,
) -> Result<Element, JsValue> {
    let g = child(
        p,
        "div",
        if on_val { "io-group on" } else { "io-group" },
        "",
    );
    let b = id_button(
        &g,
        id,
        if on_val { "io-toggle on" } else { "io-toggle" },
        "",
        "",
    );
    child(&b, "i", "", "");
    b.append_child(&doc().create_text_node(label))?;
    listen(&b, "click", move |_| crate::editor::report(click()));
    Ok(g)
}
pub fn render_io() -> Result<(), JsValue> {
    let s = store::state(&app())?;
    let input = element("io-in")?;
    let output = element("io-out")?;
    input.set_text_content(None);
    output.set_text_content(None);
    meter(&input, "meter-in")?;
    dial(
        &input,
        &s,
        "/input/gain",
        "in",
        "INPUT",
        "input.gain",
        -24.,
        24.,
        0.,
        "дБ",
        "Вход",
    )?;
    let gate = group(
        &input,
        s["input"]["gate"]["on"] == true,
        "gate-toggle",
        "GATE",
        || call("toggleBlock", &["gate".into()]).map(drop),
    )?;
    dial(
        &gate,
        &s,
        "/input/gate/threshold",
        "thr",
        "THRESH",
        "gate.threshold",
        -96.,
        -10.,
        -62.,
        "дБ",
        "Порог гейта",
    )?;
    dial(
        &input,
        &s,
        "/input/transpose",
        "tr",
        "TRANSPOSE",
        "input.transpose",
        -12.,
        12.,
        0.,
        "st",
        "Транспонирование",
    )?;
    let dbl = group(
        &output,
        s["output"]["doubler"]["on"] == true,
        "doubler-toggle",
        "DOUBLER",
        || {
            store::edit(&app(), "Doubler", None, |st| {
                let dbl = &mut st["output"]["doubler"];
                if !dbl.is_object() {
                    *dbl = json!({"spread":12});
                }
                dbl["on"] = json!(dbl["on"] != true);
            })?;
            render_io()
        },
    )?;
    dial(
        &dbl,
        &s,
        "/output/doubler/spread",
        "spread",
        "SPREAD",
        "output.doubler",
        3.,
        35.,
        12.,
        "мс",
        "Doubler spread",
    )?;
    dial(
        &output,
        &s,
        "/output/gain",
        "out",
        "OUTPUT",
        "output.gain",
        -36.,
        12.,
        0.,
        "дБ",
        "Выход",
    )?;
    meter(&output, "meter-out")
}
fn option(el: &Element, v: &str, label: &str, active: bool) {
    let o = child(el, "option", "", label);
    attr(&o, "value", v);
    if active {
        attr(&o, "selected", "");
    }
}
fn metro(engine: &JsValue, on: bool, settings: &JsValue) -> Result<(), JsValue> {
    invoke(
        engine,
        "setMetronome",
        &[
            on.into(),
            get(settings, "bpm"),
            get(settings, "metronomeLevel"),
        ],
    )
    .map(drop)
}
async fn tempo(bpm: f64) -> Result<(), JsValue> {
    let app = app();
    let s = save_settings(app.clone(), js(&json!({"bpm":bpm}))?).await?;
    let engine = get(&app, "engine");
    if get(&engine, "metronome").is_truthy() {
        metro(&engine, true, &s)?;
    }
    if let Some(el) = doc().get_element_by_id("bpm") {
        set(&el, "value", &num(&s, "bpm", 120.).to_string().into());
    }
    Ok(())
}
async fn select_di(v: String) -> Result<(), JsValue> {
    let a = app();
    let engine = get(&a, "engine");
    let files = Array::from(&get(&a, "diList"));
    if v == "__file" {
        let f=command("file_open",&js(&json!({"title":crate::i18n::text("DI-запись"),"filters":[{"name":"Audio","extensions":["wav","mp3","flac","ogg","m4a"]}]}))?).await?;
        if let Some(file) = f.as_string() {
            resolved(invoke(&engine, "loadDIPath", &[file.clone().into()])?).await?;
            get(&a, "diList").unchecked_into::<Array>().push(&js(
                &json!({"name":file.rsplit(['/',char::from(92)]).next(),"url":null,"file":file}),
            )?);
            invoke(&engine, "playDI", &[])?;
        }
    } else if let Ok(i) = v.parse::<u32>() {
        let d = files.get(i);
        if d.is_object() {
            if get(&d, "file").is_truthy() {
                resolved(invoke(&engine, "loadDIPath", &[get(&d, "file")])?).await?;
            } else {
                resolved(invoke(
                    &engine,
                    "loadDI",
                    &[get(&d, "url"), get(&d, "name")],
                )?)
                .await?;
            }
            invoke(&engine, "playDI", &[])?;
        }
    }
    render_footer()
}
pub fn render_footer() -> Result<(), JsValue> {
    let a = app();
    let f = element("footer")?;
    f.set_text_content(None);
    let engine = get(&a, "engine");
    let st = get(&a, "settings");
    let live = get(&engine, "liveNode").is_truthy();
    let switch = child(&f, "div", "src-switch", "");
    attr(&switch, "title", "Источник сигнала");
    for (mode, id, label) in [("live", "src-guitar", "ГИТАРА"), ("di", "src-di", "DI")] {
        let b = id_button(
            &switch,
            id,
            if live == (mode == "live") { "on" } else { "" },
            "",
            label,
        );
        listen(&b, "click", move |_| {
            task(async move {
                let a = app();
                save_settings(a.clone(), js(&json!({"inputMode":mode}))?).await?;
                if mode == "live" {
                    apply_audio(a).await?;
                    render_footer()
                } else {
                    invoke(&get(&a, "engine"), "stopLive", &[])?;
                    call("toggleDI", &[true.into()]).map(drop)
                }
            })
        });
    }
    if live {
        let b = id_button(&f, "btn-input", "src-live", "Вход: настройки аудио", "");
        child(&b, "i", "live-dot", "");
        let info = get(&engine, "liveInfo");
        let label = string(&info, "label");
        let label = if label.is_empty() {
            crate::i18n::text("аудиоинтерфейс").to_owned()
        } else {
            label.chars().take(28).collect()
        };
        let channel = (num(&info, "channel", 0.) + 1.).to_string();
        b.append_child(&doc().create_text_node(&crate::i18n::message(
            "Гитара · {} · вход {}",
            &[&label, &channel],
        )))?;
        on(&b, "click", &a, |a, _| {
            crate::settings::open_settings(a.clone(), Some("audio".into()))
        });
    } else {
        let d = child(&f, "div", "di-player", "");
        let playing = get(&engine, "diPlaying").is_truthy();
        let b = id_button(
            &d,
            "btn-play",
            if playing { "play on" } else { "play" },
            "Пробел",
            if playing { "■" } else { "▶" },
        );
        on(&b, "click", &a, |a, _| invoke(a, "toggleDI", &[]).map(drop));
        child(&d, "span", "di-label", "DI");
        let sel = child(&d, "select", "", "");
        sel.set_id("di-select");
        for (i, v) in Array::from(&get(&a, "diList")).iter().enumerate() {
            let name = string(&v, "name");
            option(
                &sel,
                &i.to_string(),
                &name,
                name == string(&engine, "diName"),
            );
        }
        option(&sel, "__file", "Свой файл…", false);
        listen(&sel, "change", |e| {
            task(select_di(string(&e.current_target().unwrap(), "value")))
        });
    }
    child(&f, "div", "sep", "");
    let tuner = get(&a, "tuner");
    let b = id_button(
        &f,
        "btn-tuner",
        if get(&tuner, "open").is_truthy() {
            "fbtn on"
        } else {
            "fbtn"
        },
        "",
        "ТЮНЕР ",
    );
    child(&b, "span", "", "—").set_id("mini-tuner");
    on(&b, "click", &a, |a, _| {
        invoke(&get(a, "tuner"), "toggle", &[]).map(drop)
    });
    child(&f, "div", "sep", "");
    on(
        &id_button(&f, "btn-tap", "fbtn", "", "TAP"),
        "click",
        &a,
        |a, _| invoke(a, "tap", &[]).map(drop),
    );
    let label = child(&f, "label", "bpm-box", "");
    let bpm = child(&label, "input", "bpm", "");
    bpm.set_id("bpm");
    for (k, v) in [("type", "number"), ("min", "30"), ("max", "300")] {
        attr(&bpm, k, v);
    }
    set(&bpm, "value", &num(&st, "bpm", 120.).to_string().into());
    child(&label, "span", "", "BPM");
    listen(&bpm, "change", |e| {
        task(tempo(
            string(&e.current_target().unwrap(), "value")
                .parse::<f64>()
                .unwrap_or(120.)
                .clamp(30., 300.),
        ))
    });
    let on_metro = get(&engine, "metronome").is_truthy();
    let b = id_button(
        &f,
        "btn-metronome",
        if on_metro { "fbtn on" } else { "fbtn" },
        "",
        "МЕТРОНОМ",
    );
    on(&b, "click", &a, |a, _| {
        let e = get(a, "engine");
        metro(&e, !get(&e, "metronome").is_truthy(), &get(a, "settings"))?;
        render_footer()
    });
    child(&f, "div", "grow", "");
    let b = id_button(
        &f,
        "btn-latency",
        "lat",
        "Оценка задержки. Открыть настройки аудио",
        &crate::i18n::message(
            "{} кГц · ≈{} мс",
            &[
                &((num(&get(&engine, "ctx"), "sampleRate", 48000.) / 100.).round() / 10.)
                    .to_string(),
                &format!("{:.0}", num(&engine, "latencyMs", 0.)),
            ],
        ),
    );
    on(&b, "click", &a, |a, _| {
        crate::settings::open_settings(a.clone(), None)
    });
    child(&f, "div", "sep", "");
    on(
        &id_button(&f, "btn-settings", "fbtn", "", "⚙ НАСТРОЙКИ"),
        "click",
        &a,
        |a, _| crate::settings::open_settings(a.clone(), None),
    );
    Ok(())
}
pub fn tick(mut hold: [(f64, f64); 2]) -> Result<(), JsValue> {
    let engine = get(&app(), "engine");
    let pos = |p: f64| {
        format!(
            "{}%",
            ((20. * (p + 1e-6).log10() + 48.) / 48. * 100.).clamp(0., 100.)
        )
    };
    for (i, (id, port)) in [("meter-in", "input"), ("meter-out", "output")]
        .iter()
        .enumerate()
    {
        if let Some(m) = doc().get_element_by_id(id) {
            let v = invoke(&engine, "peak", &[(*port).into()])?
                .as_f64()
                .unwrap_or_default();
            let t = now();
            if v >= hold[i].0 || t - hold[i].1 > 1500. {
                hold[i] = (v, t);
            }
            if let Some(bar) = m.query_selector(".m-bar i")? {
                style(&bar, "height", &pos(v));
            }
            if let Some(line) = m.query_selector(".m-bar b")? {
                style(&line, "bottom", &pos(hold[i].0));
            }
            if v > 0.98 {
                m.class_list().add_1("clipped")?;
            }
        }
    }
    let open = get(&get(&get(&engine, "gate"), "status"), "open").is_truthy();
    for (sel, class, on) in [
        (".block.gate", "closed", !open),
        (".gate-led", "open", open),
    ] {
        if let Some(el) = doc().query_selector(sel)? {
            el.class_list().toggle_with_force(class, on)?;
        }
    }
    let frame = Closure::once_into_js(move |_: f64| crate::editor::report(tick(hold)));
    web_sys::window()
        .unwrap()
        .request_animation_frame(frame.unchecked_ref())?;
    Ok(())
}
#[wasm_bindgen]
impl Desktop {
    #[wasm_bindgen(js_name=renderIO)]
    pub fn render_io(&self) -> Result<(), JsValue> {
        render_io()
    }
    #[wasm_bindgen(js_name=renderFooter)]
    pub fn render_footer(&self) -> Result<(), JsValue> {
        render_footer()
    }
    #[wasm_bindgen(js_name=toggleDI)]
    pub fn toggle_di(&self, force: Option<bool>) -> Result<(), JsValue> {
        let e = get(&app(), "engine");
        if force.unwrap_or_else(|| !get(&e, "diPlaying").is_truthy()) {
            invoke(&e, "stopLive", &[])?;
            invoke(&e, "playDI", &[])?;
        } else {
            invoke(&e, "stopDI", &[])?;
        }
        render_footer()
    }
    pub fn tap(&self) {
        TAPS.with(|ts| {
            let t = now();
            let mut ts = ts.borrow_mut();
            ts.retain(|p| t - p < 2500.);
            ts.push(t);
            if ts.len() > 1 {
                let delta = (t - ts[0]) / (ts.len() - 1) as f64;
                if delta > 0. {
                    task(tempo((60000. / delta).round().clamp(30., 300.)));
                }
            }
        })
    }
}
