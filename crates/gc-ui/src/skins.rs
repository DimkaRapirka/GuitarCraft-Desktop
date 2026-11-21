//! Built-in and imported amplifier panels. Layouts retain the original normalised coordinate format.
use crate::{
    dom::*,
    editor::{amp_knob, attr, button, on, report},
    host::{command, resolved},
    js, read, store,
};
use gc_model::{
    catalog::{list, text},
    EQUIPMENT,
};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::LazyLock,
};
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::spawn_local;
use web_sys::{Element, PointerEvent, ResizeObserver};
static BUILTIN: LazyLock<Vec<Value>> =
    LazyLock::new(|| serde_json::from_str(include_str!("../../gc-model/data/skins.json")).unwrap());
thread_local! {
    static USERS:RefCell<Option<Vec<Value>>>=const{RefCell::new(None)};
    static GENERATION:Cell<u32>=const{Cell::new(0)};
    static EDITING:Cell<bool>=const{Cell::new(false)};
    static VIEW:RefCell<Option<(ResizeObserver,Closure<dyn FnMut()>)>>=const{RefCell::new(None)};
}
pub fn clear_view() {
    VIEW.with(|slot| {
        if let Some((observer, _)) = slot.take() {
            observer.disconnect();
        }
    });
}
fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}
fn layouts() -> Value {
    storage()
        .and_then(|s| s.get_item("gc.skinLayouts").ok().flatten())
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}
fn save_layout(id: &str, layout: Option<Value>) -> Result<(), JsValue> {
    let mut layouts = layouts();
    if let Some(layout) = layout {
        layouts[id] = layout;
    } else {
        layouts.as_object_mut().unwrap().remove(id);
    }
    if let Some(s) = storage() {
        s.set_item("gc.skinLayouts", &layouts.to_string())?;
    }
    Ok(())
}
async fn all() -> Result<Vec<Value>, JsValue> {
    loop {
        if let Some(user) = USERS.with(|s| s.borrow().clone()) {
            let mut v = BUILTIN.clone();
            v.extend(user);
            return Ok(v);
        }
        let epoch = GENERATION.with(Cell::get);
        let files: Vec<String> = read(
            command(
                "list_files",
                &js(&json!({"kind":"skins","ext":[".gcskin.json"]}))?,
            )
            .await?,
        )?;
        let mut users = vec![];
        for f in files {
            let Ok(raw) = command("read_text", &js(&json!({"file":f}))?).await else {
                continue;
            };
            let Some(mut s) = raw
                .as_string()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                .filter(|v| v.is_object() && v["id"].is_string())
            else {
                continue;
            };
            s["user"] = json!(true);
            s["file"] = json!(f);
            users.push(s);
        }
        if GENERATION.with(Cell::get) == epoch {
            USERS.with(|s| s.replace(Some(users)));
        }
    }
}
fn slot(state: &Value) -> &'static str {
    if matches!(text(&state["amp"], "channel"), "rhythm" | "lead") {
        "headSkin"
    } else {
        "skin"
    }
}
pub(crate) fn active(s: &Value) -> &str {
    let key = slot(s);
    if let Some(id) = s[key].as_str().filter(|s| !s.is_empty()) {
        id
    } else if key == "headSkin" {
        s["skin"]
            .as_str()
            .filter(|s| !s.is_empty() && *s != "ember")
            .unwrap_or("ironhead")
    } else {
        "ember"
    }
}
fn image(s: &Value) -> Result<String, JsValue> {
    if s["user"] == true {
        Ok(invoke(
            &get(&get(&web_sys::window().unwrap(), "__TAURI__"), "core"),
            "convertFileSrc",
            &[text(s, "imagePath").into()],
        )?
        .as_string()
        .unwrap_or_default())
    } else {
        Ok(text(s, "image").to_owned())
    }
}
fn layout(s: &Value) -> Value {
    let p = &s["panel"];
    let x0 = p["x0"].as_f64().unwrap_or(0.08);
    let x1 = p["x1"].as_f64().unwrap_or(0.92);
    let y = p["y"].as_f64().unwrap_or(0.235);
    let params = list(&EQUIPMENT["params"]["amp"]);
    let mut lay = json!({"channel":{"x":x0+0.125,"y":y+0.011}});
    for (i, p) in params.iter().enumerate() {
        lay[text(p, "id")] =
            json!({"x":x0+0.295+(x1-x0-0.335)*i as f64/(params.len()-1).max(1) as f64,"y":y});
    }
    for ovs in [&s["positions"], &layouts()[text(s, "id")]] {
        if let Some(o) = ovs.as_object() {
            for (id, pos) in o {
                if pos["x"].is_number() && pos["y"].is_number() && lay.get(id).is_some() {
                    lay[id] = pos.clone();
                }
            }
        }
    }
    lay
}
fn place(el: &Element, p: &Value) {
    style(
        el,
        "left",
        &format!("{}%", p["x"].as_f64().unwrap_or(0.5) * 100.),
    );
    style(
        el,
        "top",
        &format!("{}%", p["y"].as_f64().unwrap_or(0.5) * 100.),
    );
}
#[wasm_bindgen(js_name=listSkins)]
pub async fn list_skins() -> Result<JsValue, JsValue> {
    js(&all()
        .await?
        .into_iter()
        .map(|s| json!({"id":s["id"],"name":s["name"],"user":s["user"]==true}))
        .collect::<Vec<_>>())
}
#[wasm_bindgen(js_name=setSkin)]
pub async fn set_skin(app: JsValue, id: String) -> Result<JsValue, JsValue> {
    let s = all()
        .await?
        .into_iter()
        .find(|s| s["id"] == id)
        .unwrap_or_else(|| BUILTIN[0].clone());
    store::edit(
        &app,
        &crate::i18n::message("Оформление: {}", &[text(&s, "name")]),
        None,
        |st| {
            let slot = slot(st);
            st[slot] = s["id"].clone();
        },
    )?;
    invoke(&app, "select", &["amp".into()])?;
    js(&json!({"skin":s["id"],"name":s["name"]}))
}
#[wasm_bindgen(js_name=setEditing)]
pub fn set_editing(app: JsValue, on: bool) -> Result<JsValue, JsValue> {
    EDITING.with(|s| s.set(on));
    invoke(&app, "renderStage", &[])?;
    js(&json!({"editing":on}))
}
fn controls(
    app: &JsValue,
    frame: &Element,
    ctl: &Element,
    skin: &Value,
    lay: &Rc<RefCell<Value>>,
    ch: &str,
    edit: bool,
) -> Result<(), JsValue> {
    ctl.set_text_content(None);
    let width = frame.client_width().max(1) as f64;
    let size = (width * skin["knob"].as_f64().unwrap_or(0.05)).round();
    let positions = lay.borrow().clone();
    for (id, pos) in positions.as_object().unwrap() {
        let b = child(
            ctl,
            "div",
            if edit { "skin-ctl editing" } else { "skin-ctl" },
            "",
        );
        attr(&b, "data-ctl", id);
        place(&b, pos);
        if id == "channel" {
            let group = child(&b, "div", "chan-buttons", "");
            style(&group, "--bsz", &format!("{}px", (size * 0.42).round()));
            for (cid, c) in EQUIPMENT["channels"].as_object().unwrap() {
                let k = button(
                    &group,
                    if cid == ch { "chan-btn on" } else { "chan-btn" },
                    text(c, "desc"),
                    "",
                );
                attr(&k, "data-channel", cid);
                style(&k, "--c", text(c, "color"));
                child(&k, "i", "lamp", "");
                child(&k, "span", "cap", "");
                child(&k, "em", "", text(c, "name"));
                if !edit {
                    on(&k, "click", app, move |a, _| {
                        store::edit(a, &format!("Канал {}", text(c, "name")), None, |st| {
                            st["amp"]["channel"] = json!(cid)
                        })?;
                        crate::editor::update_views(a)
                    });
                }
            }
        } else if let Some(p) = list(&EQUIPMENT["params"]["amp"])
            .iter()
            .find(|p| p["id"] == *id)
        {
            let control = amp_knob(
                app.clone(),
                js(p)?,
                js(&json!({"size":size,"style":"overlay"}))?,
            )?;
            b.append_child(&control)?;
        }
        if edit {
            for kind in ["pointerdown", "pointermove", "pointerup", "pointercancel"] {
                let lay = lay.clone();
                let id = id.clone();
                let skin = text(skin, "id").to_owned();
                let cb=Closure::<dyn FnMut(PointerEvent)>::new(move|e:PointerEvent|{
                    let el:Element=e.current_target().unwrap().unchecked_into();let pid=e.pointer_id();
                    if kind!="pointerdown"&&get(&el,"gcDrag").as_f64()!=Some(pid as f64){return;}
                    e.prevent_default();e.stop_propagation();
                    if kind=="pointerdown"{set(&el,"gcDrag",&pid.into());let _=el.set_pointer_capture(pid);}
                    else if kind=="pointermove"{if let Ok(Some(frame))=el.closest(".skin-frame"){let r=frame.get_bounding_client_rect();if r.width()>0.&&r.height()>0.{
                        let p=json!({"x":(e.client_x() as f64-r.left())/r.width(),"y":(e.client_y() as f64-r.top())/r.height()});place(&el,&p);lay.borrow_mut()[&id]=p;
                    }}}
                    else{set(&el,"gcDrag",&JsValue::NULL);let _=el.release_pointer_capture(pid);report(save_layout(&skin,Some(lay.borrow().clone())));}
                }).into_js_value();
                b.add_event_listener_with_callback_and_bool(kind, cb.unchecked_ref(), true)?;
            }
        }
    }
    Ok(())
}
pub async fn render(app: JsValue, wrap: Element) -> Result<(), JsValue> {
    let all = all().await?;
    if !wrap.is_connected() {
        return Ok(());
    }
    let state = store::state(&app)?;
    let s = all
        .into_iter()
        .find(|s| s["id"] == active(&state))
        .unwrap_or_else(|| BUILTIN[0].clone());
    let ch = text(&state["amp"], "channel").to_owned();
    let edit = EDITING.with(Cell::get);
    let lay = Rc::new(RefCell::new(layout(&s)));
    wrap.set_text_content(None);
    let vp = child(&wrap, "div", "skin-viewport", "");
    let frame = child(&vp, "div", "skin-frame", "");
    for k in ["text", "ring", "pointer"] {
        style(&frame, &format!("--skin-{k}"), text(&s, k));
    }
    let img = child(&frame, "img", "", "");
    attr(&img, "src", &image(&s)?);
    attr(&img, "alt", text(&s, "name"));
    attr(&img, "draggable", "false");
    let ctl = child(&frame, "div", "skin-controls", "");
    child(&vp, "div", "floor-shadow", "");
    let a = app.clone();
    let skin = s.clone();
    let channel = ch.clone();
    let last = Cell::new(-1);
    let update = Closure::<dyn FnMut()>::new(move || {
        let w = frame.client_width();
        if w > 0 && last.replace(w) != w {
            report(controls(&a, &frame, &ctl, &skin, &lay, &channel, edit));
        }
    });
    let observer = ResizeObserver::new(update.as_ref().unchecked_ref())?;
    observer.observe(&vp.first_element_child().unwrap());
    clear_view();
    VIEW.with(|v| v.replace(Some((observer, update))));
    let bar = child(&wrap, "div", "amp-bar", "");
    let chan = &EQUIPMENT["channels"][&ch];
    if edit {
        child(
            &bar,
            "span",
            "edit-hint",
            "Разметка: перетащите ручки на панель",
        );
    } else {
        let desc = child(&bar, "span", "amp-desc", "");
        child(&desc, "b", "", text(chan, "name"));
        let description = crate::i18n::text(text(chan, "desc"));
        desc.append_child(&doc().create_text_node(&format!(" · {description}")))?;
    }
    child(&bar, "div", "grow", "");
    if edit {
        let done = button(&bar, "primary", "", "Готово");
        done.set_id("btn-layout");
        on(&done, "click", &app, |a, _| {
            set_editing(a.clone(), false).map(drop)
        });
        let id = text(&s, "id").to_owned();
        on(
            &button(&bar, "", "", "Сбросить"),
            "click",
            &app,
            move |a, _| {
                save_layout(&id, None)?;
                invoke(a, "renderStage", &[]).map(drop)
            },
        );
    } else {
        let pick = button(&bar, "ghost", "", &format!("◐ {}", text(&s, "name")));
        pick.set_id("btn-skin-picker");
        on(&pick, "click", &app, |a, _| {
            let app = a.clone();
            spawn_local(async move {
                report(picker(app, false).await);
            });
            Ok(())
        });
        let on_amp = state["amp"]["on"] != false;
        let power = button(
            &bar,
            if on_amp {
                "ghost power-amp on"
            } else {
                "ghost power-amp"
            },
            "",
            "",
        );
        child(&power, "i", "", "");
        power.append_child(&doc().create_text_node(if on_amp { "POWER" } else { "BYPASS" }))?;
        on(&power, "click", &app, |a, _| {
            invoke(a, "toggleBlock", &["amp".into()]).map(drop)
        });
    }
    Ok(())
}
#[wasm_bindgen(js_name=importSkin)]
pub async fn import_skin(app: JsValue, file: String, opts: JsValue) -> Result<JsValue, JsValue> {
    let dest = command("import_to", &js(&json!({"kind":"skins","file":file}))?)
        .await?
        .as_string()
        .ok_or_else(|| js_sys::Error::new("Ошибка импорта PNG"))?;
    let filename = dest.rsplit(['/', char::from(92)]).next().unwrap_or(&dest);
    let mut pretty = filename.to_owned();
    if pretty.to_lowercase().ends_with(".png") {
        pretty.truncate(pretty.len() - 4);
    }
    pretty = pretty
        .split(['_', '-'])
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if let Some(c) = pretty.get_mut(0..1) {
        c.make_ascii_uppercase();
    }
    let name = if let Some(n) = get(&opts, "name").as_string().filter(|s| !s.is_empty()) {
        n
    } else {
        resolved(invoke(
            &app,
            "prompt",
            &["Название оформления".into(), pretty.clone().into()],
        )?)
        .await?
        .as_string()
        .filter(|s| !s.is_empty())
        .unwrap_or(pretty)
    };
    let id = format!("user-{}", gc_model::new_uid());
    let skin = json!({"id":id,"name":name,"imagePath":dest,"panel":{"x0":0.08,"x1":0.92,"y":0.235},"knob":0.062,"text":"#f0e6d2","ring":"#b07cff","pointer":"#ffffff"});
    command(
        "write_json",
        &js(&json!({"kind":"skins","name":format!("{id}.gcskin.json"),"data":skin}))?,
    )
    .await?;
    GENERATION.with(|g| g.set(g.get().wrapping_add(1)));
    USERS.with(|s| s.take());
    EDITING.with(|v| v.set(get(&opts, "edit") != false));
    set_skin(app.clone(), id.clone()).await?;
    invoke(&app,"toast",&["PNG импортирован. Ручки стоят поверх картинки, «Разметка ручек» позволяет их сдвинуть.".into()])?;
    js(&json!({"skin":id,"file":dest}))
}
fn close_picker() {
    if let Ok(Some(el)) = doc().query_selector(".skins-bg") {
        el.remove();
    }
}
fn prompt(style: &str) -> String {
    include_str!("skin-prompt.txt").trim_end().replace(
        "{style}",
        if style.trim().is_empty() {
            "ваш стиль: цвета, материалы, эпоха"
        } else {
            style.trim()
        },
    )
}
fn create_page(content: &Element, app: &JsValue) {
    content.set_text_content(None);
    child(content,"p","muted","1) Опишите стиль и скопируйте промпт. 2) Сгенерируйте картинку в любом генераторе. 3) Импортируйте PNG: ручки встанут на пустую панель, их можно подвинуть.");
    let input = child(content, "input", "", "");
    input.set_id("skin-style");
    attr(
        &input,
        "placeholder",
        "Опишите стиль: «чёрный тольекс, неоновая подсветка, киберпанк»",
    );
    style(&input, "width", "100%");
    let r = child(content, "div", "", "");
    style(&r, "margin-top", "8px");
    let area = child(&r, "textarea", "prompt-box", &prompt(""));
    attr(&area, "readonly", "");
    listen(&input, "input", |e| {
        if let Ok(Some(el)) = doc().query_selector(".skins-bg .prompt-box") {
            set(
                &el,
                "value",
                &prompt(&string(&e.current_target().unwrap(), "value")).into(),
            );
        }
    });
    let btns = child(content, "div", "row", "");
    style(&btns, "margin-top", "10px");
    let copy = button(&btns, "", "", "Скопировать промпт");
    copy.set_id("btn-copy-prompt");
    on(&copy, "click", app, |app, _| {
        let a = app.clone();
        if let Some(el) = doc().query_selector(".skins-bg .prompt-box")? {
            let args = js(&json!({"text":string(&el,"value")}))?;
            spawn_local(async move {
                report(
                    async {
                        command("clipboard_write", &args).await?;
                        invoke(
                            &a,
                            "toast",
                            &[crate::i18n::text("Промпт скопирован: вставьте его в генератор изображений").into()],
                        )
                        .map(drop)
                    }
                    .await,
                );
            });
        }
        Ok(())
    });
    let import = button(&btns, "primary", "", "Импорт PNG…");
    import.set_id("btn-import-png");
    on(&import, "click", app, |app, _| {
        let a = app.clone();
        spawn_local(async move {
            report(async{
        let file=command("file_open",&js(&json!({"title":crate::i18n::text("PNG с фасадом комбика"),"filters":[{"name":"PNG","extensions":["png"]}]}))?).await?;if let Some(f)=file.as_string(){close_picker();import_skin(a,f,JsValue::UNDEFINED).await?;}Ok(())
    }.await);
        });
        Ok(())
    });
    on(
        &button(&btns, "", "", "← Назад"),
        "click",
        app,
        |a, _| {
            let a = a.clone();
            spawn_local(async move {
                report(picker(a, false).await);
            });
            Ok(())
        },
    );
}
async fn picker(app: JsValue, create: bool) -> Result<(), JsValue> {
    let skins = all().await?;
    let st = store::state(&app)?;
    let cur = active(&st);
    close_picker();
    let m = make("div", "modal-bg skins-bg");
    let modal = child(&m, "div", "modal", "");
    style(&modal, "width", "min(980px, 94vw)");
    let head = child(&modal, "div", "cat-head", "");
    child(&head, "h2", "", "Оформление комбика");
    let b = button(&head, "icon close", "", "✕");
    listen(&b, "click", |_| close_picker());
    listen(&m, "click", |e| {
        if e.target() == e.current_target() {
            close_picker();
        }
    });
    let content = child(&modal, "div", "", "");
    if create {
        create_page(&content, &app);
    } else {
        let grid = child(&content, "div", "skins-grid", "");
        style(&grid, "margin-top", "14px");
        for s in &skins {
            let id = text(s, "id").to_owned();
            let card = child(
                &grid,
                "div",
                if id == cur {
                    "skin-card on"
                } else {
                    "skin-card"
                },
                "",
            );
            attr(&card, "data-skin", &id);
            let pic = child(&card, "div", "card-img", "");
            let img = child(&pic, "img", "", "");
            attr(&img, "src", &image(s)?);
            for (id, p) in layout(s).as_object().unwrap() {
                let el = child(
                    &pic,
                    "i",
                    if id == "channel" {
                        "card-chan"
                    } else {
                        "card-knob"
                    },
                    "",
                );
                place(&el, p);
                if id == "channel" {
                    for _ in 0..4 {
                        child(&el, "b", "", "");
                    }
                }
            }
            let label = child(&card, "div", "", text(s, "name"));
            child(
                &label,
                "span",
                "muted",
                if s["user"] == true {
                    " · свой"
                } else {
                    ""
                },
            );
            on(&card, "click", &app, move |a, _| {
                let a = a.clone();
                let id = id.clone();
                spawn_local(async move {
                    report(set_skin(a, id).await.map(|_| close_picker()));
                });
                Ok(())
            });
        }
        let row = child(&content, "div", "row", "");
        style(&row, "margin-top", "14px");
        let create = button(&row, "primary", "", "＋ Создать своё оформление…");
        create.set_id("btn-create-skin");
        on(&create, "click", &app, |a, e| {
            let el: Element = e.current_target().unwrap().unchecked_into();
            if let Some(content) = el.parent_element().and_then(|r| r.parent_element()) {
                create_page(&content, a);
            }
            Ok(())
        });
        if let Some(s) = skins.iter().find(|s| s["id"] == cur) {
            let b = button(
                &row,
                "",
                "",
                &crate::i18n::message("Разметка ручек для «{}»", &[text(s, "name")]),
            );
            b.set_id("btn-edit-layout");
            on(&b, "click", &app, |a, _| {
                EDITING.with(|v| v.set(true));
                close_picker();
                invoke(a, "select", &["amp".into()]).map(drop)
            });
        }
    }
    doc().body().unwrap().append_child(&m)?;
    Ok(())
}
