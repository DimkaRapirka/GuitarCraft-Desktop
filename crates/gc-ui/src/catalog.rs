use crate::{err, host, js, read, Project};
use gc_model::catalog::Preferences;
use serde_json::{json, Value};
use wasm_bindgen::prelude::*;

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}
fn load(key: &str) -> Vec<String> {
    storage()
        .and_then(|s| s.get_item(key).ok().flatten())
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default()
}
pub(crate) fn preferences() -> Preferences {
    Preferences {
        favorites: load("gc.favorites"),
        recent: load("gc.recent"),
    }
}
pub(crate) fn persist(key: &str, value: &[String]) {
    if let Some(storage) = storage() {
        if let Err(e) = storage.set_item(key, &serde_json::to_string(value).unwrap()) {
            web_sys::console::warn_1(&e);
        }
    }
}

impl Project {
    async fn refresh_source(&self, kind: &str, index: usize) -> Result<(), JsValue> {
        let epoch = &self.catalog_epochs[index];
        let generation = epoch.get().wrapping_add(1);
        epoch.set(generation);
        let defs: Vec<Value> =
            read(host::command("catalog_discover", &js(&json!({"source":kind}))?).await?)?;
        if epoch.get() != generation {
            return Ok(());
        }
        if kind == "user" {
            let raw = defs
                .iter()
                .map(|d| {
                    let mut r = d["json"].clone();
                    r["id"] = d["id"].clone();
                    r
                })
                .collect::<Vec<_>>();
            host::command("engine_user_pedals", &js(&json!({"defs":raw}))?).await?;
            if epoch.get() != generation {
                return Ok(());
            }
        }
        let mut rig = self.rig.borrow_mut();
        rig.catalog.items.retain(|p| p["source"] != kind);
        for d in defs {
            rig.catalog.add(d);
        }
        Ok(())
    }
}
#[wasm_bindgen]
impl Project {
    pub fn get(&self, id: &str) -> Result<JsValue, JsValue> {
        js(&self.rig.borrow().catalog.get(id))
    }
    pub fn add(&self, def: JsValue) -> Result<(), JsValue> {
        let d: Value = read(def)?;
        if !d["id"].is_string() {
            return Err(err("Нет идентификатора педали".into()));
        }
        self.rig.borrow_mut().catalog.add(d);
        Ok(())
    }
    pub fn all(&self) -> Result<JsValue, JsValue> {
        js(&self.rig.borrow().catalog.items)
    }
    pub fn search(&self, q: &str, filters: JsValue) -> Result<JsValue, JsValue> {
        let filters = if filters.is_undefined() {
            Value::Null
        } else {
            read(filters)?
        };
        js(&self.rig.borrow().catalog.search(q, &filters))
    }
    pub fn summary(&self, def: JsValue) -> Result<JsValue, JsValue> {
        js(&self.rig.borrow().catalog.summary(&read(def)?))
    }
    #[wasm_bindgen(js_name=isFavorite)]
    pub fn is_favorite(&self, id: &str) -> bool {
        self.rig
            .borrow()
            .catalog
            .prefs
            .favorites
            .iter()
            .any(|s| s == id)
    }
    #[wasm_bindgen(js_name=toggleFavorite)]
    pub fn toggle_favorite(&self, id: &str) -> bool {
        let mut r = self.rig.borrow_mut();
        let on = r.catalog.favorite(id);
        persist("gc.favorites", &r.catalog.prefs.favorites);
        on
    }
    #[wasm_bindgen(js_name=loadUserPedals)]
    pub async fn load_user_pedals(&self) -> Result<(), JsValue> {
        self.refresh_source("user", 0).await
    }
    #[wasm_bindgen(js_name=loadExtensions)]
    pub async fn load_extensions(&self) -> Result<(), JsValue> {
        self.refresh_source("extension", 1).await
    }
    #[wasm_bindgen(js_name=scanPlugins)]
    pub async fn scan_plugins(&self) -> Result<(), JsValue> {
        self.refresh_source("plugin", 2).await
    }
}
