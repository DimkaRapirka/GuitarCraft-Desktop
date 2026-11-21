//! DSP-block pedal editor. A draft and its audition parameters live for the lifetime of the modal.
use crate::{
    dom::*,
    host::{command, resolved},
    js,
    pedal::pedal_visual,
    read,
};
use gc_model::{
    catalog::{compile_pedal, list, text},
    EQUIPMENT,
};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::{Element, Event};
thread_local! {static OPEN:RefCell<Option<Rc<Builder>>>=const{RefCell::new(None)};}

struct Builder {
    app: JsValue,
    modal: Element,
    palette: Element,
    flow: Element,
    props: Element,
    draft: RefCell<Value>,
    selection: Cell<usize>,
    listening: Cell<bool>,
    params: RefCell<Value>,
    saving: Cell<bool>,
}
fn friendly(s: &str) -> &str {
    match s {
        "lowpass" => "Срез верха (LPF)",
        "highpass" => "Срез низа (HPF)",
        "bandpass" => "Полоса (вау)",
        "peaking" => "Колокол",
        "lowshelf" => "Полка низа",
        "highshelf" => "Полка верха",
        "notch" => "Вырез",
        "soft" => "Мягкий",
        "asym" => "Асимметричный, ламповый",
        "tube" => "Ламповый",
        "diode" => "Диодный",
        "hard" => "Жёсткий, кремний",
        "fuzz" => "Фузз",
        s => s,
    }
}
fn attr(el: &Element, k: &str, v: &str) {
    el.set_attribute(k, v).unwrap();
}
fn button(root: &Element, class: &str, label: &str, action: &str) -> Element {
    let el = child(root, "button", class, label);
    attr(&el, "data-action", action);
    el
}
fn input(root: &Element, kind: &str, value: &str, action: &str) -> Element {
    let el = child(root, "input", "", "");
    attr(&el, "type", kind);
    set(&el, "value", &value.into());
    attr(&el, "data-edit", action);
    el
}
fn option(root: &Element, id: &str, label: &str) {
    attr(&child(root, "option", "", label), "value", id);
}
fn selected(select: &Element, id: &str) {
    set(select, "value", &id.into())
}
fn error_message(e: &JsValue) -> String {
    get(e, "message")
        .as_string()
        .or_else(|| e.as_string())
        .unwrap_or_else(|| "Не удалось выполнить операцию".into())
}
impl Builder {
    fn compiled(&self) -> Value {
        let mut draft = self.draft.borrow().clone();
        let mut slug = String::new();
        for c in text(&draft, "name").to_lowercase().chars() {
            if c.is_ascii_alphanumeric() || ('а'..='я').contains(&c) || c == 'ё' {
                slug.push(c);
            } else if !slug.ends_with('-') {
                slug.push('-');
            }
        }
        if slug.is_empty() {
            slug.push_str("pedal");
        }
        draft["id"] = json!(format!("user.{slug}"));
        draft["format"] = json!("guitarcraft-pedal");
        draft
    }
    fn preview(&self) -> Result<(), JsValue> {
        let draft = self.compiled();
        let mut tmp = draft.clone();
        tmp["id"] = json!(format!("{}.preview", text(&draft, "id")));
        let def = compile_pedal(&tmp).map_err(crate::err)?;
        invoke(&get(&self.app, "catalog"), "add", &[js(&def)?])?;
        let state: Value = read(get(&get(&self.app, "store"), "state"))?;
        let chain = list(&state["chain"]);
        let amp = chain
            .iter()
            .position(|b| b["kind"] == "amp")
            .unwrap_or(chain.len());
        let ok = invoke(
            &self.app,
            "startAudition",
            &[
                text(&def, "id").into(),
                (amp as u32).into(),
                JsValue::NULL,
                js(&*self.params.borrow())?,
                js(&draft)?,
            ],
        )?;
        self.listening.set(ok.as_bool().unwrap_or(false));
        Ok(())
    }
    fn stop(&self) {
        if self.listening.replace(false) {
            let _ = invoke(&self.app, "stopAudition", &[]);
        }
    }
    fn refresh_audio(&self) -> Result<(), JsValue> {
        if self.listening.get() {
            self.stop();
            self.preview()?;
        }
        Ok(())
    }
    fn palette(&self) {
        let p = &self.palette;
        p.set_text_content(None);
        style(&child(p, "h3", "", "Блоки"), "margin-top", "0");
        for (id, t) in EQUIPMENT["blockTypes"].as_object().unwrap() {
            let b = button(
                p,
                "pal-item",
                &format!("+ {}", text(t, "name")),
                "add-block",
            );
            attr(&b, "data-add-block", id);
        }
        child(p, "h3", "", "Ручки педали");
        for (i, k) in list(&self.draft.borrow()["knobs"]).iter().enumerate() {
            let row = child(p, "div", "row", "");
            attr(&row, "data-knob-index", &i.to_string());
            style(&row, "margin-bottom", "5px");
            let field = input(&row, "text", text(k, "name"), "knob-name");
            style(&field, "width", "170px");
            attr(&field, "title", "Название ручки");
            button(&row, "icon", "✕", "delete-knob");
        }
        button(p, "", "+ ручка", "add-knob");
    }
    fn fields(&self, row: &Element, b: &Value, f: &Value, knobs: &[Value]) {
        let key = text(f, "id");
        let value = &b[key];
        attr(row, "data-field", key);
        if let Some(opts) = f["options"].as_array() {
            let el = child(row, "select", "", "");
            attr(&el, "data-edit", "option");
            for v in opts.iter().filter_map(Value::as_str) {
                option(&el, v, friendly(v));
            }
            selected(&el, value.as_str().unwrap_or_default());
            return;
        }
        let line = child(row, "div", "row", "");
        let number = |key: &str, v: &Value, width: &str, title: &str| {
            let el = input(
                &line,
                "number",
                &v.as_f64().map(|n| n.to_string()).unwrap_or_default(),
                key,
            );
            attr(&el, "step", "any");
            style(&el, "width", width);
            if !title.is_empty() {
                attr(&el, "title", title);
            }
        };
        if !value.is_object() {
            number(
                "fixed",
                if value.is_null() { &f["def"] } else { value },
                "80px",
                "",
            );
        }
        let bind = child(&line, "select", "", "");
        attr(&bind, "data-edit", "binding");
        option(&bind, "", "число");
        for k in knobs {
            option(
                &bind,
                text(k, "id"),
                &crate::i18n::message("ручка {}", &[text(k, "name")]),
            );
        }
        selected(&bind, text(value, "knob"));
        if value.is_object() {
            number("from", &value["from"], "62px", "от");
            line.append_child(&doc().create_text_node("→")).unwrap();
            number("to", &value["to"], "62px", "до");
        }
    }
    fn flow(&self) {
        let root = &self.flow;
        root.set_text_content(None);
        child(root, "div", "flow-arrow", "ВХОД ↓");
        let draft = self.draft.borrow();
        for (i, b) in list(&draft["blocks"]).iter().enumerate() {
            let t = &EQUIPMENT["blockTypes"][text(b, "type")];
            let blk = child(
                root,
                "div",
                if self.selection.get() == i {
                    "blk sel"
                } else {
                    "blk"
                },
                "",
            );
            attr(&blk, "data-block", &i.to_string());
            let head = child(
                &blk,
                "div",
                "blk-head",
                &format!("{}. {}", i + 1, text(t, "name")),
            );
            let buttons = child(&head, "div", "", "");
            for (action, label, title) in [
                ("up", "↑", "Выше"),
                ("down", "↓", "Ниже"),
                ("delete-block", "✕", "Удалить"),
            ] {
                attr(&button(&buttons, "icon", label, action), "title", title);
            }
            let fields = child(&blk, "div", "fields", "");
            for f in list(&t["fields"]) {
                self.fields(
                    &child(&fields, "label", "", text(f, "name")),
                    b,
                    f,
                    list(&draft["knobs"]),
                );
            }
            child(root, "div", "flow-arrow", "↓");
        }
        child(root, "div", "flow-arrow", "ВЫХОД");
    }
    fn props(self: &Rc<Self>) -> Result<(), JsValue> {
        let p = &self.props;
        p.set_text_content(None);
        style(&child(p, "h3", "", "Педаль"), "margin-top", "0");
        let draft = self.draft.borrow().clone();
        for (key, title) in [("name", "Название"), ("type", "Тип")] {
            let label = child(p, "label", "muted small", title);
            attr(
                &label,
                "style",
                "display:flex;flex-direction:column;gap:3px;margin-bottom:8px",
            );
            let el = input(&label, "text", text(&draft, key), key);
            if key == "name" {
                el.set_id("builder-name");
            }
        }
        let cat = child(
            &child(p, "label", "muted small", "Категория"),
            "select",
            "",
            "",
        );
        attr(&cat, "style", "width:100%;margin-bottom:8px");
        attr(&cat, "data-edit", "category");
        for c in list(&EQUIPMENT["categories"]) {
            option(&cat, text(c, "id"), text(c, "name"));
        }
        selected(&cat, text(&draft, "category"));
        let tags = list(&draft["character"])
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        let el = input(
            &child(p, "label", "muted small", "Характер звука (через запятую)"),
            "text",
            &tags,
            "character",
        );
        attr(&el, "style", "width:100%;margin-bottom:8px");
        input(
            &child(p, "label", "muted small", "Цвет корпуса "),
            "color",
            text(&draft, "color"),
            "color",
        );
        let actions = child(p, "div", "det-actions", "");
        style(&actions, "margin-top", "18px");
        let preview = child(&actions, "div", "builder-preview", "");
        if let Ok(def) = compile_pedal(&self.compiled()) {
            let opts = js(&json!({"size":"md","uid":"builder"}))?;
            let weak = Rc::downgrade(self);
            let change = Closure::<dyn Fn(String, f64)>::new(move |id: String, v: f64| {
                if let Some(b) = weak.upgrade() {
                    b.params.borrow_mut()[&id] = json!(v);
                    let engine = get(&b.app, "engine");
                    let playing = get(&engine, "audition");
                    if b.listening.get() && playing.is_object() {
                        if let Ok(params) = js(&*b.params.borrow()) {
                            set(&playing, "params", &params);
                            let _ = invoke(
                                &engine,
                                "applyParams",
                                &[get(&get(&b.app, "store"), "state")],
                            );
                        }
                    }
                }
            });
            set(&opts, "onChange", &change.into_js_value());
            preview.append_child(&pedal_visual(
                &self.app,
                &js(&def)?,
                &js(&*self.params.borrow())?,
                &opts,
            ))?;
        }
        let on = self.listening.get();
        button(
            &actions,
            if on { "listen on" } else { "listen" },
            if on {
                "■ Остановить"
            } else {
                "▶ Послушать в цепочке"
            },
            "listen",
        )
        .set_id("builder-listen");
        button(&actions, "primary", "Сохранить в каталог", "save").set_id("builder-save");
        child(&actions,"p","muted small","Файл .gcpedal.json появится в Документы/GuitarCraft/Pedals. Этот же формат принимает MCP-инструмент pedal_create.");
        Ok(())
    }
    fn render(self: &Rc<Self>) -> Result<(), JsValue> {
        self.palette();
        self.flow();
        self.props()?;
        self.refresh_audio()
    }
    fn notify(&self, s: &str) {
        let _ = invoke(&self.app, "toast", &[s.into()]);
    }
    async fn save(self: Rc<Self>) {
        if self.saving.replace(true) {
            return;
        }
        let draft = self.compiled();
        let result:Result<(),JsValue>=async {
            compile_pedal(&draft).map_err(crate::err)?;
            command("write_json",&js(&json!({"kind":"pedals","name":format!("{}.gcpedal.json",text(&draft,"id")),"data":draft}))?).await?;
            resolved(invoke(&get(&self.app,"catalog"),"loadUserPedals",&[])?).await?;
            self.notify(&crate::i18n::message("«{}» сохранена в каталог (Мои)", &[text(&draft,"name")]));
            if self.modal.is_connected(){close_builder();} Ok(())
        }.await;
        self.saving.set(false);
        if let Err(e) = result {
            self.notify(&crate::i18n::message("Ошибка: {}", &[&error_message(&e)]));
        }
    }
    fn click(self: &Rc<Self>, target: Element) -> Result<(), JsValue> {
        if target == self.modal {
            close_builder();
            return Ok(());
        }
        let block = idx(&target, "data-block");
        let Some(btn) = target.closest("[data-action]")? else {
            if let Some(i) = block {
                self.selection.set(i);
                let rows = self.flow.query_selector_all(".blk")?;
                for n in 0..rows.length() {
                    let e: Element = rows.get(n).unwrap().unchecked_into();
                    e.class_list().toggle_with_force("sel", n as usize == i)?;
                }
            }
            return Ok(());
        };
        match btn.get_attribute("data-action").as_deref() {
            Some("close") => {
                close_builder();
                return Ok(());
            }
            Some("save") => {
                wasm_bindgen_futures::spawn_local(self.clone().save());
                return Ok(());
            }
            Some("listen") => {
                if self.listening.get() {
                    self.stop();
                } else {
                    self.preview()?;
                }
                return self.props();
            }
            Some("add-block") => {
                let key = btn.get_attribute("data-add-block").unwrap();
                let mut b = json!({"type":key});
                for f in list(&EQUIPMENT["blockTypes"][&key]["fields"]) {
                    b[text(f, "id")] = f["def"].clone();
                }
                let mut p = self.draft.borrow_mut();
                let blocks = p["blocks"].as_array_mut().unwrap();
                let i = (self.selection.get() + 1).min(blocks.len());
                blocks.insert(i, b);
                self.selection.set(i);
            }
            Some("up" | "down" | "delete-block") => {
                let i = block.unwrap();
                let mut p = self.draft.borrow_mut();
                let blocks = p["blocks"].as_array_mut().unwrap();
                match btn.get_attribute("data-action").as_deref() {
                    Some("up") if i > 0 => {
                        blocks.swap(i, i - 1);
                        self.selection.set(i - 1);
                    }
                    Some("down") if i + 1 < blocks.len() => {
                        blocks.swap(i, i + 1);
                        self.selection.set(i + 1);
                    }
                    Some("delete-block") => {
                        blocks.remove(i);
                        self.selection.set(i.saturating_sub(1));
                    }
                    _ => return Ok(()),
                }
            }
            Some("add-knob") => {
                let mut p = self.draft.borrow_mut();
                let ks = p["knobs"].as_array_mut().unwrap();
                let mut n = ks.len() + 1;
                while ks.iter().any(|k| k["id"] == format!("k{n}")) {
                    n += 1;
                }
                ks.push(json!({"id":format!("k{n}"),"name":"Knob","def":0.5}));
            }
            Some("delete-knob") => {
                let i = idx(&btn, "data-knob-index").unwrap();
                let mut p = self.draft.borrow_mut();
                let removed = p["knobs"].as_array_mut().unwrap().remove(i);
                for b in p["blocks"].as_array_mut().unwrap() {
                    let schema = &EQUIPMENT["blockTypes"][text(b, "type")];
                    for f in list(&schema["fields"]) {
                        let k = text(f, "id");
                        if b[k].is_object() && b[k]["knob"] == removed["id"] {
                            b[k] = f["def"].clone();
                        }
                    }
                }
                if let Some(params) = self.params.borrow_mut().as_object_mut() {
                    params.remove(text(&removed, "id"));
                }
            }
            _ => return Ok(()),
        }
        self.render()
    }
    fn edit(self: &Rc<Self>, el: Element, kind: &str) -> Result<(), JsValue> {
        let Some(key) = el.get_attribute("data-edit") else {
            return Ok(());
        };
        let v = string(&el, "value");
        let mut p = self.draft.borrow_mut();
        match key.as_str() {
            "name" | "type" | "character" if kind == "input" => {
                p[&key] = if key == "character" {
                    json!(v
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>())
                } else {
                    json!(v)
                };
                return Ok(());
            }
            _ if kind != "change" => return Ok(()),
            "knob-name" => {
                p["knobs"][idx(&el, "data-knob-index").unwrap()]["name"] = json!(v);
                drop(p);
                self.flow();
                return self.props();
            }
            "category" | "color" => {
                p[&key] = json!(v);
                drop(p);
                if key == "color" {
                    self.props()?;
                }
                return Ok(());
            }
            _ => {}
        }
        let Some(i) = idx(&el, "data-block") else {
            return Ok(());
        };
        let Some(f) = el
            .closest("[data-field]")?
            .and_then(|e| e.get_attribute("data-field"))
        else {
            return Ok(());
        };
        let b = &mut p["blocks"][i];
        let number = || json!(v.parse::<f64>().unwrap_or(0.));
        match key.as_str() {
            "binding" => {
                if let Some(spec) = list(&EQUIPMENT["blockTypes"][text(b, "type")]["fields"])
                    .iter()
                    .find(|spec| spec["id"] == f)
                {
                    b[&f] = if v.is_empty() {
                        spec["def"].clone()
                    } else {
                        json!({"knob":v,"from":spec["min"],"to":spec["max"],"exp":spec["exp"]==true})
                    };
                }
            }
            "option" => b[&f] = json!(v),
            "fixed" => b[&f] = number(),
            "from" | "to" => b[&f][&key] = number(),
            _ => return Ok(()),
        }
        drop(p);
        if key == "binding" {
            self.flow();
        }
        self.refresh_audio()
    }
}
fn idx(el: &Element, attr: &str) -> Option<usize> {
    el.closest(&format!("[{attr}]"))
        .ok()??
        .get_attribute(attr)?
        .parse()
        .ok()
}
#[wasm_bindgen(js_name=closeBuilder)]
pub fn close_builder() {
    if let Some(b) = OPEN.with(|o| o.borrow_mut().take()) {
        b.stop();
        b.modal.remove();
    }
}
#[wasm_bindgen(js_name=openBuilder)]
pub fn open_builder(app: JsValue, initial: JsValue) -> Result<(), JsValue> {
    close_builder();
    let modal = make("div", "modal-bg builder-bg");
    let content = child(&modal, "div", "modal", "");
    attr(
        &content,
        "style",
        "display:flex;flex-direction:column;gap:12px",
    );
    let head = child(&content, "div", "cat-head", "");
    let title = child(&head, "div", "", "");
    child(&title, "h2", "", "Конструктор педалей");
    child(&title,"div","muted","Соберите педаль из DSP-блоков, привяжите значения к ручкам, послушайте в цепочке и сохраните в каталог");
    button(&head, "icon close", "✕", "close");
    let root = child(&content, "div", "builder", "");
    let mut draft: Value = if initial.is_object() {
        read(initial)?
    } else {
        let mut pedal: Value = serde_json::from_str(include_str!("builder-template.json")).unwrap();
        for key in ["name", "desc"] {
            pedal[key] = json!(crate::i18n::text(text(&pedal, key)));
        }
        pedal["character"] = json!([crate::i18n::text("мой звук")]);
        pedal
    };
    if !draft.is_object() {
        return Err(crate::err("Неверный формат педали".into()));
    }
    for key in ["blocks", "knobs"] {
        if !draft[key].is_array() {
            draft[key] = json!([]);
        }
    }
    let b = Rc::new(Builder {
        app,
        modal,
        palette: child(&root, "div", "palette", ""),
        flow: child(&root, "div", "flow", ""),
        props: child(&root, "div", "props", ""),
        draft: RefCell::new(draft),
        selection: Cell::new(0),
        listening: Cell::new(false),
        params: RefCell::new(json!({})),
        saving: Cell::new(false),
    });
    for kind in ["click", "input", "change"] {
        let weak = Rc::downgrade(&b);
        listen(&b.modal, kind, move |e: Event| {
            if let (Some(b), Some(target)) = (
                weak.upgrade(),
                e.target().and_then(|t| t.dyn_into::<Element>().ok()),
            ) {
                let result = if kind == "click" {
                    b.click(target)
                } else {
                    b.edit(target, kind)
                };
                if let Err(e) = result {
                    b.notify(&error_message(&e));
                }
            }
        });
    }
    b.render()?;
    doc().body().unwrap().append_child(&b.modal)?;
    OPEN.with(|o| o.replace(Some(b)));
    Ok(())
}
