use crate::{dom::*, js, pedal::pedal_visual, read};
use gc_model::{
    catalog::{list, text},
    EQUIPMENT,
};
use js_sys::Object;
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::{Element, Event, HtmlInputElement, KeyboardEvent};

thread_local! {static OPEN:RefCell<Option<Rc<Panel>>>=const{RefCell::new(None)};}
#[derive(Default)]
struct Selection {
    category: String,
    source: String,
    view: String,
    selected: String,
    params: Value,
    results: Vec<Value>,
    auditioning: bool,
}
struct Panel {
    app: JsValue,
    catalog: JsValue,
    index: usize,
    replace: Option<String>,
    replacing: String,
    modal: Element,
    input: HtmlInputElement,
    chips: Element,
    sources: Element,
    now: Element,
    rows: Element,
    detail: Element,
    st: RefCell<Selection>,
    keys: RefCell<Option<Closure<dyn FnMut(Event)>>>,
}
fn action(root: &Element, kind: &str, class: &str, label: &str, id: &str) -> Element {
    let b = child(root, "button", class, label);
    b.set_attribute("data-action", kind).unwrap();
    if !id.is_empty() {
        b.set_id(id);
    }
    b
}
fn data(e: &Element, key: &str, v: &str) {
    e.set_attribute(key, v).unwrap();
}
fn report(r: Result<(), JsValue>) {
    if let Err(e) = r {
        web_sys::console::error_1(&e);
    }
}
impl Panel {
    fn info(&self, id: &str) -> Result<Value, JsValue> {
        read(invoke(&self.catalog, "get", &[id.into()])?)
    }
    fn favorite(&self, id: &str) -> bool {
        invoke(&self.catalog, "isFavorite", &[id.into()])
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
    fn refresh(self: &Rc<Self>) -> Result<(), JsValue> {
        let st = self.st.borrow();
        let view = st.view.as_str();
        let q = self.input.value();
        let filters = match view {
            "fav" => json!({"favorites":true,"category":st.category}),
            "recent" => json!({"recent":true}),
            _ => json!({"category":st.category,"source":st.source}),
        };
        let mut results: Vec<Value> =
            read(invoke(&self.catalog, "search", &[q.into(), js(&filters)?])?)?;
        if view != "fav" && view != "recent" && st.source != "plugin" {
            results.retain(|d| d["source"] != "plugin");
        }
        self.chips.set_text_content(None);
        let chip = |id: &str, name: &str, on: bool| {
            let b = action(
                &self.chips,
                "chip",
                if on { "chip on" } else { "chip" },
                name,
                "",
            );
            data(&b, "data-chip", id);
        };
        chip("fav", "★ Избранное", view == "fav");
        chip("recent", "🕘 Недавние", view == "recent");
        child(&self.chips, "span", "chip-sep", "");
        for c in list(&EQUIPMENT["categories"]) {
            chip(text(c, "id"), text(c, "name"), st.category == text(c, "id"));
        }
        self.sources.set_text_content(None);
        let items: Vec<Value> = read(invoke(&self.catalog, "all", &[])?)?;
        for (id, name) in [
            ("", "Все источники"),
            ("builtin", "Встроенные"),
            ("user", "Мои педали"),
            ("extension", "Расширения"),
            ("plugin", "Найденные VST3/AU (не обрабатывают звук)"),
        ] {
            let count = items
                .iter()
                .filter(|d| {
                    if id.is_empty() {
                        d["source"] != "plugin"
                    } else {
                        d["source"] == id
                    }
                })
                .count();
            let opt = child(&self.sources, "option", "", &format!("{name} ({count})"));
            data(&opt, "value", id);
        }
        set(&self.sources, "value", &st.source.clone().into());
        drop(st);
        {
            let mut st = self.st.borrow_mut();
            if !results.iter().any(|d| d["id"] == st.selected) {
                st.selected = results
                    .first()
                    .map(|d| text(d, "id").into())
                    .unwrap_or_default();
            }
            st.results = results;
        }
        self.render_rows()?;
        self.render_detail()
    }
    fn render_rows(&self) -> Result<(), JsValue> {
        self.rows.set_text_content(None);
        let st = self.st.borrow();
        if st.results.is_empty() {
            child(
                &self.rows,
                "div",
                "empty",
                match st.view.as_str() {
                    "fav" => "В избранном пока пусто. Нажмите ★ у педали.",
                    "recent" => "Вы ещё ничего не добавляли.",
                    _ => "Ничего не найдено. Попробуйте «драйв», «эхо», «тёплый»…",
                },
            );
        }
        for d in &st.results {
            let id = text(d, "id");
            let row = child(&self.rows, "div", "cat-row", "");
            data(&row, "data-pedal", id);
            data(&row, "data-action", "select");
            row.class_list()
                .toggle_with_force("sel", id == st.selected)?;
            row.class_list()
                .toggle_with_force("unsupported", d["supported"] != true)?;
            if d["source"] == "plugin" {
                child(&row, "i", "sw plug", text(d, "format"));
            } else {
                row.append_child(&pedal_visual(
                    &self.app,
                    &js(d)?,
                    &JsValue::NULL,
                    &js(&json!({"size":"thumb"}))?,
                ))?;
            }
            let desc = child(&row, "div", "cr-main", "");
            child(&desc, "b", "", text(d, "name"));
            let tags = list(&d["character"])
                .iter()
                .filter_map(Value::as_str)
                .take(3)
                .collect::<Vec<_>>()
                .join(", ");
            child(&desc, "small", "", &format!("{} · {tags}", text(d, "type")));
            child(
                &row,
                "span",
                &format!("src {}", text(d, "source")),
                match text(d, "source") {
                    "builtin" => "встроенная",
                    "user" => "моя",
                    "extension" => "расширение",
                    _ => text(d, "format"),
                },
            );
            let star = action(
                &row,
                "star",
                if self.favorite(id) { "star on" } else { "star" },
                "★",
                "",
            );
            data(&star, "title", "Избранное");
            data(&star, "data-model", id);
        }
        Ok(())
    }
    fn ab(&self, parent: &Element, playing: &Value, ids: bool) -> Result<(), JsValue> {
        let controls = child(parent, "div", "ab", "");
        if ids {
            controls.set_id("ab")
        }
        let active = playing["active"] == true;
        action(
            &controls,
            "a",
            if active { "" } else { "on" },
            &format!(
                "A: {}",
                if self.replacing.is_empty() {
                    "без педали"
                } else {
                    &self.replacing
                }
            ),
            if ids { "ab-a" } else { "" },
        );
        let d = self.info(text(playing, "pedalId"))?;
        action(
            &controls,
            "b",
            if active { "on" } else { "" },
            &format!("B: {}", text(&d, "name")),
            if ids { "ab-b" } else { "" },
        );
        Ok(())
    }
    fn render_detail(self: &Rc<Self>) -> Result<(), JsValue> {
        self.now.set_text_content(None);
        self.detail.set_text_content(None);
        let a: Value = read(get(&get(&self.app, "engine"), "audition")).unwrap_or_default();
        let st = self.st.borrow();
        let playing = st.auditioning && a.is_object();
        self.now
            .class_list()
            .toggle_with_force("hidden", !playing)?;
        if playing {
            let id = text(&a, "pedalId");
            child(&self.now, "i", "now-dot", "");
            let head = child(&self.now, "span", "", "Сейчас звучит: ");
            child(&head, "b", "", text(&self.info(id)?, "name"));
            self.ab(&self.now, &a, false)?;
            action(&self.now, "stop", "", "■ Стоп", "");
            let add = action(
                &self.now,
                "commit",
                "primary",
                if self.replace.is_some() {
                    "Заменить"
                } else {
                    "Добавить"
                },
                "",
            );
            data(&add, "data-model", id);
        }
        let id = st.selected.clone();
        if id.is_empty() {
            return Ok(());
        }
        let params = st.params[&id].clone();
        let d = self.info(&id)?;
        drop(st);
        if d.is_null() {
            return Ok(());
        }
        if d["supported"] != true {
            let info = child(&self.detail, "div", "unsup", "");
            child(&info, "h3", "", text(&d, "name"));
            child(&info, "div", "muted", text(&d, "type"));
            child(&info, "p", "", text(&d, "desc"));
            child(&info,"p","muted","Для обработки звука доступны встроенные педали, педали из конструктора и IR-файлы кабинетов.");
            return Ok(());
        }
        let opts = js(&json!({"size":"md","uid":"cat","on":!playing||a["active"]==true}))?;
        let weak = Rc::downgrade(self);
        let model = id.clone();
        let on_change = Closure::wrap(Box::new(move |key: String, v: f64, _: bool| {
            if let Some(panel) = weak.upgrade() {
                let mut st = panel.st.borrow_mut();
                st.params[&model][&key] = json!(v);
                let engine = get(&panel.app, "engine");
                let audition = get(&engine, "audition");
                if string(&audition, "pedalId") == model {
                    let params = js(&st.params[&model]).unwrap();
                    drop(st);
                    set(&audition, "params", &params);
                    let _ = invoke(
                        &engine,
                        "applyParams",
                        &[get(&get(&panel.app, "store"), "state")],
                    );
                }
            }
        }) as Box<dyn FnMut(String, f64, bool)>)
        .into_js_value();
        set(&opts, "onChange", &on_change);
        let weak = Rc::downgrade(self);
        let toggle = Closure::wrap(Box::new(move || {
            if let Some(panel) = weak.upgrade() {
                if panel.st.borrow().auditioning {
                    let a = get(&get(&panel.app, "engine"), "audition");
                    report(panel.set_ab(!get(&a, "active").is_truthy()));
                }
            }
        }) as Box<dyn FnMut()>)
        .into_js_value();
        set(&opts, "onToggle", &toggle);
        self.detail
            .append_child(&pedal_visual(&self.app, &js(&d)?, &js(&params)?, &opts))?;
        let info = child(&self.detail, "div", "det-info", "");
        child(&info, "h3", "", text(&d, "name"));
        let cat = list(&EQUIPMENT["categories"])
            .iter()
            .find(|c| c["id"] == d["category"])
            .map(|c| text(c, "name"))
            .unwrap_or_default();
        child(
            &info,
            "div",
            "muted",
            &format!("{} · {cat}", text(&d, "type")),
        );
        child(&info, "p", "", text(&d, "desc"));
        let tags = child(&info, "div", "tags", "");
        for tag in list(&d["character"])
            .iter()
            .filter_map(Value::as_str)
            .take(8)
        {
            let badge = child(&tags, "span", "tag", tag);
            data(&badge, "data-action", "tag");
        }
        let acts = child(&info, "div", "det-actions", "");
        action(
            &acts,
            "listen",
            if playing { "listen on" } else { "listen" },
            if playing {
                "■ Остановить"
            } else {
                "▶ Послушать в цепочке"
            },
            "btn-listen",
        );
        if playing {
            self.ab(&acts, &a, true)?;
        }
        let row = child(&acts, "div", "row", "");
        let replace = self.replace.is_some();
        action(
            &row,
            "commit",
            "primary",
            if replace {
                "Заменить"
            } else {
                "Добавить в цепочку"
            },
            if replace { "btn-replace" } else { "btn-add" },
        );
        let star = action(
            &row,
            "star",
            if self.favorite(&id) {
                "star on"
            } else {
                "star"
            },
            "★",
            "",
        );
        data(&star, "data-model", &id);
        child(
            &acts,
            "div",
            "muted small",
            "Двойной клик или Enter: добавить. Пробел: послушать.",
        );
        Ok(())
    }
    fn set_ab(self: &Rc<Self>, active: bool) -> Result<(), JsValue> {
        invoke(&self.app, "setAuditionActive", &[active.into()])?;
        self.render_detail()
    }
    fn stop(&self) -> Result<(), JsValue> {
        if self.st.borrow().auditioning {
            invoke(&self.app, "stopAudition", &[])?;
            self.st.borrow_mut().auditioning = false;
        }
        Ok(())
    }
    fn play(&self, id: &str) -> Result<(), JsValue> {
        let params = js(&self.st.borrow().params[id])?;
        let on = invoke(
            &self.app,
            "startAudition",
            &[
                id.into(),
                (self.index as f64).into(),
                js(&self.replace)?,
                params,
            ],
        )?
        .is_truthy();
        self.st.borrow_mut().auditioning = on;
        Ok(())
    }
    fn select(self: &Rc<Self>, id: &str) -> Result<(), JsValue> {
        let was = self.st.borrow().auditioning;
        self.st.borrow_mut().selected = id.into();
        if was {
            self.stop()?;
            self.play(id)?;
        }
        let rows = self.rows.children();
        for i in 0..rows.length() {
            if let Some(el) = rows.item(i) {
                el.class_list().toggle_with_force(
                    "sel",
                    el.get_attribute("data-pedal").as_deref() == Some(id),
                )?;
            }
        }
        self.render_detail()
    }
    fn commit(&self, id: &str) -> Result<(), JsValue> {
        if self.info(id)?["supported"] != true {
            return Ok(());
        }
        self.stop()?;
        let params = js(&self.st.borrow().params[id])?;
        let params = if params.is_null() {
            Object::new().into()
        } else {
            params
        };
        if let Some(uid) = &self.replace {
            invoke(
                &self.app,
                "replacePedal",
                &[uid.as_str().into(), id.into(), "ui".into(), params],
            )?;
        } else {
            invoke(
                &self.app,
                "addPedal",
                &[id.into(), (self.index as f64).into(), params],
            )?;
        }
        close_catalog()
    }
    fn clicked(self: &Rc<Self>, e: Event) -> Result<(), JsValue> {
        let Some(target) = e.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
            return Ok(());
        };
        if target == self.modal {
            return close_catalog();
        }
        let Some(el) = target.closest("[data-action]")? else {
            return Ok(());
        };
        let selected = self.st.borrow().selected.clone();
        let id = el.get_attribute("data-model").unwrap_or(selected);
        match el.get_attribute("data-action").as_deref() {
            Some("close") => close_catalog(),
            Some("build") => {
                close_catalog()?;
                invoke(&self.app, "openBuilder", &[])?;
                Ok(())
            }
            Some("select") => self.select(&el.get_attribute("data-pedal").unwrap_or_default()),
            Some("commit") => self.commit(&id),
            Some("a") => self.set_ab(false),
            Some("b") => self.set_ab(true),
            Some("listen" | "stop") => {
                if self.st.borrow().auditioning {
                    self.stop()?
                } else {
                    self.play(&id)?
                }
                self.render_detail()
            }
            Some("star") => {
                invoke(&self.catalog, "toggleFavorite", &[id.into()])?;
                self.refresh()
            }
            Some("tag") => {
                self.input.set_value(&el.text_content().unwrap_or_default());
                self.refresh()
            }
            Some("chip") => {
                let chip = el.get_attribute("data-chip").unwrap_or_default();
                let mut st = self.st.borrow_mut();
                if matches!(chip.as_str(), "fav" | "recent") {
                    st.view = if st.view == chip { "all".into() } else { chip }
                } else {
                    st.category = if st.category == chip {
                        String::new()
                    } else {
                        chip
                    };
                    st.view = "all".into();
                }
                drop(st);
                self.refresh()
            }
            _ => Ok(()),
        }
    }
    fn key(self: &Rc<Self>, e: KeyboardEvent) -> Result<(), JsValue> {
        if !self.modal.is_connected() {
            return close_catalog();
        }
        let id = self.st.borrow().selected.clone();
        match e.key().as_str() {
            "Escape" => close_catalog(),
            "Enter" if !id.is_empty() => self.commit(&id),
            " " if !id.is_empty()
                && doc().active_element().as_ref() != Some(self.input.as_ref()) =>
            {
                e.prevent_default();
                if self.st.borrow().auditioning {
                    self.stop()?
                } else {
                    self.play(&id)?
                }
                self.render_detail()
            }
            key @ ("ArrowDown" | "ArrowUp") => {
                e.prevent_default();
                let st = self.st.borrow();
                let next = st
                    .results
                    .iter()
                    .position(|d| d["id"] == id)
                    .and_then(|i| {
                        if key == "ArrowUp" {
                            i.checked_sub(1)
                        } else {
                            Some(i + 1)
                        }
                    })
                    .and_then(|i| st.results.get(i))
                    .map(|d| text(d, "id").to_owned());
                drop(st);
                if let Some(id) = next {
                    self.select(&id)?;
                    if let Some(el) = self.rows.query_selector(".sel")? {
                        let opts = web_sys::ScrollIntoViewOptions::new();
                        opts.set_block(web_sys::ScrollLogicalPosition::Nearest);
                        el.scroll_into_view_with_scroll_into_view_options(&opts);
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
#[wasm_bindgen(js_name=closeCatalog)]
pub fn close_catalog() -> Result<(), JsValue> {
    let prev = OPEN.with(|s| s.take());
    if let Some(p) = prev {
        if let Some(k) = p.keys.take() {
            doc().remove_event_listener_with_callback("keydown", k.as_ref().unchecked_ref())?;
        }
        p.modal.remove();
        p.stop()?;
    }
    Ok(())
}
#[wasm_bindgen(js_name=openCatalog)]
pub fn open_catalog(app: JsValue, opts: JsValue) -> Result<(), JsValue> {
    close_catalog()?;
    let index = num(&opts, "index", 0.).max(0.) as usize;
    let replace = get(&opts, "replaceUid").as_string();
    let catalog = get(&app, "catalog");
    let store = get(&app, "store");
    let replacing = if let Some(id) = replace.as_ref() {
        let b = invoke(&store, "find", &[id.as_str().into()])?;
        let pedal = invoke(&catalog, "get", &[get(&b, "pedal")])?;
        string(&pedal, "name")
    } else {
        String::new()
    };
    let where_text = if replace.is_some() {
        "Новая педаль встанет на то же место в цепочке".into()
    } else {
        let chain = js_sys::Array::from(&get(&get(&store, "state"), "chain"));
        let name = |i: Option<usize>, fallback: &str| {
            i.map(|i| chain.get(i as u32))
                .filter(|b| b.is_object())
                .and_then(|b| invoke(&app, "blockInfo", &[b]).ok())
                .map(|d| string(&d, "name"))
                .unwrap_or_else(|| fallback.into())
        };
        format!(
            "Место в цепочке: между «{}» и «{}»",
            name(index.checked_sub(1), "Gate"),
            name(Some(index), "выход")
        )
    };
    let modal = make("div", "modal-bg catalog-bg");
    modal.set_inner_html(include_str!("catalog.html"));
    let find = |s: &str| modal.query_selector(s).unwrap().unwrap();
    find(".cat-head h2").set_text_content(Some(&if replacing.is_empty() {
        "Добавить педаль".into()
    } else {
        format!("Заменить «{replacing}»")
    }));
    find(".cat-head .muted").set_text_content(Some(&where_text));
    let input: HtmlInputElement = find("#cat-search").unchecked_into();
    input.set_value(&string(&opts, "query"));
    let panel = Rc::new(Panel {
        app,
        catalog,
        index,
        replace,
        replacing,
        input,
        chips: find(".chips"),
        sources: find("#cat-source"),
        now: find("#now-bar"),
        rows: find("#cat-list"),
        detail: find("#cat-detail"),
        modal: modal.clone(),
        st: Default::default(),
        keys: Default::default(),
    });
    for (kind, target) in [
        ("click", &panel.modal),
        ("dblclick", &panel.rows),
        ("change", &panel.sources),
        ("input", panel.input.as_ref()),
    ] {
        let weak = Rc::downgrade(&panel);
        listen(target, kind, move |e| {
            let Some(p) = weak.upgrade() else { return };
            let result = match kind {
                "click" => p.clicked(e),
                "dblclick" => {
                    let row = e
                        .target()
                        .and_then(|t| t.dyn_into::<Element>().ok())
                        .and_then(|t| t.closest("[data-pedal]").ok().flatten());
                    if let Some(r) = row {
                        p.commit(&r.get_attribute("data-pedal").unwrap_or_default())
                    } else {
                        Ok(())
                    }
                }
                _ => {
                    let mut st = p.st.borrow_mut();
                    if kind == "change" {
                        st.source = string(&p.sources, "value");
                    }
                    st.view = "all".into();
                    drop(st);
                    p.refresh()
                }
            };
            report(result);
        });
    }
    let weak = Rc::downgrade(&panel);
    let keys = Closure::wrap(Box::new(move |e: Event| {
        if let Some(p) = weak.upgrade() {
            report(p.key(e.unchecked_into()));
        }
    }) as Box<dyn FnMut(Event)>);
    doc().add_event_listener_with_callback("keydown", keys.as_ref().unchecked_ref())?;
    panel.keys.replace(Some(keys));
    doc().body().unwrap().append_child(&modal)?;
    OPEN.with(|s| s.replace(Some(panel.clone())));
    panel.refresh()?;
    panel.input.focus()
}
