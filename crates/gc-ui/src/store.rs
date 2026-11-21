//! Project notifications and the editable snapshot displayed in the current WebView.
use crate::{dom::*, js};
use js_sys::{Array, Function, Set};
use serde_json::json;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Store {
    project: JsValue,
    state: RefCell<JsValue>,
    listeners: Set,
}
#[wasm_bindgen]
impl Store {
    #[wasm_bindgen(constructor)]
    pub fn new(project: JsValue) -> Result<Store, JsValue> {
        Ok(Self {
            state: RefCell::new(invoke(&project, "state", &[])?),
            project,
            listeners: Set::new(&JsValue::UNDEFINED),
        })
    }
    #[wasm_bindgen(getter)]
    pub fn state(&self) -> JsValue {
        self.state.borrow().clone()
    }
    #[wasm_bindgen(setter)]
    pub fn set_state(&self, state: JsValue) {
        self.state.replace(state);
    }
    #[wasm_bindgen(getter)]
    pub fn dirty(&self) -> Result<JsValue, JsValue> {
        invoke(&self.project, "dirty", &[])
    }
    #[wasm_bindgen(getter,js_name=currentFile)]
    pub fn file(&self) -> Result<JsValue, JsValue> {
        let v = invoke(&self.project, "file", &[])?;
        Ok(if v.is_undefined() { JsValue::NULL } else { v })
    }
    #[wasm_bindgen(getter,js_name=currentBank)]
    pub fn bank(&self) -> Result<JsValue, JsValue> {
        invoke(&self.project, "bank", &[])
    }
    #[wasm_bindgen(getter)]
    pub fn past(&self) -> Result<JsValue, JsValue> {
        invoke(&self.project, "history", &[false.into()])
    }
    #[wasm_bindgen(getter)]
    pub fn future(&self) -> Result<JsValue, JsValue> {
        invoke(&self.project, "history", &[true.into()])
    }
    #[wasm_bindgen(getter,js_name=savedJSON)]
    pub fn saved_json(&self) -> Result<String, JsValue> {
        Ok(
            js_sys::JSON::stringify(&invoke(&self.project, "saved", &[])?)?
                .as_string()
                .unwrap(),
        )
    }
    pub fn subscribe(&self, listener: Function) -> JsValue {
        self.listeners.add(&listener);
        let set = self.listeners.clone();
        Closure::once_into_js(move || {
            set.delete(&listener);
        })
    }
    pub fn notify(&self, kind: &str, meta: JsValue) -> Result<(), JsValue> {
        for fun in Array::from(&self.listeners) {
            Function::from(fun).call2(&JsValue::UNDEFINED, &kind.into(), &meta)?;
        }
        Ok(())
    }
    pub fn changed(&self, kind: &str, meta: JsValue) -> Result<(), JsValue> {
        self.set_state(invoke(&self.project, "state", &[])?);
        self.notify(kind, meta)
    }
    pub fn commit(&self, label: &str, mutate: Function, opts: JsValue) -> Result<(), JsValue> {
        let source = get(&opts, "source")
            .as_string()
            .unwrap_or_else(|| "ui".into());
        let kind = get(&opts, "kind")
            .as_string()
            .unwrap_or_else(|| "params".into());
        let state = self.state();
        let result = mutate.call1(&JsValue::UNDEFINED, &state).and_then(|_| {
            invoke(
                &self.project,
                "edit",
                &[
                    state,
                    label.into(),
                    source.clone().into(),
                    get(&opts, "coalesceKey"),
                ],
            )
        });
        if let Err(e) = result {
            self.set_state(invoke(&self.project, "state", &[])?);
            return Err(e);
        }
        self.notify(&kind, js(&json!({"label":label,"source":source}))?)
    }
    pub fn undo(&self, source: Option<String>) -> Result<JsValue, JsValue> {
        self.history_step(false, source)
    }
    pub fn redo(&self, source: Option<String>) -> Result<JsValue, JsValue> {
        self.history_step(true, source)
    }
    pub fn load(
        &self,
        preset: JsValue,
        file: Option<String>,
        bank: Option<String>,
    ) -> Result<(), JsValue> {
        invoke(
            &self.project,
            "load",
            &[
                preset.clone(),
                file.into(),
                bank.unwrap_or_else(|| "Мои".into()).into(),
            ],
        )?;
        self.changed(
            "structure",
            js(&json!({"label":format!("Пресет «{}»",string(&preset,"name")),"preset":true}))?,
        )
    }
    #[wasm_bindgen(js_name=markSaved)]
    pub fn mark_saved(&self, file: String, bank: String) -> Result<(), JsValue> {
        invoke(
            &self.project,
            "edit",
            &[
                self.state(),
                "Сохранение пресета".into(),
                "ui".into(),
                JsValue::NULL,
            ],
        )?;
        invoke(&self.project, "mark_saved", &[file.into(), bank.into()])?;
        self.notify("saved", js(&json!({}))?)
    }
    pub fn find(&self, uid: &str) -> JsValue {
        self.chain()
            .iter()
            .find(|b| string(b, "uid") == uid)
            .unwrap_or(JsValue::UNDEFINED)
    }
    pub fn index(&self, uid: &str) -> i32 {
        self.chain()
            .iter()
            .position(|b| string(&b, "uid") == uid)
            .map(|n| n as i32)
            .unwrap_or(-1)
    }
    pub fn pedals(&self) -> Array {
        self.chain()
            .iter()
            .filter(|b| string(b, "kind") == "pedal")
            .collect()
    }
    #[wasm_bindgen(js_name=resolveTarget)]
    pub fn target(&self, id: &str) -> Result<JsValue, JsValue> {
        let Some(uid) = invoke(&self.project, "target", &[id.into()])?.as_string() else {
            return Ok(JsValue::NULL);
        };
        if matches!(
            uid.as_str(),
            "gate" | "input" | "output" | "amp" | "cab" | "eq" | "delay" | "reverb"
        ) {
            js(&json!({"core":uid}))
        } else {
            let o = js_sys::Object::new();
            set(&o, "pedal", &self.find(&uid));
            Ok(o.into())
        }
    }
}
impl Store {
    fn chain(&self) -> Array {
        Array::from(&get(&self.state(), "chain"))
    }
    fn history_step(&self, redo: bool, source: Option<String>) -> Result<JsValue, JsValue> {
        let l = invoke(&self.project, if redo { "redo" } else { "undo" }, &[])?;
        if let Some(label) = l.as_string() {
            self.changed("structure",js(&json!({"label":format!("{}: {label}",if redo{"Повторено"}else{"Отменено"}),"source":source.as_deref().unwrap_or("ui"),if redo{"redo"}else{"undo"}:true}))?)?;
        }
        Ok(l)
    }
}
#[wasm_bindgen(js_name=defaultRig)]
pub fn default_rig(name: Option<String>) -> Result<JsValue, JsValue> {
    js(&gc_model::default_rig(name.as_deref().unwrap_or("Default")))
}
#[wasm_bindgen(js_name=factoryPresets)]
pub fn factory_presets() -> Result<JsValue, JsValue> {
    js(&gc_model::factory_presets())
}

/// Commit a synchronous UI edit using the shared project history, then notify the view.
pub fn edit(
    app: &JsValue,
    label: &str,
    key: Option<&str>,
    change: impl FnOnce(&mut serde_json::Value),
) -> Result<(), JsValue> {
    let store = get(app, "store");
    let mut doc: serde_json::Value = crate::read(get(&store, "state"))?;
    change(&mut doc);
    invoke(
        &get(app, "catalog"),
        "edit",
        &[
            js(&doc)?,
            label.into(),
            "ui".into(),
            key.map(crate::knob::gesture_key).into(),
        ],
    )?;
    invoke(
        &store,
        "changed",
        &["params".into(), js(&json!({"source":"ui","label":label}))?],
    )?;
    Ok(())
}
pub fn state(app: &JsValue) -> Result<serde_json::Value, JsValue> {
    crate::read(get(&get(app, "store"), "state"))
}
