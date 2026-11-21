//! User controls, MIDI and the local agent API dispatch through the same project operations.
use crate::{
    dom::*,
    host::{command, resolved},
    js, read, skins, store,
};
use gc_model::{
    catalog::{compile_pedal, list, text, SEARCH},
    EQUIPMENT,
};
use js_sys::{Array, Promise};
use serde_json::{json, Map, Value};
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::future_to_promise;
fn result(v: Value) -> Result<JsValue, JsValue> {
    js(&v)
}
fn arrays(o: &JsValue, k: &str) -> Result<Vec<Value>, JsValue> {
    read(get(o, k))
}
fn keys(v: &Value) -> Vec<&String> {
    v.as_object()
        .map(|m| m.keys().collect())
        .unwrap_or_default()
}
fn round(v: &Value) -> Value {
    v.as_f64()
        .map(|n| json!((n * 1000.).round() / 1000.))
        .unwrap_or_else(|| v.clone())
}
fn chain(app: &JsValue, s: &Value) -> Result<Vec<Value>, JsValue> {
    let mut blocks = vec![];
    for (i, b) in list(&s["chain"]).iter().enumerate() {
        let kind = text(b, "kind");
        let data = &s[kind];
        let mut item = json!({"index":i,"uid":kind,"kind":kind,"on":data["on"]!=false});
        let (defs, values) = match kind {
            "cab" => {
                for key in [
                    "mode",
                    "model",
                    "mic",
                    "invert2",
                    "position2",
                    "distance2",
                    "blend",
                    "mic2",
                ] {
                    item[key] = data[key].clone();
                }
                if item["mic2"] == "" {
                    item["mic2"] = Value::Null;
                }
                item["ir"] = data["irPath"].clone();
                for k in ["position", "distance", "level"] {
                    item[k] = round(&data[k]);
                }
                item["models"] = json!(keys(&EQUIPMENT["cabs"]));
                item["mics"] = json!(keys(&EQUIPMENT["mics"]));
                blocks.push(item);
                continue;
            }
            "pedal" => {
                item["on"] = json!(b["on"] != false);
                item["uid"] = b["uid"].clone();
                item["pedal"] = b["pedal"].clone();
                let def = invoke(&get(app, "catalog"), "get", &[text(b, "pedal").into()])?;
                let d = if def.is_object() {
                    read(def)?
                } else {
                    Value::Null
                };
                item["name"] = d["name"].clone();
                item["type"] = d["type"].clone();
                (d["params"].clone(), &b["params"])
            }
            "amp" => {
                item["channel"] = data["channel"].clone();
                item["channels"] = json!(keys(&EQUIPMENT["channels"]));
                (
                    EQUIPMENT["params"]["amp"].clone(),
                    &data["channels"][text(data, "channel")],
                )
            }
            _ => (EQUIPMENT["params"][kind].clone(), data),
        };
        let mut params = Map::new();
        for d in list(&defs) {
            let k = text(d, "id");
            let v = if values[k].is_null() {
                &d["def"]
            } else {
                &values[k]
            };
            let mut p = json!({"value":round(v),"min":d["min"].as_f64().unwrap_or(0.),"max":d["max"].as_f64().unwrap_or(1.)});
            if kind != "amp" {
                p["unit"] = json!(text(d, "unit"));
            }
            params.insert(k.into(), p);
        }
        item["params"] = Value::Object(params);
        blocks.push(item);
    }
    Ok(blocks)
}
fn describe(app: &JsValue) -> Result<JsValue, JsValue> {
    let s = store::state(app)?;
    let st = get(app, "store");
    let e = get(app, "engine");
    let past = arrays(&st, "past")?;
    result(
        json!({"preset":s["name"],"bank":string(&st,"currentBank"),"unsaved_changes":get(&st,"dirty").is_truthy(),"input":{"gain_db":round(&s["input"]["gain"]),"gate":s["input"]["gate"]},"chain":chain(app,&s)?,"output":{"gain_db":round(&s["output"]["gain"])},
        "undo_available":past.iter().skip(past.len().saturating_sub(5)).map(|x|&x["label"]).collect::<Vec<_>>(),"source":if get(&e,"liveNode").is_truthy(){"live input".into()}else{format!("DI: {} ({})",string(&e,"diName"),if get(&e,"diPlaying").is_truthy(){"playing"}else{"stopped"})}}),
    )
}
fn target(app: &JsValue, id: &str) -> Result<String, JsValue> {
    invoke(&get(app,"catalog"),"target",&[id.into()])?.as_string().ok_or_else(||crate::err(format!("Блок «{id}» не найден. Используйте uid из chain_get или: gate, amp, cab, eq, delay, reverb, input, output")))
}
fn index_from(app: &JsValue, p: &Value) -> Result<i32, JsValue> {
    if let Some(i) = p["index"].as_f64().filter(|v| v.fract() == 0.) {
        return Ok(i as i32);
    }
    let s = store::state(app)?;
    let ch = list(&s["chain"]);
    let pos = |uid: &str| {
        ch.iter()
            .position(|v| v["uid"] == uid || v["kind"] == uid)
            .map(|n| n as i32)
            .unwrap_or(-1)
    };
    for (k, add) in [("before", 0), ("after", 1)] {
        if let Some(id) = p[k].as_str() {
            return Ok(pos(&target(app, id)?) + add);
        }
    }
    Ok(match text(p, "position") {
        "end" => ch.len() as i32,
        "post-amp" => pos("amp") + 1,
        _ => pos("amp"),
    })
}
fn param(app: &JsValue, p: &Value, source: &str) -> Result<JsValue, JsValue> {
    let id = text(p, "target");
    let key = text(p, "param");
    let r = invoke(
        &get(app, "catalog"),
        "set_param",
        &[
            id.into(),
            key.into(),
            js(&p["value"])?,
            source.into(),
            JsValue::NULL,
        ],
    )?;
    invoke(
        &get(app, "store"),
        "changed",
        &[
            if id == "cab" { "structure" } else { "params" }.into(),
            js(&json!({"label":format!("{id} · {key}"),"source":source}))?,
        ],
    )?;
    Ok(r)
}
async fn call(o: &JsValue, k: &str, a: &[JsValue]) -> Result<JsValue, JsValue> {
    resolved(invoke(o, k, a)?).await
}
fn el(id: &str) -> Result<web_sys::Element, JsValue> {
    doc()
        .get_element_by_id(id)
        .ok_or_else(|| crate::err(format!("Нет элемента {id}")))
}
async fn dispatch(app: JsValue, method: &str, p: Value, src: &str) -> Result<JsValue, JsValue> {
    let cat = get(&app, "catalog");
    let st = get(&app, "store");
    let engine = get(&app, "engine");
    let s = || store::state(&app);
    let id = text(&p, "target");
    let pedal = text(&p, "pedal");
    let chain_names = || -> Result<Value, JsValue> {
        Ok(json!(chain(&app, &s()?)?
            .iter()
            .map(|b| b["name"].as_str().unwrap_or(text(b, "kind")))
            .collect::<Vec<_>>()))
    };
    match method {
        "state.get" => describe(&app),
        "state.raw" => js(&s()?),
        "catalog.categories" => js(&EQUIPMENT["categories"]),
        "catalog.formats" => js(&SEARCH["formats"]),
        "catalog.favorite" => {
            result(json!({"favorite":invoke(&cat,"toggleFavorite",&[pedal.into()])?.is_truthy()}))
        }
        "catalog.search" => {
            let hits = Array::from(&invoke(
                &cat,
                "search",
                &[text(&p, "query").into(), js(&p)?],
            )?);
            let limit = p["limit"].as_f64().filter(|v| *v != 0.).unwrap_or(12.) as i32;
            let n = if limit < 0 {
                (hits.length() as i32 + limit).max(0)
            } else {
                limit
            } as usize;
            let out = Array::new();
            for d in hits.iter().take(n) {
                out.push(&invoke(&cat, "summary", &[d])?);
            }
            Ok(out.into())
        }
        "chain.add" => {
            let uid = invoke(
                &app,
                "addPedal",
                &[
                    pedal.into(),
                    index_from(&app, &p)?.into(),
                    js(&p["params"].as_object().cloned().unwrap_or_default())?,
                    src.into(),
                ],
            )?;
            result(json!({"added":uid.as_string(),"chain":chain_names()?}))
        }
        "chain.move" => {
            let uid = target(&app, id)?;
            invoke(
                &app,
                "moveBlock",
                &[uid.into(), index_from(&app, &p)?.into(), src.into()],
            )?;
            result(json!({"chain":chain_names()?}))
        }
        "chain.replace" | "chain.remove" => {
            let uid = target(&app, id)?;
            if method == "chain.replace" {
                invoke(
                    &app,
                    "replacePedal",
                    &[uid.into(), pedal.into(), src.into()],
                )?;
                result(json!({"replaced":id,"with":pedal}))
            } else {
                invoke(&app, "removePedal", &[uid.into(), src.into()])?;
                result(json!({"removed":id}))
            }
        }
        "chain.toggle" => {
            let uid = target(&app, id)?;
            let s = s()?;
            let cur = match uid.as_str() {
                "gate" => s["input"]["gate"]["on"] == true,
                k if s.get(k).is_some() => s[k]["on"] != false,
                _ => list(&s["chain"])
                    .iter()
                    .find(|b| b["uid"] == uid)
                    .is_some_and(|b| b["on"] != false),
            };
            let on = p["on"].as_bool().unwrap_or(!cur);
            if on != cur {
                invoke(&app, "toggleBlock", &[uid.into(), src.into()])?;
            }
            result(json!({"target":id,"on":on}))
        }
        "param.set" => param(&app, &p, src),
        "params.set" => {
            let a = Array::new();
            for c in list(&p["changes"]) {
                a.push(&param(&app, c, src)?);
            }
            Ok(a.into())
        }
        "preset.list" => {
            let cur = string(&st, "currentFile");
            result(json!(arrays(&app, "presets")?
                .iter()
                .map(|p| json!({"name":p["name"],"bank":p["bank"],"current":p["file"]==cur}))
                .collect::<Vec<_>>()))
        }
        "preset.load" => {
            let presets = arrays(&app, "presets")?;
            let name = text(&p, "name").to_lowercase();
            let x = presets
                .iter()
                .find(|p| text(p, "name").to_lowercase() == name)
                .or_else(|| {
                    presets
                        .iter()
                        .find(|p| text(p, "name").to_lowercase().contains(&name))
                })
                .ok_or_else(|| crate::err("Пресет не найден".into()))?;
            invoke(
                &st,
                "load",
                &[js(&x["data"])?, js(&x["file"])?, js(&x["bank"])?],
            )?;
            result(json!({"loaded":x["name"],"bank":x["bank"]}))
        }
        "preset.save" => {
            call(
                &app,
                "savePreset",
                &[
                    (p["as_new"] == true || !text(&p, "name").is_empty()).into(),
                    js(&p["name"])?,
                    js(&p["bank"])?,
                    js(&json!({"source":src}))?,
                ],
            )
            .await
        }
        "history.undo" | "history.redo" => {
            let redo = method == "history.redo";
            let mut labels = vec![];
            for _ in 0..(p["steps"].as_f64().filter(|n| *n != 0.).unwrap_or(1.) as usize) {
                let l = invoke(&st, if redo { "redo" } else { "undo" }, &[src.into()])?;
                let Some(l) = l.as_string() else {
                    break;
                };
                labels.push(l);
            }
            result(json!({if redo{"redone"}else{"undone"}:labels}))
        }
        "history.list" => {
            let past = arrays(&st, "past")?;
            result(
                json!({"undo":past.iter().skip(past.len().saturating_sub(20)).map(|x|format!("{}{}",text(x,"label"),if x["source"]=="mcp"{" (агент)"}else{""})).collect::<Vec<_>>(),"redo":arrays(&st,"future")?.iter().map(|v|&v["label"]).collect::<Vec<_>>()}),
            )
        }
        "audition.start" => {
            invoke(&app, "stopAudition", &[])?;
            let replace = if let Some(r) = p["replace"].as_str() {
                let uid = target(&app, r)?;
                let b = invoke(&st, "find", &[uid.clone().into()])?;
                if string(&b, "kind") == "pedal" {
                    uid.into()
                } else {
                    JsValue::NULL
                }
            } else {
                JsValue::NULL
            };
            if !invoke(
                &app,
                "startAudition",
                &[pedal.into(), index_from(&app, &p)?.into(), replace],
            )?
            .is_truthy()
            {
                return Err(crate::err("Педаль нельзя прослушать".into()));
            }
            result(json!({"auditioning":pedal}))
        }
        "audition.ab" => {
            let on = p["active"] == true;
            invoke(&app, "setAuditionActive", &[on.into()])?;
            result(json!({"active":on}))
        }
        "audition.stop" => {
            invoke(&app, "stopAudition", &[])?;
            result(json!({"stopped":true}))
        }
        "tone.analyze" => {
            call(
                &engine,
                "analyze",
                &[p["seconds"]
                    .as_f64()
                    .filter(|n| *n > 0.)
                    .unwrap_or(1.5)
                    .into()],
            )
            .await
        }
        "tuner.read" => Ok(get(&get(&app, "tuner"), "reading")),
        "pedal.create" => {
            let mut d = p["pedal"].clone();
            if !d.is_object() {
                return Err(crate::err("Нет описания педали".into()));
            }
            if d.get("format").is_none() {
                d["format"] = json!("guitarcraft-pedal");
            }
            if text(&d, "id").is_empty() {
                let name = text(&d, "name").to_lowercase();
                let mut slug = String::new();
                for c in name.chars() {
                    if c.is_ascii_alphanumeric() || ('а'..='я').contains(&c) {
                        slug.push(c);
                    } else if !slug.ends_with('-') {
                        slug.push('-');
                    }
                }
                d["id"] = json!(format!("user.{slug}"));
            }
            let def = compile_pedal(&d).map_err(crate::err)?;
            let file=command("write_json",&js(&json!({"kind":"pedals","name":format!("{}.gcpedal.json",text(&d,"id")),"data":d}))?).await?;
            call(&cat, "loadUserPedals", &[]).await?;
            result(json!({"created":def["id"],"file":file.as_string()}))
        }
        "di.play" => {
            if let Some(i) = p["index"].as_f64().filter(|i| *i >= 0. && i.fract() == 0.) {
                let d = Array::from(&get(&app, "diList")).get(i as u32);
                if d.is_object() {
                    if get(&d, "file").is_truthy() {
                        call(&engine, "loadDIPath", &[get(&d, "file")]).await?;
                    } else {
                        call(&engine, "loadDI", &[get(&d, "url"), get(&d, "name")]).await?;
                    }
                }
            }
            invoke(&app, "toggleDI", &[true.into()])?;
            result(json!({"playing":string(&engine,"diName")}))
        }
        "di.stop" => {
            invoke(&app, "toggleDI", &[false.into()])?;
            result(json!({"stopped":true}))
        }
        "ui.select" => {
            invoke(&app, "select", &[target(&app, id)?.into()])?;
            Ok(true.into())
        }
        "ui.agentPanel" => {
            el("agentlog")?
                .class_list()
                .toggle_with_force("hidden", p["show"] != true)?;
            Ok(true.into())
        }
        "skin.set" => skins::set_skin(app, text(&p, "id").into()).await,
        "skin.list" => skins::list_skins().await,
        "skin.layoutEdit" => skins::set_editing(app, p["on"] == true),
        "skin.import" => {
            skins::import_skin(
                app,
                text(&p, "file").into(),
                js(&json!({"edit":p["edit"]!=false,"name":p["name"]}))?,
            )
            .await
        }
        "audio.apply" => {
            crate::preferences::apply_audio(app).await?;
            call(&engine, "refreshStatus", &[]).await
        }
        "ui.closeModals" => {
            crate::builder::close_builder();
            crate::browser::close_catalog()?;
            crate::settings::close_settings();
            let nodes = doc().query_selector_all(".modal-bg")?;
            for i in 0..nodes.length() {
                nodes
                    .get(i)
                    .unwrap()
                    .unchecked_into::<web_sys::Element>()
                    .remove();
            }
            Ok(true.into())
        }
        "record.start" => call(&engine, "startRecording", &[]).await,
        "record.stop" => call(&engine, "stopRecording", &[js(&p["name"])?]).await,
        "demo.run" => call(&get(&app, "demo"), "run", &[js(list(&p["steps"]))?]).await,
        "agent.say" => {
            invoke(
                &app,
                "logAgent",
                &[
                    text(&p, "text").into(),
                    p["kind"].as_str().unwrap_or("say").into(),
                ],
            )?;
            Ok(true.into())
        }
        "agent.clear" => {
            el("agentlog-body")?.set_text_content(None);
            el("agentlog")?.class_list().add_1("hidden")?;
            get(&app, "root")
                .unchecked_into::<web_sys::Element>()
                .class_list()
                .remove_1("agent-on")?;
            Ok(true.into())
        }
        "settings.background.list" => js(&crate::backgrounds::list().await?),
        "settings.background.import" => {
            js(&crate::backgrounds::import(&app, text(&p, "scene"), text(&p, "file")).await?)
        }
        "settings.background.select" => {
            crate::backgrounds::select(&app, text(&p, "scene"), p["id"].as_str()).await?;
            Ok(true.into())
        }
        "settings.get" => Ok(get(&app, "settings")),
        "settings.set" => {
            let mut patch = p;
            patch.as_object_mut().unwrap().remove("__source");
            let s = crate::preferences::save_settings(app.clone(), js(&patch)?).await?;
            invoke(&app, "renderFooter", &[])?;
            Ok(s)
        }
        "ui.openSettings" => {
            crate::settings::open_settings(app, p["tab"].as_str().map(str::to_owned))?;
            Ok(true.into())
        }
        "midi.simulate" => {
            let midi = get(&app, "midi");
            call(
                &midi,
                "handle",
                &[
                    js(&p["data"])?,
                    p["from"].as_str().unwrap_or("simulated").into(),
                ],
            )
            .await?;
            result(json!({"mappings":read::<Value>(get(&midi,"mappings"))?}))
        }
        "midi.learn" => {
            invoke(
                &get(&app, "midi"),
                "learn",
                &[js(
                    &json!({"type":p["type"].as_str().unwrap_or("param"),"id":p["id"],"name":p["name"].as_str().unwrap_or(text(&p,"id"))}),
                )?],
            )?;
            Ok(true.into())
        }
        "engine.status" => call(&engine, "refreshStatus", &[]).await,
        "window.info" => {
            let w = web_sys::window().unwrap();
            result(
                json!({"w":num(&w,"innerWidth",0.),"h":num(&w,"innerHeight",0.),"dpr":w.device_pixel_ratio(),"sx":num(&w,"screenX",0.),"sy":num(&w,"screenY",0.)}),
            )
        }
        "eval.dom" => {
            let Some(e) = doc().query_selector(text(&p, "selector"))? else {
                return Ok(JsValue::NULL);
            };
            let r = e.get_bounding_client_rect();
            result(
                json!({"x":r.x(),"y":r.y(),"w":r.width(),"h":r.height(),"text":e.text_content().unwrap_or_default().chars().take(200).collect::<String>()}),
            )
        }
        _ => Err(crate::err(format!("Неизвестный метод {method}"))),
    }
}
fn log(app: &JsValue, msg: String, kind: &str) {
    let _ = invoke(app, "logAgent", &[msg.into(), kind.into()]);
}
fn message(e: &JsValue) -> String {
    get(e, "message")
        .as_string()
        .or_else(|| e.as_string())
        .unwrap_or_else(|| "Ошибка команды".into())
}
fn handler(app: JsValue, internal: bool) -> JsValue {
    Closure::<dyn Fn(String,JsValue)->Promise>::new(move|method:String,params:JsValue|{
        let app=app.clone();future_to_promise(async move{
            let p:Value=if params.is_object(){read(params)?}else{json!({})};if !p.is_object(){return Err(crate::err("Ожидается объект параметров".into()));}
            let src=if internal{"midi"}else{p["__source"].as_str().unwrap_or("api")}.to_owned();
            if src=="mcp"{
                if !get(&get(&app,"settings"),"mcpEnabled").is_truthy(){return Err(crate::err("Доступ агентов выключен. Пользователь может включить его: GuitarCraft → ⚙ Настройки → Агенты (MCP).".into()));}
                if method.starts_with("settings."){return Err(crate::err("Настройки недоступны агенту".into()));}
                set(&app,"lastAgentCall",&js_sys::Date::now().into());
                let verb=match method.as_str(){"catalog.search"=>"ищет в каталоге","chain.add"=>"добавляет педаль","chain.move"=>"переставляет блок","chain.remove"=>"убирает","chain.replace"=>"заменяет","param.set"=>"меняет параметр","params.set"=>"меняет параметры","preset.save"=>"сохраняет пресет","history.undo"=>"отменяет","tone.analyze"=>"слушает и анализирует звук","state.get"=>"смотрит цепочку","chain.toggle"=>"переключает блок","pedal.create"=>"собирает педаль",v=>v};
                let args=p.as_object().unwrap().iter().filter(|(k,v)|*k!="__source" && !v.is_null() && !v.is_object()&&!v.is_array()).map(|(k,v)|format!("{k}: {}",v.as_str().map(str::to_owned).unwrap_or_else(||v.to_string()))).collect::<Vec<_>>().join(", ");
                log(&app,if args.is_empty(){verb.to_owned()}else{format!("{verb} ({args})")},"call");
            }
            let res=dispatch(app.clone(),&method,p,&src).await;
            if src=="mcp"{match &res{Err(e)=>log(&app,format!("⚠ {}",message(e)),"error"),Ok(v)if method=="tone.analyze"=>{
                let b=get(v,"bands_relative_db");let d=num(&get(v,"deviation_from_reference_db"),"low",0.);log(&app,format!("низ {} дБ (к эталону {}{d}) · середина {} · верх {}",num(&b,"low",0.),if d>0.{"+"}else{""},num(&b,"mid",0.),num(&b,"highmid",0.)),"result");
            }_=>{}}}res
        })
    }).into_js_value()
}
pub async fn install(app: JsValue) -> Result<(), JsValue> {
    set(&app, "rpc", &handler(app.clone(), true));
    let dispatch = handler(app, false).unchecked_into::<js_sys::Function>();
    let events = get(&get(&web_sys::window().unwrap(), "__TAURI__"), "event");
    let cb = Closure::<dyn Fn(JsValue)>::new(move |event: JsValue| {
        let payload = get(&event, "payload");
        let id = get(&payload, "id");
        let dispatch = dispatch.clone();
        crate::desktop::task(async move {
            let r = async {
                resolved(dispatch.call2(
                    &JsValue::UNDEFINED,
                    &get(&payload, "method"),
                    &get(&payload, "params"),
                )?)
                .await
            }
            .await;
            let args = js_sys::Object::new();
            set(&args, "id", &id);
            set(&args, "ok", &r.is_ok().into());
            match r {
                Ok(v) => set(
                    &args,
                    "result",
                    &if v.is_undefined() { JsValue::NULL } else { v },
                ),
                Err(e) => set(&args, "error", &message(&e).into()),
            }
            command("rpc_response", &args).await.map(drop)
        });
    });
    resolved(invoke(
        &events,
        "listen",
        &["rpc:request".into(), cb.into_js_value()],
    )?)
    .await?;
    Ok(())
}
