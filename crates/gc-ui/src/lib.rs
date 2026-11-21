//! The UI uses the platform WebView as its renderer. Rust owns the project model and creates
//! ordinary DOM controls so the two desktop platforms share artwork, CSS and accessibility.
use gc_model::{
    catalog::{compile_pedal, SEARCH},
    rig::Rig,
    EQUIPMENT, PEDALS,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use wasm_bindgen::prelude::*;

fn js<T: Serialize + ?Sized>(v: &T) -> Result<JsValue, JsValue> {
    v.serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|e| JsValue::from_str(&e.to_string()))
}
fn read<T: DeserializeOwned>(v: JsValue) -> Result<T, JsValue> {
    serde_wasm_bindgen::from_value(v).map_err(|e| JsValue::from_str(&e.to_string()))
}
fn err(e: String) -> JsValue {
    js_sys::Error::new(&e).into()
}

#[wasm_bindgen]
pub fn factory_data() -> Result<JsValue, JsValue> {
    js(
        &json!({"rig":gc_model::default_rig("Default"),"presets":gc_model::factory_presets(),"pedals":*PEDALS,"equipment":*EQUIPMENT,"formats":SEARCH["formats"],"skins":serde_json::from_str::<Value>(include_str!("../../gc-model/data/skins.json")).unwrap()}),
    )
}

#[wasm_bindgen]
pub fn compile_pedal_definition(raw: JsValue) -> Result<JsValue, JsValue> {
    js(&compile_pedal(&read(raw)?).map_err(err)?)
}

#[wasm_bindgen]
pub struct Project {
    rig: RefCell<Rig>,
    catalog_epochs: [Cell<u64>; 3],
}
#[wasm_bindgen]
impl Project {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Project {
        let mut rig = Rig::default();
        rig.catalog.prefs = catalog::preferences();
        Project {
            rig: RefCell::new(rig),
            catalog_epochs: Default::default(),
        }
    }
    pub fn state(&self) -> Result<JsValue, JsValue> {
        js(self.rig.borrow().session.rig())
    }
    pub fn saved(&self) -> Result<JsValue, JsValue> {
        js(self.rig.borrow().session.saved())
    }
    pub fn dirty(&self) -> bool {
        self.rig.borrow().session.dirty()
    }
    pub fn file(&self) -> Option<String> {
        self.rig.borrow().session.file().map(str::to_owned)
    }
    pub fn bank(&self) -> String {
        self.rig.borrow().session.bank().into()
    }
    pub fn history(&self, redo: bool) -> Result<JsValue, JsValue> {
        if redo {
            js(&self
                .rig
                .borrow()
                .session
                .redo_labels()
                .map(|s| json!({"label":s}))
                .collect::<Vec<_>>())
        } else {
            js(&self
                .rig
                .borrow()
                .session
                .undo_labels()
                .map(|(l, s)| json!({"label":l,"source":s}))
                .collect::<Vec<_>>())
        }
    }
    pub fn clear_history(&self) {
        self.rig.borrow_mut().session.clear_history();
    }
    pub fn load(&self, doc: JsValue, file: Option<String>, bank: &str) -> Result<(), JsValue> {
        self.rig
            .borrow_mut()
            .session
            .load(&read(doc)?, file, bank)
            .map_err(err)
    }
    pub fn mark_saved(&self, file: String, bank: &str) {
        self.rig.borrow_mut().session.mark_saved(file, bank);
    }
    pub fn save_baseline(
        &self,
        file: String,
        bank: &str,
        snapshot: JsValue,
    ) -> Result<(), JsValue> {
        self.rig
            .borrow_mut()
            .session
            .mark_snapshot(file, bank, read(snapshot)?);
        Ok(())
    }
    pub fn undo(&self) -> Option<String> {
        self.rig.borrow_mut().session.undo()
    }
    pub fn redo(&self) -> Option<String> {
        self.rig.borrow_mut().session.redo()
    }
    pub fn edit(
        &self,
        state: JsValue,
        label: &str,
        source: &str,
        key: Option<String>,
    ) -> Result<(), JsValue> {
        let doc = read(state)?;
        self.rig
            .borrow_mut()
            .session
            .edit(label, source, key.as_deref(), |r| {
                *r = doc;
                Ok(())
            })
            .map_err(err)
    }
    pub fn chain(&self, method: &str, args: JsValue, source: &str) -> Result<JsValue, JsValue> {
        let mut r = self.rig.borrow_mut();
        let out = r.chain_action(method, &read(args)?, source).map_err(err)?;
        if matches!(method, "chain.add" | "chain.replace") {
            catalog::persist("gc.recent", &r.catalog.prefs.recent);
        }
        js(&out)
    }
    pub fn set_param(
        &self,
        id: &str,
        key: &str,
        v: JsValue,
        source: &str,
        gesture: Option<String>,
    ) -> Result<JsValue, JsValue> {
        js(&self
            .rig
            .borrow_mut()
            .set_param(id, key, &read(v)?, source, gesture.as_deref())
            .map_err(err)?)
    }
    pub fn target(&self, id: &str) -> Option<String> {
        self.rig.borrow().target(id).ok()
    }
    pub fn audio_rig(
        &self,
        current: JsValue,
        audition: JsValue,
        bpm: f64,
    ) -> Result<JsValue, JsValue> {
        let current: Value = read(current)?;
        let mut a: Option<Value> = read(audition)?;
        if let Some(a) = a.as_mut() {
            let defaults = if a["def"].is_object() {
                compile_pedal(&a["def"]).ok()
            } else {
                self.rig
                    .borrow()
                    .catalog
                    .get(a["pedalId"].as_str().unwrap_or(""))
                    .cloned()
            };
            if let Some(d) = defaults {
                a["params"] = gc_model::rig::parameters(&d, &a["params"]);
            }
        }
        js(&self.rig.borrow().prepare_audio(&current, a.as_ref(), bpm))
    }
}

mod dom;
mod i18n;
mod knob;

mod pedal;

mod host;
mod tuner;

mod demo;

mod preferences;

mod midi;

mod assets;

mod catalog;

mod browser;

mod builder;

mod engine;

mod settings;

mod store;

mod editor;

mod cab;

mod racks;

mod skins;

mod rpc;

mod chain;
mod desktop;

mod presets;

mod transport;

mod start;
mod updates;

mod backgrounds;

mod wear;
