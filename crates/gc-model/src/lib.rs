//! GuitarCraft project data. Persisted rigs are open JSON documents: future fields and
//! third-party pedal parameters must survive loading, editing and saving in older clients.
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    LazyLock,
};

pub const CORE: [&str; 5] = ["amp", "cab", "eq", "delay", "reverb"];
pub static EQUIPMENT: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../data/equipment.json")).expect("equipment metadata")
});
pub static PEDALS: LazyLock<Vec<Value>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../data/pedals.json")).expect("pedal metadata")
});
static DEFAULT: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../data/default-rig.json")).expect("default rig")
});
static FACTORY: LazyLock<Vec<Value>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../data/factory-presets.json")).expect("factory presets")
});

pub fn default_rig(name: &str) -> Value {
    let mut rig = DEFAULT.clone();
    rig["name"] = json!(name);
    rig
}

pub fn factory_presets() -> Vec<Value> {
    FACTORY
        .iter()
        .map(|preset| normalize(preset).expect("factory rig"))
        .collect()
}

pub fn new_uid() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let now = web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("p{now:x}-{:x}", NEXT.fetch_add(1, Ordering::Relaxed))
}

fn overlay(base: &mut Value, overrides: &Value) {
    if let (Some(dest), Some(src)) = (base.as_object_mut(), overrides.as_object()) {
        for (key, value) in src {
            overlay(dest.entry(key).or_insert(Value::Null), value);
        }
    } else {
        *base = overrides.clone();
    }
}

pub fn normalize(preset: &Value) -> Result<Value, String> {
    if !preset.is_object() {
        return Err("Ожидается объект пресета".into());
    }
    let mut rig = DEFAULT.clone();
    overlay(&mut rig, preset);
    if preset["headSkin"].is_null() {
        if let Some(skin) = preset["skin"].as_str().filter(|skin| *skin != "ember") {
            rig["headSkin"] = json!(skin);
        }
    }
    let chain = rig["chain"]
        .as_array_mut()
        .ok_or("Цепочка пресета должна быть массивом")?;
    let mut used: HashSet<String> = CORE.into_iter().map(String::from).collect();
    // Reserve existing IDs before repairing missing ones so the repair cannot shadow a later block.
    let reserved: HashSet<String> = chain
        .iter()
        .filter_map(|b| b["uid"].as_str().map(str::to_owned))
        .collect();
    for block in chain.iter_mut() {
        let kind = block["kind"]
            .as_str()
            .ok_or("У блока отсутствует kind")?
            .to_owned();
        if kind == "pedal" {
            let valid = block["uid"]
                .as_str()
                .is_some_and(|id| !id.is_empty() && used.insert(id.into()));
            if !valid {
                let id = loop {
                    let id = new_uid();
                    if !reserved.contains(&id) && used.insert(id.clone()) {
                        break id;
                    }
                };
                block["uid"] = json!(id);
            }
            if block["params"].is_null() {
                block["params"] = json!({});
            }
        } else if CORE.contains(&kind.as_str()) {
            block["uid"] = json!(kind);
        }
    }
    for kind in CORE {
        if !chain.iter().any(|b| b["kind"] == kind) {
            chain.push(json!({"uid":kind,"kind":kind}));
        }
    }
    Ok(rig)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_missing_fields_and_preserves_custom_data_and_ids() {
        let original = json!({"skin":"walnut","extra":{"v":7},"amp":{"channels":{"crunch":{"mid":0.2}}},"chain":[
            {"kind":"pedal","pedal":"user.test","uid":"abc","params":{"custom":42}},
            {"kind":"pedal","pedal":"user.test","uid":"abc"},{"kind":"pedal","pedal":"gc.lagoon"}]});
        let rig = normalize(&original).unwrap();
        assert_eq!(rig["extra"], original["extra"]);
        assert_eq!(rig["headSkin"], "walnut");
        assert_eq!(rig["amp"]["channels"]["crunch"]["mid"], 0.2);
        assert_eq!(rig["amp"]["channels"]["crunch"]["gain"], 0.5);
        assert_eq!(rig["chain"][0], original["chain"][0]);
        let chain = rig["chain"].as_array().unwrap();
        assert_eq!(chain.len(), 8);
        assert_eq!(
            chain
                .iter()
                .map(|b| b["uid"].as_str().unwrap())
                .collect::<HashSet<_>>()
                .len(),
            8
        );
        assert_eq!(normalize(&rig).unwrap(), rig);
    }

    #[test]
    fn factory_presets_and_pedal_art_are_available() {
        assert_eq!(PEDALS.len(), 25);
        let artwork: Vec<_> = PEDALS
            .iter()
            .filter_map(|p| p["artwork"].as_str())
            .collect();
        assert_eq!(artwork.len(), 17);
        let frontend = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app");
        for image in artwork {
            assert!(
                frontend.join(image).is_file(),
                "Missing pedal artwork {image}"
            );
        }
        let factory = factory_presets();
        assert_eq!(factory.len(), 5);
        assert_eq!(factory[0]["chain"][0]["pedal"], "gc.squash");
        assert_ne!(
            factory[0]["chain"][0]["uid"],
            factory_presets()[0]["chain"][0]["uid"]
        );
        assert!(factory
            .iter()
            .all(|p| p["factory"] == true && normalize(p).unwrap() == *p));
    }
}

pub mod session;

pub mod catalog;

pub mod rig;
