//! Preset files, bank navigation, favourites and A/B comparison with the last saved rig.
use crate::{
    desktop::*,
    dom::*,
    editor::attr,
    host::{command, resolved},
    js,
    preferences::save_settings,
    read, store,
};
use gc_model::catalog::text;
use js_sys::{Array, Promise};
use serde_json::{json, Value};
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::{Element, HtmlInputElement, KeyboardEvent};
fn all() -> Result<Vec<Value>, JsValue> {
    read(get(&app(), "presets"))
}
fn current() -> String {
    string(&get(&app(), "store"), "currentFile")
}
fn choose(p: &Value) -> Result<(), JsValue> {
    invoke(
        &get(&app(), "store"),
        "load",
        &[js(&p["data"])?, js(&p["file"])?, js(&p["bank"])?],
    )
    .map(drop)
}
fn go_new() -> Result<(), JsValue> {
    invoke(
        &get(&app(), "store"),
        "load",
        &[js(&gc_model::default_rig("Новый пресет"))?],
    )
    .map(drop)
}
fn dialog(title: &str, prompt: Option<&str>) -> Result<Promise, JsValue> {
    let m = make("div", "modal-bg");
    let modal = child(&m, "div", "modal small", "");
    child(&modal, "h3", "", title);
    let input = if let Some(v) = prompt {
        let e: HtmlInputElement = child(&modal, "input", "", "").dyn_into()?;
        e.set_id("prompt-input");
        e.set_value(v);
        Some(e)
    } else {
        child(
            &modal,
            "p",
            "muted",
            "Сохранить их перед переключением пресета?",
        );
        None
    };
    let row = child(&modal, "div", "row end", "");
    let cancel = id_button(&row, "", "close", "", "Отмена");
    attr(&cancel, "data-reply", "cancel");
    if input.is_none() {
        attr(
            &id_button(&row, "", "", "", "Не сохранять"),
            "data-reply",
            "discard",
        );
    }
    let ok = id_button(
        &row,
        if input.is_some() { "prompt-ok" } else { "" },
        "primary",
        "",
        if input.is_some() {
            "OK"
        } else {
            "Сохранить"
        },
    );
    attr(&ok, "data-reply", "ok");
    let p = Promise::new(&mut |resolve, _| {
        let submit = resolve.clone();
        listen(&m, "click", move |ev| {
            let Some(el) = ev.target().and_then(|e| e.dyn_into::<Element>().ok()) else {
                return;
            };
            let Some(b) = el.closest("[data-reply]").ok().flatten() else {
                return;
            };
            let Some(modal) = b.closest(".modal-bg").ok().flatten() else {
                return;
            };
            let resp = match b.get_attribute("data-reply").as_deref() {
                Some("discard") => "discard".into(),
                Some("ok") => modal
                    .query_selector("input")
                    .ok()
                    .flatten()
                    .map(|el| JsValue::from(string(&el, "value").trim()))
                    .unwrap_or_else(|| "save".into()),
                _ => JsValue::NULL,
            };
            modal.remove();
            let _ = submit.call1(&JsValue::UNDEFINED, &resp);
        });
        if let Some(inp) = &input {
            let submit = resolve.clone();
            listen(inp, "keydown", move |ev| {
                let e = ev.unchecked_into::<KeyboardEvent>();
                if e.key() != "Enter" && e.key() != "Escape" {
                    return;
                }
                e.stop_propagation();
                e.prevent_default();
                let t: Element = e.current_target().unwrap().unchecked_into();
                if let Ok(Some(m)) = t.closest(".modal-bg") {
                    m.remove();
                }
                let value = if e.key() == "Escape" {
                    JsValue::NULL
                } else {
                    string(&t, "value").trim().into()
                };
                let _ = submit.call1(&JsValue::UNDEFINED, &value);
            });
        }
    });
    doc().body().unwrap().append_child(&m)?;
    if let Some(i) = input {
        i.focus()?;
        i.select();
    }
    Ok(p)
}
async fn discard() -> Result<bool, JsValue> {
    if !get(&get(&app(), "store"), "dirty").is_truthy() || get(&app(), "comparing").is_truthy() {
        return Ok(true);
    }
    let s = store::state(&app())?;
    let choice = resolved(
        dialog(
            &format!("Изменения в «{}» не сохранены", text(&s, "name")),
            None,
        )?
        .into(),
    )
    .await?;
    match choice.as_string().as_deref() {
        Some("discard") => Ok(true),
        Some("save") => Ok(save(false, None, None, "ui").await?.is_truthy()),
        _ => Ok(false),
    }
}
pub async fn refresh() -> Result<(), JsValue> {
    let mut users: Vec<Value> = read(command("presets_list", &JsValue::UNDEFINED).await?)?;
    users.sort_by(|a, b| {
        (text(a, "bank"), text(a, "name")).cmp(&(text(b, "bank"), text(b, "name")))
    });
    let mut v=gc_model::factory_presets().into_iter().map(|p|json!({"file":format!("factory:{}",text(&p,"name")),"name":p["name"],"bank":"Заводские","data":p,"favorite":false})).collect::<Vec<_>>();
    v.extend(users);
    set(&app(), "presets", &js(&v)?);
    Ok(())
}
async fn save(
    new: bool,
    name: Option<String>,
    bank: Option<String>,
    src: &str,
) -> Result<JsValue, JsValue> {
    let a = app();
    let file = current();
    let presets = all()?;
    let cur = presets.iter().find(|p| p["file"] == file);
    let n = string(&get(&get(&a, "store"), "state"), "name");
    let as_new = new || cur.is_none_or(|p| p["bank"] == "Заводские");
    let name = match name.filter(|n| !n.is_empty()) {
        Some(n) => n,
        None if as_new && src == "ui" => {
            let pretty = format!("{} (мой)", n.split(" — ").next().unwrap_or(&n));
            let Some(n) = resolved(dialog("Название пресета", Some(&pretty))?.into())
                .await?
                .as_string()
                .filter(|s| !s.is_empty())
            else {
                return Ok(JsValue::NULL);
            };
            n
        }
        None => n,
    };
    let bank = if as_new {
        bank.filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Мои".into())
    } else {
        text(cur.unwrap(), "bank").into()
    };
    let base = store::state(&a)?;
    let mut saved = base.clone();
    saved["name"] = json!(name);
    let mut doc = saved.clone();
    doc["tempo"] = json!(num(&get(&a, "settings"), "bpm", 120.));
    doc.as_object_mut().unwrap().remove("factory");
    doc["savedAt"] = json!(js_sys::Date::new_0().to_iso_string().as_string());
    doc["app"] = json!(concat!(
        "GuitarCraft ",
        env!("CARGO_PKG_VERSION"),
        " (Rust)"
    ));
    let res = command("presets_save", &js(&json!({"preset":doc,"bank":bank}))?).await?;
    let dest = string(&res, "file");
    if current() == file {
        let project = get(&a, "catalog");
        let mut current = store::state(&a)?;
        current["name"] = json!(name);
        invoke(
            &project,
            "edit",
            &[
                js(&current)?,
                "Сохранение пресета".into(),
                src.into(),
                JsValue::NULL,
            ],
        )?;
        invoke(
            &project,
            "save_baseline",
            &[dest.clone().into(), bank.clone().into(), js(&saved)?],
        )?;
        invoke(
            &get(&a, "store"),
            "changed",
            &["saved".into(), js(&json!({}))?],
        )?;
    }
    refresh().await?;
    call("renderPresetBar", &[])?;
    call(
        "toast",
        &[format!("Пресет «{name}» сохранён").into(), src.into()],
    )?;
    js(&json!({"file":dest,"bank":bank,"name":name}))
}
fn fill(m: &Element) -> Result<(), JsValue> {
    let q = m.query_selector(".menu-search")?.unwrap();
    let t = string(&q, "value").to_lowercase();
    let only = m
        .query_selector(".preset-filter")?
        .unwrap()
        .get_attribute("aria-pressed")
        .as_deref()
        == Some("true");
    let items = m.query_selector(".preset-list")?.unwrap();
    items.set_text_content(None);
    let stars = get(&get(&app(), "settings"), "presetStars");
    let mut bank = String::new();
    for p in all()? {
        let name = text(&p, "name");
        let file = text(&p, "file").to_owned();
        let star = get(&stars, &file)
            .as_bool()
            .unwrap_or(p["favorite"] == true);
        if (only && !star) || !name.to_lowercase().contains(&t) {
            continue;
        }
        if p["bank"] != bank {
            bank = text(&p, "bank").to_owned();
            child(&items, "div", "menu-head", &bank);
        }
        let b = child(
            &items,
            "div",
            if file == current() {
                "menu-item active"
            } else {
                "menu-item"
            },
            name,
        );
        let k = id_button(
            &b,
            "",
            "preset-star",
            if star {
                "Убрать из избранных"
            } else {
                "В избранные"
            },
            if star { "★" } else { "☆" },
        );
        attr(&k, "aria-pressed", &star.to_string());
        listen(&k, "click", move |e| {
            e.stop_propagation();
            let file = file.clone();
            task(async move {
                let mut stars =
                    crate::read::<Option<Value>>(get(&get(&app(), "settings"), "presetStars"))?
                        .filter(Value::is_object)
                        .unwrap_or_else(|| json!({}));
                stars[&file] = json!(!star);
                save_settings(app(), js(&json!({"presetStars":stars}))?).await?;
                if let Some(m) = doc().query_selector(".preset-menu")? {
                    fill(&m)?;
                }
                Ok(())
            });
        });
        listen(&b, "click", move |_| {
            close_menu();
            let p = p.clone();
            task(async move {
                if discard().await? {
                    choose(&p)?;
                }
                Ok(())
            });
        });
    }
    Ok(())
}
#[wasm_bindgen]
impl Desktop {
    pub fn prompt(&self, title: String, value: String) -> Result<Promise, JsValue> {
        dialog(&title, Some(&value))
    }
    #[wasm_bindgen(js_name=refreshPresets)]
    pub async fn refresh_presets(&self) -> Result<(), JsValue> {
        refresh().await
    }
    #[wasm_bindgen(js_name=confirmDiscard)]
    pub async fn confirm_discard(&self) -> Result<bool, JsValue> {
        discard().await
    }
    #[wasm_bindgen(js_name=stepPreset)]
    pub async fn step_preset(&self, d: i32) -> Result<(), JsValue> {
        if d != 0 && !discard().await? {
            return Ok(());
        }
        let ps = all()?;
        if ps.is_empty() {
            return Ok(());
        }
        let i = ps
            .iter()
            .position(|p| p["file"] == current())
            .map(|i| i as i32)
            .unwrap_or(if d == 0 { 0 } else { -1 });
        choose(&ps[(i + d).rem_euclid(ps.len() as i32) as usize])
    }
    #[wasm_bindgen(js_name=savePreset)]
    pub async fn save_preset(
        &self,
        new: bool,
        name: Option<String>,
        bank: Option<String>,
        opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        save(
            new,
            name,
            bank,
            &get(&opts, "source")
                .as_string()
                .unwrap_or_else(|| "ui".into()),
        )
        .await
    }
    #[wasm_bindgen(js_name=deletePreset)]
    pub async fn delete_preset(&self) -> Result<(), JsValue> {
        let Some(p) = all()?
            .into_iter()
            .find(|p| p["file"] == current() && p["bank"] != "Заводские")
        else {
            return Ok(());
        };
        if web_sys::window()
            .unwrap()
            .confirm_with_message(&format!("Удалить пресет «{}»?", text(&p, "name")))?
        {
            command("presets_delete", &js(&json!({"file":p["file"]}))?).await?;
            refresh().await?;
            self.step_preset(0).await?;
        }
        Ok(())
    }
    pub fn compare(&self, on: bool) -> Result<(), JsValue> {
        let a = app();
        let st = get(&a, "store");
        let old = get(&a, "comparing").is_truthy();
        let on = on && (old || get(&st, "dirty").is_truthy());
        if old == on {
            return Ok(());
        }
        set(&a, "comparing", &on.into());
        let pr = get(&a, "catalog");
        set(
            &st,
            "state",
            &invoke(&pr, if on { "saved" } else { "state" }, &[])?,
        );
        invoke(&get(&a, "engine"), "build", &[get(&st, "state")])?;
        element_root()?
            .class_list()
            .toggle_with_force("comparing", on)?;
        for k in ["renderChain", "renderStage", "renderIO", "renderPresetBar"] {
            call(k, &[])?;
        }
        Ok(())
    }
    #[wasm_bindgen(js_name=presetMenu)]
    pub fn preset_menu(&self, anchor: Element) -> Result<(), JsValue> {
        if doc().query_selector(".preset-menu")?.is_some() {
            close_menu();
            return Ok(());
        }
        let r = anchor.get_bounding_client_rect();
        let m = menu(
            r.left(),
            r.bottom() + 6.,
            "menu preset-menu",
            Some(r.width().max(320.)),
        )?;
        let input = child(&m, "input", "menu-search", "");
        attr(&input, "placeholder", "Поиск пресета…");
        let filter = id_button(&m, "", "preset-filter", "", "★ Избранные");
        attr(&filter, "aria-pressed", "false");
        child(&m, "div", "preset-list", "");
        listen(&input, "input", |_| {
            if let Ok(Some(m)) = doc().query_selector(".preset-menu") {
                crate::editor::report(fill(&m));
            }
        });
        listen(&filter, "click", |e| {
            let b: Element = e.current_target().unwrap().unchecked_into();
            let on = b.get_attribute("aria-pressed").as_deref() != Some("true");
            attr(&b, "aria-pressed", &on.to_string());
            if let Some(m) = b.parent_element() {
                crate::editor::report(fill(&m));
            }
        });
        fill(&m)?;
        child(&m, "div", "menu-sep", "");
        menu_item(&m, "+ Новый пресет", false, || {
            task(async {
                if discard().await? {
                    go_new()?;
                }
                Ok(())
            });
            Ok(())
        });
        input.unchecked_into::<HtmlInputElement>().focus()
    }
    #[wasm_bindgen(js_name=presetMore)]
    pub fn preset_more(&self, anchor: Element) -> Result<(), JsValue> {
        let r = anchor.get_bounding_client_rect();
        let m = menu(r.right() - 220., r.bottom() + 6., "menu", Some(220.))?;
        let p = all()?.into_iter().find(|p| p["file"] == current());
        let factory = p.as_ref().is_none_or(|p| p["bank"] == "Заводские");
        menu_item(&m, "Сохранить как…", false, || {
            task(async { save(true, None, None, "ui").await.map(drop) });
            Ok(())
        });
        menu_item(&m, "Новый пресет", false, || {
            task(async {
                if discard().await? {
                    go_new()?;
                }
                Ok(())
            });
            Ok(())
        });
        let file = current();
        menu_item(&m, "Показать файл", factory, move || {
            let f = file.clone();
            task(async move { command("reveal", &js(&json!({"file":f}))?).await.map(drop) });
            Ok(())
        });
        child(&m, "div", "menu-sep", "");
        menu_item(&m, "Удалить пресет", factory, || {
            call("deletePreset", &[]).map(drop)
        });
        Ok(())
    }
    #[wasm_bindgen(js_name=renderPresetBar)]
    pub fn render_preset_bar(&self) -> Result<(), JsValue> {
        let a = app();
        let st = get(&a, "store");
        let dirty = get(&st, "dirty").is_truthy();
        let comparing = get(&a, "comparing").is_truthy();
        let factory = all()?
            .iter()
            .find(|p| p["file"] == current())
            .is_none_or(|p| p["bank"] == "Заводские");
        let r = element("presetbar")?;
        r.set_text_content(None);
        let step = |d: i32, label: &str| {
            let b = id_button(
                &r,
                "",
                "icon",
                if d < 0 {
                    "Предыдущий пресет"
                } else {
                    "Следующий пресет"
                },
                label,
            );
            listen(&b, "click", move |_| {
                let _ = call("stepPreset", &[d.into()]);
            });
        };
        step(-1, "‹");
        let name = id_button(&r, "preset-name", "preset-name", "", "");
        child(&name, "span", "dirty", if dirty { "*" } else { "" });
        child(&name, "span", "pn", &string(&get(&st, "state"), "name"));
        child(&name, "small", "", &string(&st, "currentBank"));
        child(&name, "span", "chev", "▾");
        listen(&name, "click", |e| {
            let _ = call("presetMenu", &[e.current_target().unwrap().into()]);
        });
        step(1, "›");
        let actions = child(&r, "div", "preset-actions", "");
        let save = id_button(
            &actions,
            "btn-save",
            "",
            if factory {
                "Заводской пресет сохранится как новый"
            } else {
                "⌘S"
            },
            if factory {
                "Сохранить как…"
            } else {
                "Сохранить"
            },
        );
        checkbox(&save, "disabled", !dirty && !factory);
        listen(&save, "click", move |_| {
            let _ = call("savePreset", &[factory.into()]);
        });
        if dirty || comparing {
            let ab = child(&actions, "div", "ab-preset", "");
            attr(&ab, "title", "Сравнить с сохранённой версией");
            for (on, id, label, tip) in [
                (true, "btn-compare-a", "A", "A: сохранённая версия пресета"),
                (false, "btn-compare-b", "B", "B: ваша правка"),
            ] {
                let b = id_button(&ab, id, if comparing == on { "on" } else { "" }, tip, label);
                listen(&b, "click", move |_| {
                    let _ = call("compare", &[on.into()]);
                });
            }
        }
        let more = id_button(&actions, "btn-preset-more", "icon", "Ещё", "⋯");
        listen(&more, "click", |e| {
            let _ = call("presetMore", &[e.current_target().unwrap().into()]);
        });
        let history = child(&r, "div", "history", "");
        for (k, ls, id, title, label) in [
            ("undo", "past", "btn-undo", "Отменить (⌘Z)", "↶"),
            ("redo", "future", "btn-redo", "Вернуть (⇧⌘Z)", "↷"),
        ] {
            let b = id_button(&history, id, "icon", title, label);
            checkbox(&b, "disabled", Array::from(&get(&st, ls)).length() == 0);
            listen(&b, "click", move |_| {
                let _ = invoke(&get(&app(), "store"), k, &[]);
            });
        }
        Ok(())
    }
}
fn element_root() -> Result<Element, JsValue> {
    get(&app(), "root").dyn_into::<Element>()
}
