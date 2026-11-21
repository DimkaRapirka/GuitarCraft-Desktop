//! Hardware editors render the same pedal enclosures, controls and signal order as the catalog.
use crate::{browser::open_catalog, dom::*, js, knob::knob, pedal::pedal_visual, store};
use gc_model::catalog::{list, text};
use serde_json::{json, Value};
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::{Element, Event};

pub(crate) const PEDALBOARD: &str = "pedalboard";

pub(crate) fn attr(el: &Element, k: &str, v: &str) {
    let value = match k {
        "title" | "placeholder" | "aria-label" => crate::i18n::text(v),
        _ => v,
    };
    el.set_attribute(k, value).unwrap();
}
pub(crate) fn report(r: Result<(), JsValue>) {
    if let Err(e) = r {
        web_sys::console::error_1(&e);
    }
}
pub(crate) fn button(root: &Element, class: &str, title: &str, label: &str) -> Element {
    let el = child(root, "button", class, label);
    if !title.is_empty() {
        attr(&el, "title", title);
    }
    el
}
pub(crate) fn on(
    el: &Element,
    kind: &str,
    app: &JsValue,
    f: impl Fn(&JsValue, Event) -> Result<(), JsValue> + 'static,
) {
    let app = app.clone();
    listen(el, kind, move |e| report(f(&app, e)));
}
pub(crate) fn knob_for(
    param: Value,
    value: f64,
    id: &str,
    size: f64,
    style: &str,
    change: impl Fn(f64) -> Result<(), JsValue> + 'static,
) -> Result<Element, JsValue> {
    let opts = js(&json!({"param":param,"value":value,"id":id,"size":size,"style":style}))?;
    let cb = Closure::<dyn Fn(f64)>::new(move |v| report(change(v)));
    set(&opts, "onChange", &cb.into_js_value());
    Ok(knob(&opts))
}
pub(crate) fn update_views(app: &JsValue) -> Result<(), JsValue> {
    invoke(app, "renderStage", &[])?;
    invoke(app, "renderChain", &[])?;
    Ok(())
}
fn plus(parent: &Element, index: usize, app: &JsValue) {
    let b = button(parent, "board-plus", "Добавить педаль сюда", "+");
    attr(&b, "data-board-add", &index.to_string());
    on(&b, "click", app, move |app, _| {
        open_catalog(app.clone(), js(&json!({"index":index}))?)
    });
}
fn info(app: &JsValue, p: &Element) -> Result<(), JsValue> {
    p.set_text_content(None);
    let st = get(app, "store");
    let uid = string(app, "selected");
    if uid == PEDALBOARD {
        let message = if js_sys::Array::from(&invoke(&st, "pedals", &[])?).length() == 0 {
            "Нажмите «+», чтобы добавить педаль."
        } else {
            "Выберите педаль на педалборде."
        };
        child(p, "span", "muted", message);
        return Ok(());
    }
    let block = invoke(&st, "find", &[uid.clone().into()])?;
    if string(&block, "kind") != "pedal" {
        return Ok(());
    }
    let catalog = get(app, "catalog");
    let def = invoke(&catalog, "get", &[get(&block, "pedal")])?;
    child(p, "b", "", &string(&def, "name"));
    child(
        p,
        "span",
        "muted",
        &format!(
            " {} · {}",
            string(&def, "type"),
            if get(&block, "on") == false {
                crate::i18n::text("выключена")
            } else {
                crate::i18n::text("включена")
            }
        ),
    );
    let desc = string(&def, "desc");
    let b = button(p, "icon", &desc, "ⓘ");
    on(&b, "click", app, move |a, _| {
        invoke(a, "toast", &[desc.clone().into()]).map(drop)
    });
    child(p, "div", "grow", "");
    let b = button(p, "", "", "Заменить…");
    let id = uid.clone();
    on(&b, "click", app, move |a, _| {
        let index = invoke(&get(a, "store"), "index", &[id.clone().into()])?.as_f64();
        open_catalog(a.clone(), js(&json!({"index":index,"replaceUid":id}))?)
    });
    let model = string(&def, "id");
    let fav = invoke(&catalog, "isFavorite", &[model.clone().into()])?.is_truthy();
    let b = button(p, if fav { "star on" } else { "star" }, "", "★");
    on(&b, "click", app, move |a, ev| {
        let on = invoke(
            &get(a, "catalog"),
            "toggleFavorite",
            &[model.clone().into()],
        )?
        .is_truthy();
        let el: Element = ev.current_target().unwrap().unchecked_into();
        el.class_list().toggle_with_force("on", on)?;
        Ok(())
    });
    on(
        &button(p, "board-remove", "", "Убрать"),
        "click",
        app,
        move |a, _| invoke(a, "removePedal", &[uid.clone().into()]).map(drop),
    );
    Ok(())
}
#[wasm_bindgen(js_name=renderBoard)]
pub fn render_board(app: JsValue, sel: &str, stage: Element) -> Result<Element, JsValue> {
    stage.class_list().add_1("scene-board")?;
    let state = store::state(&app)?;
    let chain = list(&state["chain"]);
    let size = if chain.iter().filter(|b| b["kind"] == "pedal").count() <= 3 {
        "lg"
    } else {
        "md"
    };
    let wrap = make("div", "board-wrap");
    let board = child(
        &child(&wrap, "div", "board-surface", ""),
        "div",
        "board-row",
        "",
    );
    let status = child(&wrap, "div", "board-info", "");
    plus(&board, 0, &app);
    let mut i = 0;
    while i < chain.len() {
        let b = &chain[i];
        if b["kind"] != "pedal" {
            let start = i;
            while i < chain.len() && chain[i]["kind"] != "pedal" {
                i += 1;
            }
            let run = child(&board, "div", "board-amp", "");
            attr(&run, "title", "Блоки цепочки между педалями");
            child(
                &run,
                "span",
                "",
                &chain[start..i]
                    .iter()
                    .map(|v| text(v, "kind").to_uppercase())
                    .collect::<Vec<_>>()
                    .join(" · "),
            );
            child(&run, "i", "", "→");
            plus(&board, i, &app);
            continue;
        }
        let uid = text(b, "uid").to_owned();
        let def = invoke(&get(&app, "catalog"), "get", &[text(b, "pedal").into()])?;
        let slot = child(
            &board,
            "div",
            if uid == sel {
                "board-slot sel"
            } else {
                "board-slot"
            },
            "",
        );
        attr(&slot, "data-board-pedal", &uid);
        let opts = js(&json!({"uid":uid,"size":size,"on":b["on"]!=false}))?;
        let a = app.clone();
        let id = uid.clone();
        let model = def.clone();
        let change = Closure::<dyn Fn(String, f64)>::new(move |key: String, v: f64| {
            let params = get(&model, "params");
            let name = if js_sys::Array::is_array(&params) {
                js_sys::Array::from(&params)
                    .iter()
                    .find(|p| string(p, "id") == key)
                    .map(|p| string(&p, "name"))
                    .unwrap_or(key.clone())
            } else {
                key.clone()
            };
            report(store::edit(
                &a,
                &format!("{}: {name}", string(&model, "name")),
                Some(&format!("{id}.{key}")),
                |s| {
                    if let Some(p) = s["chain"]
                        .as_array_mut()
                        .and_then(|bs| bs.iter_mut().find(|p| p["uid"] == id))
                    {
                        p["params"][&key] = json!(v);
                    }
                },
            ));
        });
        set(&opts, "onChange", &change.into_js_value());
        let a = app.clone();
        let id = uid.clone();
        let toggle = Closure::<dyn Fn()>::new(move || {
            report(invoke(&a, "toggleBlock", &[id.clone().into()]).map(drop))
        });
        set(&opts, "onToggle", &toggle.into_js_value());
        slot.append_child(&pedal_visual(&app, &def, &js(&b["params"])?, &opts))?;
        let model = js(b)?;
        on(
            &button(&slot, "slot-menu", "Меню педали", "⋯"),
            "click",
            &app,
            move |a, e| {
                e.stop_propagation();
                invoke(a, "blockMenu", &[model.clone(), e.into()]).map(drop)
            },
        );
        on(&slot, "pointerdown", &app, move |a, e| {
            if string(a, "selected") == uid {
                return Ok(());
            }
            set(a, "selected", &uid.clone().into());
            invoke(a, "renderChain", &[])?;
            let t: Element = e.current_target().unwrap().unchecked_into();
            let wrap = t.closest(".board-wrap")?.unwrap();
            let slots = wrap.query_selector_all(".board-slot")?;
            for n in 0..slots.length() {
                let el: Element = slots.get(n).unwrap().unchecked_into();
                el.class_list().toggle_with_force("sel", el == t)?;
            }
            info(a, &wrap.query_selector(".board-info")?.unwrap())
        });
        i += 1;
        plus(&board, i, &app);
    }
    info(&app, &status)?;
    Ok(wrap)
}

#[wasm_bindgen(js_name=ampKnob)]
pub fn amp_knob(app: JsValue, param: JsValue, opts: JsValue) -> Result<Element, JsValue> {
    let st = store::state(&app)?;
    let ch = text(&st["amp"], "channel");
    let id = string(&param, "id");
    let mut p: Value = crate::read(param)?;
    p["min"] = json!(0);
    p["max"] = json!(1);
    let name = format!(
        "{}: {}",
        text(&gc_model::EQUIPMENT["channels"][ch], "name"),
        text(&p, "name")
    );
    let key = format!("amp.{ch}.{id}");
    let options = js(
        &json!({"param":p,"value":st["amp"]["channels"][ch][&id],"id":format!("amp.{id}"),"size":num(&opts,"size",58.),"style":get(&opts,"style").as_string().unwrap_or_else(||"hw".into()),"label":get(&opts,"label")!=false}),
    )?;
    let cb = Closure::<dyn Fn(f64)>::new(move |v| {
        report(store::edit(&app, &name, Some(&key), |s| {
            let ch = text(&s["amp"], "channel").to_owned();
            s["amp"]["channels"][ch][&id] = json!(v);
        }))
    });
    set(&options, "onChange", &cb.into_js_value());
    Ok(knob(&options))
}

#[wasm_bindgen(js_name=renderEditor)]
pub fn render_editor(app: JsValue, stage: Element) -> Result<(), JsValue> {
    crate::skins::clear_view();
    stage.set_text_content(None);
    stage.set_class_name("stage");
    let sel = string(&app, "selected");
    let root = match sel.as_str() {
        "gate" | "eq" | "delay" | "reverb" => crate::racks::render_racks(app, &sel, stage.clone())?,
        "cab" => crate::cab::render_cab(app, stage.clone())?,
        PEDALBOARD => render_board(app, &sel, stage.clone())?,
        _ => {
            let state = store::state(&app)?;
            if let Some(b) = list(&state["chain"])
                .iter()
                .find(|b| b["uid"] == sel && b["kind"] == "pedal")
            {
                render_board(app, text(b, "uid"), stage.clone())?
            } else {
                set(&app, "selected", &"amp".into());
                stage.class_list().add_1("scene-studio")?;
                let root = make("div", "editor amp-editor");
                let wrap = child(&root, "div", "amp-skin", "");
                wrap.set_id("amp-skin");
                wasm_bindgen_futures::spawn_local(async move {
                    report(crate::skins::render(app, wrap).await);
                });
                root
            }
        }
    };
    stage.append_child(&root)?;
    Ok(())
}
