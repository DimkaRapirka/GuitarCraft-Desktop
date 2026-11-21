use crate::{dom::*, host, js, preferences::save_settings, read};
use gc_model::{
    catalog::{list, text},
    EQUIPMENT,
};
use js_sys::{Array, Function, Promise};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::{future_to_promise, spawn_local};

struct Input {
    app: JsValue,
    ports: RefCell<Vec<String>>,
    last: RefCell<Value>,
    learning: RefCell<Option<Value>>,
    sequence: Cell<u64>,
}
impl Input {
    fn prefs(&self) -> JsValue {
        get(&self.app, "settings")
    }
    fn mappings(&self) -> Vec<Value> {
        read(get(&self.prefs(), "midiMappings")).unwrap_or_default()
    }
    fn toast(&self, text: String) {
        let _ = invoke(&self.app, "toast", &[text.into()]);
    }
    fn store(&self) -> JsValue {
        get(&self.app, "store")
    }
    fn pedal(&self, uid: &str) -> Option<JsValue> {
        let block = invoke(&self.store(), "find", &[uid.into()]).ok()?;
        let id = get(&block, "pedal");
        if !id.is_string() {
            return None;
        }
        invoke(&get(&self.app, "catalog"), "get", &[id])
            .ok()
            .filter(|p| p.is_object())
    }
    fn owner(&self, id: &str) -> String {
        match id {
            "amp" => "Усилитель",
            "cab" => "Кабинет",
            "eq" => "EQ",
            "delay" => "Delay",
            "reverb" => "Reverb",
            "gate" => "Gate",
            "input" => "Вход",
            "output" => "Выход",
            _ => {
                return self
                    .pedal(id)
                    .map(|p| string(&p, "name"))
                    .unwrap_or_else(|| id.into())
            }
        }
        .into()
    }
    fn apply(&self, m: &Value, t: f64) -> Result<(), JsValue> {
        let id = text(m, "id");
        if m["type"] == "bypass" {
            let store = self.store();
            let s = get(&store, "state");
            let block = invoke(&store, "find", &[id.into()])?;
            let state = if id == "gate" {
                get(&get(&s, "input"), "gate")
            } else if string(&block, "kind") == "pedal" {
                block
            } else {
                get(&s, id)
            };
            if !state.is_object() {
                return Ok(());
            }
            if get(&state, "on").as_bool().unwrap_or(true) != (t >= 0.5) {
                invoke(&self.app, "toggleBlock", &[id.into(), "midi".into()])?;
            }
            return Ok(());
        }
        let Some((head, key)) = id.split_once('.') else {
            return Ok(());
        };
        let (min, max, log) = match id {
            "gate.threshold" => (-96., -10., false),
            "gate.release" => (5., 1000., true),
            "input.gain" => (-24., 24., false),
            "input.transpose" => (-12., 12., false),
            "output.gain" => (-36., 12., false),
            "output.doubler" => (3., 35., false),
            _ if head == "amp" || head == "cab" => (0., 1., false),
            _ => {
                let def = if let Some(d) = list(&EQUIPMENT["params"][head])
                    .iter()
                    .find(|p| p["id"] == key)
                {
                    Some(d.clone())
                } else {
                    self.pedal(head)
                        .and_then(|p| read::<Vec<Value>>(get(&p, "params")).ok())
                        .and_then(|ps| ps.into_iter().find(|p| p["id"] == key))
                };
                let Some(def) = def else { return Ok(()) };
                let (lo, hi) = gc_model::rig::bounds(&def);
                (lo, hi, def["log"] == true)
            }
        };
        let v = if log {
            min * (max / min).powf(t)
        } else {
            min + (max - min) * t
        };
        invoke(
            &self.app,
            "rpc",
            &[
                "param.set".into(),
                js(
                    &json!({"target":head,"param":key,"value":if id=="input.transpose"{v.round()}else{v}}),
                )?,
            ],
        )?;
        Ok(())
    }
    async fn handle(&self, data: Vec<u8>, from: String) -> Result<(), JsValue> {
        self.last
            .replace(json!({"data":data,"from":from,"t":js_sys::Date::now()}));
        let [status, number, ..] = data.as_slice() else {
            return Ok(());
        };
        let typ = status & 0xf0;
        let ch = status & 0xf;
        if typ == 0xb0 {
            let Some(&raw) = data.get(2) else {
                return Ok(());
            };
            let learning = self.learning.take();
            if let Some(mut m) = learning {
                let device = if from.is_empty() { "MIDI" } else { &from };
                m["cc"] = json!(number);
                m["ch"] = json!(ch);
                m["device"] = json!(from);
                m["label"] = json!(format!("{device} · Ch {} · CC{number}", ch + 1));
                let message = format!("MIDI: {} → {}", text(&m, "label"), text(&m, "name"));
                let mut maps = self.mappings();
                maps.retain(|other| other["id"] != m["id"] || other["type"] != m["type"]);
                maps.push(m);
                save_settings(self.app.clone(), js(&json!({"midiMappings":maps}))?).await?;
                self.toast(message);
            } else {
                for m in self.mappings() {
                    let d = text(&m, "device");
                    if m["cc"] == *number
                        && m["ch"] == ch
                        && (d.is_empty() || d == "*" || d == from)
                    {
                        self.apply(&m, f64::from(raw.min(127)) / 127.)?;
                    }
                }
            }
        } else if typ == 0xc0 && get(&self.prefs(), "midiPC").as_bool() != Some(false) {
            let map: Value = read(get(&self.prefs(), "midiPCMap")).unwrap_or(Value::Null);
            let presets = Array::from(&get(&self.app, "presets"));
            let selected = if map.as_object().is_some_and(|v| !v.is_empty()) {
                let mapped = map[number.to_string()].as_str();
                let found =
                    mapped.and_then(|file| presets.iter().find(|v| string(v, "file") == file));
                if found.is_none() {
                    self.toast(format!(
                        "MIDI PC {number}: {}",
                        if mapped.is_some() {
                            "пресет не найден"
                        } else {
                            "не назначен"
                        }
                    ));
                }
                found
            } else {
                Some(presets.get(u32::from(*number))).filter(|p| p.is_object())
            };
            if let Some(p) = selected {
                invoke(
                    &self.store(),
                    "load",
                    &[get(&p, "data"), get(&p, "file"), get(&p, "bank")],
                )?;
            }
        }
        Ok(())
    }
}
#[wasm_bindgen(js_name=MIDI)]
pub struct Midi {
    inner: Rc<Input>,
    ready: Promise,
    unlisten: Rc<RefCell<Vec<Function>>>,
}
#[wasm_bindgen(js_class=MIDI)]
impl Midi {
    #[wasm_bindgen(constructor)]
    pub fn new(app: JsValue) -> Midi {
        let inner = Rc::new(Input {
            app,
            ports: RefCell::new(vec![]),
            last: RefCell::new(Value::Null),
            learning: RefCell::new(None),
            sequence: Cell::new(0),
        });
        let weak = Rc::downgrade(&inner);
        let unlisten: Rc<RefCell<Vec<Function>>> = Rc::default();
        let subscriptions = unlisten.clone();
        let ready = future_to_promise(async move {
            let events = get(&get(&web_sys::window().unwrap(), "__TAURI__"), "event");
            for name in ["midi", "midi-devices"] {
                let listener = weak.clone();
                let callback = Closure::wrap(Box::new(move |event: JsValue| {
                    let Some(s) = listener.upgrade() else { return };
                    let payload = get(&event, "payload");
                    if name == "midi-devices" {
                        if let Ok(names) = read(payload) {
                            s.ports.replace(names);
                        }
                    } else if let Ok(data) = read(get(&payload, "data")) {
                        let from = string(&payload, "from");
                        spawn_local(async move {
                            if let Err(e) = s.handle(data, from).await {
                                web_sys::console::warn_1(&e)
                            }
                        });
                    }
                }) as Box<dyn FnMut(JsValue)>)
                .into_js_value();
                let f = host::resolved(invoke(&events, "listen", &[name.into(), callback])?)
                    .await?
                    .dyn_into::<Function>()?;
                if weak.upgrade().is_none() {
                    let _ = f.call0(&JsValue::UNDEFINED);
                } else {
                    subscriptions.borrow_mut().push(f);
                }
            }
            Ok(JsValue::UNDEFINED)
        });
        Midi {
            inner,
            ready,
            unlisten,
        }
    }
    #[wasm_bindgen(getter)]
    pub fn ready(&self) -> Promise {
        self.ready.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn inputs(&self) -> Result<JsValue, JsValue> {
        js(&self
            .inner
            .ports
            .borrow()
            .iter()
            .map(|n| json!({"name":n,"state":"connected"}))
            .collect::<Vec<_>>())
    }
    #[wasm_bindgen(getter)]
    pub fn last(&self) -> Result<JsValue, JsValue> {
        js(&*self.inner.last.borrow())
    }
    #[wasm_bindgen(getter)]
    pub fn mappings(&self) -> Result<JsValue, JsValue> {
        js(&self.inner.mappings())
    }
    #[wasm_bindgen(js_name=mappingFor)]
    pub fn mapping_for(&self, id: &str) -> Result<JsValue, JsValue> {
        js(&self.inner.mappings().iter().find(|m| m["id"] == id))
    }
    pub fn learn(&self, target: JsValue) -> Result<(), JsValue> {
        let mut target: Value = read(target)?;
        let id = text(&target, "id");
        let name = text(&target, "name");
        let title = if target["type"] == "bypass" {
            format!("{name} › вкл/выкл")
        } else {
            format!(
                "{} › {name}",
                self.inner.owner(id.split('.').next().unwrap_or(id))
            )
        };
        target["name"] = json!(title);
        self.inner.learning.replace(Some(target));
        self.inner.toast(format!(
            "MIDI Learn: «{title}». Подвигайте регулятор на контроллере."
        ));
        let seq = self.inner.sequence.get().wrapping_add(1);
        self.inner.sequence.set(seq);
        let weak = Rc::downgrade(&self.inner);
        spawn_local(async move {
            let _ = host::sleep(15000.).await;
            if let Some(s) = weak.upgrade().filter(|s| s.sequence.get() == seq) {
                if s.learning.take().is_some() {
                    s.toast("MIDI Learn отменён: сообщений не было".into());
                }
            }
        });
        Ok(())
    }
    pub async fn unmap(&self, id: String) -> Result<(), JsValue> {
        let mut maps = self.inner.mappings();
        maps.retain(|m| m["id"] != id);
        save_settings(self.inner.app.clone(), js(&json!({"midiMappings":maps}))?).await?;
        self.inner.toast("MIDI-назначение убрано".into());
        Ok(())
    }
    pub async fn handle(&self, data: JsValue, from: Option<String>) -> Result<(), JsValue> {
        self.inner
            .handle(read(data)?, from.unwrap_or_default())
            .await
    }
}
impl Drop for Midi {
    fn drop(&mut self) {
        for f in self.unlisten.take() {
            let _ = f.call0(&JsValue::UNDEFINED);
        }
    }
}
