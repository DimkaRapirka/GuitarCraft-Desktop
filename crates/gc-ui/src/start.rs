//! Entry point for the WebAssembly application loaded by the Tauri WebView.
use crate::{desktop::*, dom::*, host::resolved, js, preferences, Project};
use js_sys::Array;
use serde_json::json;
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::{Element, KeyboardEvent};
fn render() -> Result<(), JsValue> {
    for k in ["renderChain", "renderStage", "renderIO"] {
        call(k, &[])?;
    }
    Ok(())
}
fn on_state(kind: String, meta: JsValue) -> Result<(), JsValue> {
    let a = app();
    let source = string(&meta, "source");
    if get(&a, "comparing").is_truthy() && source != "ui" {
        call("compare", &[false.into()])?;
    }
    let state = get(&get(&a, "store"), "state");
    let settings = get(&a, "settings");
    let engine = get(&a, "engine");
    let bpm = num(&state, "tempo", 0.);
    if get(&meta, "preset").is_truthy()
        && bpm > 0.
        && get(&settings, "presetTempo") != false
        && bpm != num(&settings, "bpm", 120.)
    {
        let a = a.clone();
        task(async move {
            let s = preferences::save_settings(a.clone(), js(&json!({"bpm":bpm}))?).await?;
            let engine = get(&a, "engine");
            if get(&engine, "metronome").is_truthy() {
                invoke(
                    &engine,
                    "setMetronome",
                    &[true.into(), bpm.into(), get(&s, "metronomeLevel")],
                )?;
            }
            crate::transport::render_footer()
        });
    }
    invoke(
        &engine,
        if kind == "structure" {
            "build"
        } else {
            "applyParams"
        },
        &[state],
    )?;
    if source == "midi" {
        if !get(&a, "refreshQueued").is_truthy() {
            set(&a, "refreshQueued", &true.into());
            let frame = Closure::once_into_js(move |_: f64| {
                set(&a, "refreshQueued", &false.into());
                crate::editor::report(
                    render().and_then(|_| call("renderPresetBar", &[]).map(drop)),
                );
            });
            web_sys::window()
                .unwrap()
                .request_animation_frame(frame.unchecked_ref())?;
        }
        return Ok(());
    }
    if kind == "structure" || source != "ui" || get(&meta, "rerender").is_truthy() {
        render()?;
    }
    call("renderPresetBar", &[])?;
    let label = string(&meta, "label");
    if !source.is_empty() && source != "ui" && !label.is_empty() {
        call(
            "toast",
            &[
                if source == "mcp" {
                    format!("🤖 Агент {label}")
                } else {
                    label
                }
                .into(),
                source.into(),
            ],
        )?;
    }
    Ok(())
}
fn keyboard(e: KeyboardEvent) -> Result<(), JsValue> {
    let a = app();
    let modifier = e.meta_key() || e.ctrl_key();
    if e.key() == "Escape" && doc().query_selector(".catalog-bg")?.is_none() {
        let bgs = doc().query_selector_all(".modal-bg")?;
        if let Some(t) = bgs.length().checked_sub(1).and_then(|i| bgs.get(i)) {
            let el = t.unchecked_into::<Element>();
            if let Some(close) = el.query_selector(".close")? {
                close.unchecked_into::<web_sys::HtmlElement>().click();
            } else {
                el.remove();
            }
            return Ok(());
        }
    }
    if get(&a, "comparing").is_truthy() && modifier {
        return Ok(());
    }
    match e.key().to_lowercase().as_str() {
        "z" if modifier => {
            e.prevent_default();
            invoke(
                &get(&a, "store"),
                if e.shift_key() { "redo" } else { "undo" },
                &[],
            )?;
        }
        "s" if modifier => {
            e.prevent_default();
            call("savePreset", &[false.into()])?;
        }
        " " if e.target().is_some_and(|t| {
            doc()
                .body()
                .is_some_and(|b| JsValue::from(t) == JsValue::from(b))
        }) =>
        {
            e.prevent_default();
            if !get(&get(&a, "engine"), "liveNode").is_truthy() {
                call("toggleDI", &[])?;
            }
        }
        _ => {}
    }
    Ok(())
}
#[wasm_bindgen]
impl Desktop {
    #[wasm_bindgen(js_name=onSettings)]
    pub fn on_settings(&self, p: JsValue) -> Result<(), JsValue> {
        let a = app();
        if get(&p, "showAgentPanel") == false || get(&p, "mcpEnabled") == false {
            element("agentlog")?.class_list().add_1("hidden")?;
            get(&a, "root")
                .unchecked_into::<Element>()
                .class_list()
                .remove_1("agent-on")?;
        }
        if !get(&p, "bpm").is_undefined() {
            let e = get(&a, "engine");
            set(&e, "bpm", &get(&get(&a, "settings"), "bpm"));
            invoke(&e, "applyParams", &[get(&get(&a, "store"), "state")])?;
            if string(&a, "selected") == "delay" {
                self.render_stage()?;
            }
        }
        Ok(())
    }
}
#[wasm_bindgen]
pub async fn start() -> Result<(), JsValue> {
    let window = web_sys::window().unwrap();
    let a: JsValue = Desktop::new().into();
    set(&window, "__app", &a);
    let pr: JsValue = Project::new().into();
    set(&a, "catalog", &pr);
    let st: JsValue = crate::store::Store::new(pr.clone())?.into();
    set(&a, "store", &st);
    let s = preferences::load_settings(a.clone()).await?;
    crate::i18n::init(&string(&s, "language"));
    let e: JsValue = crate::engine::RustEngine::new(pr.clone())?.into();
    set(&a, "engine", &e);
    let cfg = js_sys::Object::new();
    set(&cfg, "mode", &"di".into());
    for (k, from) in [
        ("buffer", "bufferSize"),
        ("output", "outputDevice"),
        ("sampleRate", "sampleRate"),
    ] {
        set(&cfg, k, &get(&s, from));
    }
    if let Err(err) = resolved(invoke(&e, "init", &[cfg.into()])?).await {
        web_sys::console::warn_1(&err);
    }
    set(&a, "selected", &"amp".into());
    set(&a, "presets", &Array::new());
    let loads = Array::new();
    for k in ["loadUserPedals", "loadExtensions", "scanPlugins"] {
        loads.push(&invoke(&pr, k, &[])?);
    }
    if let Err(err) = resolved(js_sys::Promise::all(&loads).into()).await {
        web_sys::console::warn_1(&err);
    }
    crate::assets::prepare_assets().await?;
    let body = doc().body().unwrap();
    body.set_text_content(None);
    let root = child(&body, "div", "app", "");
    root.set_inner_html(&crate::i18n::template(include_str!("shell.html")));
    set(&a, "root", &root);
    crate::backgrounds::apply(&a).await?;
    let cb = Closure::<dyn Fn(String, JsValue)>::new(|kind, meta| {
        crate::editor::report(on_state(kind, meta))
    });
    invoke(&st, "subscribe", &[cb.into_js_value()])?;
    crate::presets::refresh().await?;
    if let Some(p) = Array::from(&get(&a, "presets"))
        .iter()
        .find(|p| string(p, "name") == "Crunch — Brit")
    {
        invoke(
            &st,
            "load",
            &[get(&p, "data"), get(&p, "file"), get(&p, "bank")],
        )?;
    } else {
        on_state("structure".into(), js(&json!({}))?)?;
    }
    invoke(&pr, "clear_history", &[])?;
    set(&a, "tuner", &crate::tuner::Tuner::new().into());
    set(&e, "bpm", &get(&s, "bpm"));
    set(&a, "midi", &crate::midi::Midi::new(a.clone()).into());
    set(&a, "demo", &crate::demo::Demo::new(a.clone()).into());
    let files = js(
        &json!([{"name":"Риффы — палм-мьют","url":"assets/di/di_riff.wav"},{"name":"Аккорды — открытые","url":"assets/di/di_chords.wav"},{"name":"Лид — фраза","url":"assets/di/di_lead.wav"},{"name":"Открытые струны","url":"assets/di/di_open_strings.wav"}]),
    )?;
    set(&a, "diList", &files);
    let first = Array::from(&files).get(0);
    if let Err(err) = resolved(invoke(
        &e,
        "loadDI",
        &[get(&first, "url"), get(&first, "name")],
    )?)
    .await
    {
        toast_error(&err);
    }
    preferences::apply_audio(a.clone()).await?;
    crate::transport::render_footer()?;
    crate::transport::tick([(0., 0.); 2])?;
    listen(&doc(), "knob-menu", |ev| {
        crate::editor::report(call("knobMenu", &[get(&ev, "detail")]).map(drop));
    });
    listen(&doc(), "pointerdown", |ev| {
        if let Some(t) = ev.target().and_then(|t| t.dyn_into::<Element>().ok()) {
            if t.closest(".menu,#preset-name,#btn-preset-more")
                .ok()
                .flatten()
                .is_none()
            {
                close_menu();
            }
        }
    });
    listen(&window, "keydown", |e| {
        crate::editor::report(keyboard(e.unchecked_into()))
    });
    crate::rpc::install(a).await
}
