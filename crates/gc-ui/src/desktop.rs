//! Collaboration between the UI surfaces of the single desktop window.
use crate::{dom::*, editor::attr, host, js};
use gc_model::catalog::{list, text};
use serde_json::{json, Value};
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::Element;

#[wasm_bindgen]
pub struct Desktop;
pub fn app() -> JsValue {
    get(&web_sys::window().unwrap(), "__app")
}
pub fn element(id: &str) -> Result<Element, JsValue> {
    doc()
        .get_element_by_id(id)
        .ok_or_else(|| crate::err(format!("Нет элемента {id}")))
}
pub fn toast_error(e: &JsValue) {
    let message = get(e, "message")
        .as_string()
        .or_else(|| e.as_string())
        .unwrap_or_else(|| "Ошибка GuitarCraft".into());
    if element("toasts").is_ok() {
        let _ = invoke(&app(), "toast", &[message.into()]);
    }
    web_sys::console::error_1(e);
}
pub fn task(f: impl std::future::Future<Output = Result<(), JsValue>> + 'static) {
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = f.await {
            toast_error(&e);
        }
    });
}
pub fn call(method: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
    invoke(&app(), method, args)
}
pub fn id_button(root: &Element, id: &str, class: &str, title: &str, label: &str) -> Element {
    let e = crate::editor::button(root, class, title, label);
    if !id.is_empty() {
        e.set_id(id);
    }
    e
}
pub fn menu(x: f64, y: f64, class: &str, width: Option<f64>) -> Result<Element, JsValue> {
    close_menu();
    let m = make("div", class);
    style(&m, "left", &format!("{x}px"));
    style(&m, "top", &format!("{y}px"));
    if let Some(w) = width {
        style(&m, "width", &format!("{w}px"));
    }
    doc().body().unwrap().append_child(&m)?;
    Ok(m)
}
pub fn close_menu() {
    if let Ok(Some(e)) = doc().query_selector(".menu") {
        e.remove();
    }
}
pub fn menu_item(
    m: &Element,
    label: &str,
    disabled: bool,
    f: impl Fn() -> Result<(), JsValue> + 'static,
) {
    let e = child(
        m,
        "div",
        if disabled {
            "menu-item dis"
        } else {
            "menu-item"
        },
        label,
    );
    if !disabled {
        listen(&e, "click", move |_| {
            close_menu();
            if let Err(e) = f() {
                toast_error(&e);
            }
        });
    }
}
pub fn changed(kind: &str, label: &str, source: &str) -> Result<(), JsValue> {
    invoke(
        &get(&app(), "store"),
        "changed",
        &[kind.into(), js(&json!({"label":label,"source":source}))?],
    )
    .map(drop)
}
pub fn model(
    method: &str,
    p: Value,
    source: Option<String>,
    label: impl FnOnce(&JsValue) -> String,
) -> Result<JsValue, JsValue> {
    let app = app();
    let src = source.as_deref().unwrap_or("ui");
    let r = invoke(
        &get(&app, "catalog"),
        "chain",
        &[method.into(), js(&p)?, src.into()],
    )?;
    if method == "chain.add" {
        set(&app, "selected", &get(&r, "added"));
    } else if method == "chain.remove" && p["target"] == string(&app, "selected") {
        // Store still has the order before removal until changed() refreshes it.
        let state = crate::store::state(&app)?;
        let chain = list(&state["chain"]);
        let selected = chain
            .iter()
            .position(|block| block["uid"] == p["target"])
            .and_then(|index| {
                chain[index + 1..]
                    .iter()
                    .chain(chain[..index].iter().rev())
                    .find(|block| block["kind"] == "pedal")
            })
            .map_or(crate::editor::PEDALBOARD, |block| text(block, "uid"));
        set(&app, "selected", &selected.into());
    }
    changed("structure", &label(&r), src)?;
    Ok(r)
}
pub fn checkbox(el: &Element, key: &str, on: bool) {
    if on {
        attr(el, key, "");
    } else {
        let _ = el.remove_attribute(key);
    }
}
#[wasm_bindgen]
impl Desktop {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Desktop {
        Desktop
    }
    #[wasm_bindgen(js_name=openBuilder)]
    pub fn open_builder(&self) -> Result<(), JsValue> {
        crate::builder::open_builder(app(), JsValue::UNDEFINED)
    }
    #[wasm_bindgen(js_name=renderStage)]
    pub fn render_stage(&self) -> Result<(), JsValue> {
        crate::editor::render_editor(app(), element("stage")?)
    }
    pub fn select(&self, id: &str) -> Result<(), JsValue> {
        set(&app(), "selected", &id.into());
        crate::chain::render()?;
        self.render_stage()
    }
    #[wasm_bindgen(js_name=renderChain)]
    pub fn render_chain(&self) -> Result<(), JsValue> {
        crate::chain::render()
    }
    #[wasm_bindgen(js_name=addPedal)]
    pub fn add_pedal(
        &self,
        id: String,
        index: i32,
        params: JsValue,
        source: Option<String>,
    ) -> Result<JsValue, JsValue> {
        let params: Option<Value> = crate::read(params)?;
        let d = invoke(&get(&app(), "catalog"), "get", &[id.clone().into()])?;
        Ok(get(
            &model(
                "chain.add",
                json!({"pedal":id,"index":index,"params":params}),
                source,
                |_| format!("Добавлено: {}", string(&d, "name")),
            )?,
            "added",
        ))
    }
    #[wasm_bindgen(js_name=replacePedal)]
    pub fn replace_pedal(
        &self,
        uid: String,
        id: String,
        source: Option<String>,
        params: JsValue,
    ) -> Result<String, JsValue> {
        let params: Option<Value> = crate::read(params)?;
        let d = invoke(&get(&app(), "catalog"), "get", &[id.clone().into()])?;
        model(
            "chain.replace",
            json!({"target":uid,"pedal":id,"params":params}),
            source,
            |_| format!("Замена: {}", string(&d, "name")),
        )?;
        Ok(uid)
    }
    #[wasm_bindgen(js_name=removePedal)]
    pub fn remove_pedal(&self, uid: &str, source: Option<String>) -> Result<(), JsValue> {
        model("chain.remove", json!({"target":uid}), source, |_| {
            "Педаль удалена".into()
        })
        .map(drop)
    }
    #[wasm_bindgen(js_name=moveBlock)]
    pub fn move_block(&self, uid: &str, index: i32, source: Option<String>) -> Result<(), JsValue> {
        model(
            "chain.move",
            json!({"target":uid,"index":index}),
            source,
            |_| "Блок перемещён".into(),
        )
        .map(drop)
    }
    #[wasm_bindgen(js_name=toggleBlock)]
    pub fn toggle_block(&self, uid: &str, source: Option<String>) -> Result<(), JsValue> {
        model("chain.toggle", json!({"target":uid}), source, |r| {
            format!(
                "{}: {uid}",
                if get(r, "on").is_truthy() {
                    "Включено"
                } else {
                    "Выключено"
                }
            )
        })
        .map(drop)
    }
    #[wasm_bindgen(js_name=startAudition)]
    pub fn start_audition(
        &self,
        id: &str,
        index: i32,
        replace: Option<String>,
        params: JsValue,
        def: JsValue,
    ) -> Result<bool, JsValue> {
        let a = app();
        let cat = get(&a, "catalog");
        let d = invoke(&cat, "get", &[id.into()])?;
        if !get(&d, "supported").is_truthy() {
            return Ok(false);
        }
        let audition = js(
            &json!({"pedalId":id,"index":index,"replaceUid":replace,"active":true,"params":{}}),
        )?;
        if params.is_object() {
            set(&audition, "params", &js(&crate::read::<Value>(params)?)?);
        }
        set(
            &audition,
            "def",
            &if def.is_object() {
                def
            } else if string(&d, "source") == "user" {
                get(&d, "json")
            } else {
                JsValue::NULL
            },
        );
        let engine = get(&a, "engine");
        set(&engine, "audition", &audition);
        audition_changed()?;
        if !get(&engine, "diPlaying").is_truthy() && !get(&engine, "liveNode").is_truthy() {
            call("toggleDI", &[true.into()])?;
        }
        Ok(true)
    }
    #[wasm_bindgen(js_name=setAuditionActive)]
    pub fn set_active(&self, on: bool) -> Result<(), JsValue> {
        let a = get(&get(&app(), "engine"), "audition");
        if a.is_object() {
            set(&a, "active", &on.into());
            audition_changed()?;
        }
        Ok(())
    }
    #[wasm_bindgen(js_name=stopAudition)]
    pub fn stop_audition(&self) -> Result<(), JsValue> {
        let engine = get(&app(), "engine");
        if get(&engine, "audition").is_object() {
            set(&engine, "audition", &JsValue::NULL);
            audition_changed()?;
        }
        Ok(())
    }
    #[wasm_bindgen(js_name=blockMenu)]
    pub fn block_menu(&self, b: JsValue, e: JsValue) -> Result<(), JsValue> {
        crate::chain::menu_for(&b, num(&e, "clientX", 0.), num(&e, "clientY", 0.))
    }
    pub fn toast(&self, msg: String, source: Option<String>) -> Result<(), JsValue> {
        let t = child(
            &element("toasts")?,
            "div",
            &format!("toast {}", source.as_deref().unwrap_or("ui")),
            &msg,
        );
        task(async move {
            host::sleep(2600.).await?;
            t.class_list().add_1("out")?;
            host::sleep(600.).await?;
            t.remove();
            Ok(())
        });
        Ok(())
    }
    #[wasm_bindgen(js_name=logAgent)]
    pub fn log_agent(&self, line: String, kind: Option<String>) -> Result<(), JsValue> {
        let a = app();
        let settings = get(&a, "settings");
        if !get(&settings, "mcpEnabled").is_truthy()
            || !get(&settings, "showAgentPanel").is_truthy()
        {
            return Ok(());
        }
        let panel = element("agentlog")?;
        panel.class_list().remove_2("idle", "hidden")?;
        get(&a, "root")
            .unchecked_into::<Element>()
            .class_list()
            .add_1("agent-on")?;
        let body = element("agentlog-body")?;
        child(
            &body,
            "div",
            &format!("al {}", kind.as_deref().unwrap_or("call")),
            &line,
        );
        body.set_scroll_top(body.scroll_height());
        let token = num(&a, "logGeneration", 0.) + 1.;
        set(&a, "logGeneration", &token.into());
        task(async move {
            host::sleep(8000.).await?;
            if num(&a, "logGeneration", 0.) == token {
                panel.class_list().add_1("idle")?;
            }
            Ok(())
        });
        Ok(())
    }
    #[wasm_bindgen(js_name=knobMenu)]
    pub fn knob_menu(&self, detail: JsValue) -> Result<(), JsValue> {
        let id = string(&detail, "id");
        if id.is_empty() {
            return Ok(());
        }
        let m = menu(num(&detail, "x", 0.), num(&detail, "y", 0.), "menu", None)?;
        let midi = get(&app(), "midi");
        let name = get(&get(&detail, "param"), "name")
            .as_string()
            .unwrap_or_else(|| id.clone());
        child(&m, "div", "menu-head", &name);
        let map = invoke(&midi, "mappingFor", &[id.clone().into()])?;
        let target = js(&json!({"type":"param","id":id,"name":name}))?;
        let mm = midi.clone();
        menu_item(&m, "🎛 MIDI Learn", false, move || {
            invoke(&mm, "learn", &[target.clone()]).map(drop)
        });
        if map.is_object() {
            menu_item(
                &m,
                &format!("Убрать MIDI ({})", string(&map, "label")),
                false,
                move || invoke(&midi, "unmap", &[id.clone().into()]).map(drop),
            );
        }
        Ok(())
    }
}
fn audition_changed() -> Result<(), JsValue> {
    let a = app();
    invoke(
        &get(&a, "engine"),
        "build",
        &[get(&get(&a, "store"), "state")],
    )?;
    crate::chain::render()
}
