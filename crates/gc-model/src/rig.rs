use crate::{
    catalog::{list, text, Catalog},
    new_uid,
    session::Session,
    CORE, EQUIPMENT,
};
use serde_json::{json, Value};
use std::path::Path;

pub struct Rig {
    pub session: Session,
    pub catalog: Catalog,
    pub audition: Option<Value>,
}
impl Default for Rig {
    fn default() -> Self {
        Self {
            session: Session::default(),
            catalog: Catalog::default(),
            audition: None,
        }
    }
}
type Result<T> = std::result::Result<T, String>;

fn finite(value: &Value) -> Result<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
        .filter(|v| v.is_finite())
        .ok_or_else(|| "Параметр должен быть числом".into())
}
pub fn bounds(param: &Value) -> (f64, f64) {
    (
        param["min"].as_f64().unwrap_or(0.),
        param["max"].as_f64().unwrap_or(1.),
    )
}
fn normalized(param: &Value, value: &Value) -> Result<Value> {
    let (min, max) = bounds(param);
    Ok(json!(finite(value)?.clamp(min, max)))
}
pub fn parameters(def: &Value, overrides: &Value) -> Value {
    let mut out = json!({});
    for p in list(&def["params"]) {
        out[text(p, "id")] = p["def"].clone();
    }
    if let Some(obj) = overrides.as_object() {
        out.as_object_mut().unwrap().extend(obj.clone());
    }
    out
}

pub fn sync_time(sync: &str, bpm: f64) -> Option<f64> {
    let div = match sync {
        "1/2" => 2.0,
        "1/4" => 1.0,
        "1/4." => 1.5,
        "1/8" => 0.5,
        "1/8." => 0.75,
        "1/8t" => 1. / 3.,
        "1/16" => 0.25,
        _ => 0.,
    };
    (div > 0. && bpm > 0.).then(|| (60000. / bpm * div).min(3000.))
}

impl Rig {
    pub fn chain(&self) -> &[Value] {
        list(&self.session.rig()["chain"])
    }
    pub fn target(&self, reference: &str) -> Result<String> {
        if CORE.contains(&reference) || ["input", "output", "gate"].contains(&reference) {
            return Ok(reference.into());
        }
        if reference.is_empty() {
            return Err("Укажите блок цепочки".into());
        }
        let search = reference.to_lowercase();
        let by_uid = self.chain().iter().find(|b| b["uid"] == reference);
        let by_name = || {
            self.chain()
                .iter()
                .filter(|b| b["kind"] == "pedal")
                .find(|b| {
                    b["pedal"] == reference
                        || self
                            .catalog
                            .get(text(b, "pedal"))
                            .is_some_and(|d| text(d, "name").to_lowercase().contains(&search))
                })
        };
        by_uid
            .or_else(by_name)
            .map(|b| text(b, "uid").to_owned())
            .ok_or_else(|| format!("Блок «{reference}» не найден"))
    }
    pub fn index(&self, uid: &str) -> Result<usize> {
        self.chain()
            .iter()
            .position(|b| b["uid"] == uid)
            .ok_or_else(|| format!("Блок «{uid}» отсутствует в цепочке"))
    }
    pub fn insert_at(&self, p: &Value) -> Result<usize> {
        if let Some(i) = p["index"].as_i64() {
            return Ok(i.clamp(0, self.chain().len() as i64) as usize);
        }
        for (key, offset) in [("before", 0), ("after", 1)] {
            if let Some(id) = p[key].as_str() {
                return Ok(self.index(&self.target(id)?)? + offset);
            }
        }
        if p["position"] == "end" {
            return Ok(self.chain().len());
        }
        Ok(self.index("amp")? + usize::from(p["position"] == "post-amp"))
    }
    pub fn audio_rig(&self, bpm: f64) -> Value {
        self.prepare_audio(self.session.rig(), self.audition.as_ref(), bpm)
    }
    pub fn prepare_audio(&self, current: &Value, audition: Option<&Value>, bpm: f64) -> Value {
        let mut rig = current.clone();
        if let Some(time) = sync_time(text(&rig["delay"], "sync"), bpm) {
            rig["delay"]["time"] = json!(time);
        }
        for b in rig["chain"].as_array_mut().unwrap() {
            if let Some(d) = self.catalog.get(text(b, "pedal")) {
                b["params"] = parameters(d, &b["params"]);
            }
        }
        if let Some(a) = audition {
            rig["audition"] = a.clone();
        }
        rig
    }
    pub fn chain_names(&self) -> Vec<&str> {
        self.chain()
            .iter()
            .map(|b| {
                self.catalog
                    .get(text(b, "pedal"))
                    .map(|d| text(d, "name"))
                    .unwrap_or_else(|| text(b, "kind"))
            })
            .collect()
    }
    pub fn chain_action(&mut self, op: &str, p: &Value, source: &str) -> Result<Value> {
        let pedal = text(p, "pedal");
        if op == "chain.add" || op == "chain.replace" {
            let def = self.catalog.supported(pedal)?;
            let name = text(def, "name").to_string();
            let params = p["params"].as_object().cloned().unwrap_or_default();
            if op == "chain.add" {
                let at = self.insert_at(p)?;
                let uid = new_uid();
                self.session.edit(&format!("Добавлено: {name}"),source,None,|r|{
                    r["chain"].as_array_mut().unwrap().insert(at,json!({"kind":"pedal","uid":uid,"pedal":pedal,"on":true,"params":params}));Ok(())})?;
                self.catalog.touch(pedal);
                return Ok(json!({"added":uid,"chain":self.chain_names()}));
            }
            let i = self.index(&self.target(text(p, "target"))?)?;
            if self.chain()[i]["kind"] != "pedal" {
                return Err("Заменить можно только педаль".into());
            }
            self.session
                .edit(&format!("Замена на {name}"), source, None, |r| {
                    let b = &mut r["chain"][i];
                    b["pedal"] = json!(pedal);
                    b["params"] = json!(params);
                    b["on"] = json!(true);
                    Ok(())
                })?;
            self.catalog.touch(pedal);
            return Ok(json!({"replaced":p["target"],"with":pedal}));
        }
        let uid = self.target(text(p, "target"))?;
        if op == "chain.toggle" {
            let path = if uid == "gate" {
                "/input/gate".into()
            } else if CORE.contains(&uid.as_str()) {
                format!("/{uid}")
            } else {
                format!("/chain/{}", self.index(&uid)?)
            };
            let current = self
                .session
                .rig()
                .pointer(&path)
                .and_then(|b| b["on"].as_bool())
                .unwrap_or(true);
            let on = p["on"].as_bool().unwrap_or(!current);
            self.session.edit(
                &format!("{uid} {}", if on { "вкл" } else { "выкл" }),
                source,
                None,
                |r| {
                    r.pointer_mut(&path).ok_or("Блок не найден")?["on"] = json!(on);
                    Ok(())
                },
            )?;
            return Ok(json!({"target":p["target"],"on":on}));
        }
        let i = self.index(&uid)?;
        let b = &self.chain()[i];
        if op == "chain.remove" && b["kind"] != "pedal" {
            return Err("Удалять можно только педали".into());
        }
        let name = self
            .catalog
            .get(text(b, "pedal"))
            .map(|d| text(d, "name"))
            .unwrap_or(&uid)
            .to_string();
        match op {
            "chain.remove" => {
                self.session
                    .edit(&format!("Удалено: {name}"), source, None, |r| {
                        r["chain"].as_array_mut().unwrap().remove(i);
                        Ok(())
                    })?;
                Ok(json!({"removed":p["target"]}))
            }
            "chain.move" => {
                let to = self.insert_at(p)?;
                self.session.edit(
                    &format!("Перемещено: {name}"),
                    source,
                    None,
                    |r| {
                        let ch = r["chain"].as_array_mut().unwrap();
                        let b = ch.remove(i);
                        ch.insert(to - usize::from(to > i), b);
                        Ok(())
                    },
                )?;
                Ok(json!({"chain":self.chain_names()}))
            }
            _ => Err(format!("Неизвестная команда {op}")),
        }
    }
    pub fn audition(&mut self, p: &Value) -> Result<()> {
        let pedal = text(p, "pedal");
        let d = self.catalog.supported(pedal)?;
        let replace = p["replace"].as_str().map(|s| self.target(s)).transpose()?;
        self.audition = Some(
            json!({"pedalId":pedal,"active":true,"index":self.insert_at(p)?,"replaceUid":replace,"params":parameters(d,&p["params"]),"def":d["json"]}),
        );
        Ok(())
    }
    pub fn set_param(
        &mut self,
        target: &str,
        key: &str,
        value: &Value,
        source: &str,
        gesture: Option<&str>,
    ) -> Result<Value> {
        let uid = self.target(target)?;
        let r = self.session.rig();
        let mut result_target = uid.clone();
        let mut canonical = key.to_string();
        let (path, v) = match uid.as_str() {
            "amp" if key == "channel" => {
                if EQUIPMENT["channels"]
                    .get(value.as_str().unwrap_or(""))
                    .is_none()
                {
                    return Err("Неизвестный канал усилителя".into());
                }
                ("/amp/channel".into(), value.clone())
            }
            "amp" | "eq" | "delay" | "reverb" if !(uid == "delay" && key == "sync") => {
                let def = list(&EQUIPMENT["params"][&uid])
                    .iter()
                    .find(|p| p["id"] == key)
                    .ok_or_else(|| format!("Неизвестный параметр {uid}.{key}"))?;
                let path = if uid == "amp" {
                    format!("/amp/channels/{}/{key}", text(&r["amp"], "channel"))
                } else {
                    format!("/{uid}/{key}")
                };
                (path, normalized(def, value)?)
            }
            "delay" => {
                if !["off", "1/2", "1/4", "1/4.", "1/8", "1/8.", "1/8t", "1/16"]
                    .contains(&value.as_str().unwrap_or(""))
                {
                    return Err("Неизвестная длительность".into());
                }
                ("/delay/sync".into(), value.clone())
            }
            "cab" => {
                // new layout: a.* / b.* (close mics), room.*, solo; older single keys map onto mic a / b
                let (group, leaf) = match key {
                    "mic" | "position" | "distance" => ("a", key),
                    "mic2" => ("b", "mic"),
                    "position2" => ("b", "position"),
                    "distance2" => ("b", "distance"),
                    "invert2" => ("b", "phase"),
                    _ => key.split_once('.').unwrap_or(("", key)),
                };
                let bool_of = |v: &Value| v.as_bool().map(Value::from).ok_or_else(|| "Ожидается переключатель".to_string());
                let v = match (group, leaf) {
                    ("b", "mic") if value.is_null() || value == "" => Value::Null,
                    ("a" | "b" | "room", "mic") | ("", "model") => {
                        let bank = if leaf == "model" { "cabs" } else { "mics" };
                        if EQUIPMENT[bank].get(value.as_str().unwrap_or("")).is_none() { return Err(format!("Неизвестный {key}")); }
                        value.clone()
                    }
                    ("room", "room") => {
                        if EQUIPMENT["rooms"].get(value.as_str().unwrap_or("")).is_none() { return Err("Комнаты: booth, studio, live, garage, hall, church".into()); }
                        value.clone()
                    }
                    ("a" | "b", "position" | "distance" | "angle") | ("room", "distance" | "width") | ("", "blend" | "level") => normalized(&Value::Null, value)?,
                    ("a" | "b" | "room", "level") => json!(finite(value)?.clamp(-60., 12.)),
                    ("a" | "b", "pan") => json!(finite(value)?.clamp(-1., 1.)),
                    ("room", "lowcut") => json!(finite(value)?.clamp(20., 600.)),
                    ("a" | "b", "on" | "phase") | ("room", "on") => bool_of(value)?,
                    ("", "solo") => match value.as_str() { None | Some("") => Value::Null, Some(s @ ("a" | "b" | "room")) => json!(s), _ => return Err("solo: a, b, room или пусто".into()) },
                    ("", "irPath") if value.is_null() || value.is_string() => value.clone(),
                    _ => return Err(format!("Неизвестный параметр кабинета {key}. Есть: model, level, solo, irPath, a.mic/position/distance/angle/level/pan/phase/on, b.*, room.on/room/mic/distance/width/level/lowcut")),
                };
                canonical = if group.is_empty() { leaf.to_string() } else { format!("{group}.{leaf}") };
                let path = match group { "" => format!("/cab/{leaf}"), "room" => format!("/cab/room/{leaf}"), g => format!("/cab/mics/{g}/{leaf}") };
                (path, v)
            }
            "gate" | "input" | "output" => {
                let (path, min, max) = match (uid.as_str(), key) {
                    ("gate", "threshold") => ("/input/gate/threshold", -96., -10.),
                    ("gate", "release") => ("/input/gate/release", 5., 1000.),
                    ("input", "transpose") => ("/input/transpose", -12., 12.),
                    ("input", "gain") => ("/input/gain", -36., 24.),
                    ("output", "gain") => ("/output/gain", -36., 24.),
                    ("output", "doubler") => ("/output/doubler/spread", 3., 35.),
                    _ => return Err(format!("Неизвестный параметр {uid}.{key}")),
                };
                let mut v = finite(value)?.clamp(min, max);
                if key == "transpose" {
                    v = v.round()
                }
                (path.into(), json!(v))
            }
            _ => {
                let i = self.index(&uid)?;
                let d = self.catalog.supported(text(&self.chain()[i], "pedal"))?;
                let def = list(&d["params"])
                    .iter()
                    .find(|p| {
                        p["id"] == key || text(p, "name").to_lowercase() == key.to_lowercase()
                    })
                    .ok_or_else(|| format!("Нет параметра {key}"))?;
                canonical = text(def, "id").into();
                result_target = text(d, "name").into();
                (format!("/chain/{i}/params"), normalized(def, value)?)
            }
        };
        self.session.edit(
            &format!("{result_target} · {canonical}"),
            source,
            gesture,
            |r| {
                if uid == "cab" { crate::cab::migrate(&mut r["cab"]); }
                if path == "/cab/blend" {
                    // old two-mic balance: split it into the two close-mic levels
                    let b = v.as_f64().unwrap_or(0.5);
                    let db = |g: f64| if g <= 1e-6 { -60.0 } else { 20.0 * g.log10() };
                    r["cab"]["mics"]["a"]["level"] = json!(db(1.0 - b));
                    r["cab"]["mics"]["b"]["level"] = json!(db(b));
                    r["cab"]["mics"]["b"]["on"] = json!(b > 0.0);
                    return Ok(());
                }
                if path == "/cab/mics/b/mic" {
                    r["cab"]["mics"]["b"]["on"] = json!(!v.is_null());
                    if v.is_null() { return Ok(()); }
                }
                if path == "/cab/mics/a/mic" { r["cab"]["mic"] = v.clone(); }
                if path.ends_with("/params") {
                    r.pointer_mut(&path).ok_or("Нет блока")?[&canonical] = v.clone();
                } else {
                    let (parent, leaf) = path.rsplit_once('/').ok_or("Нет параметра")?;
                    r.pointer_mut(parent).ok_or("Нет блока")?[leaf] = v.clone();
                }
                if uid == "cab" && key == "irPath" {
                    let file = v.as_str().filter(|s| !s.is_empty());
                    r["cab"]["mode"] = json!(if file.is_some() { "ir" } else { "model" });
                    r["cab"]["irName"] =
                        json!(file.and_then(|s| Path::new(s.rsplit(['/', '\\']).next()?)
                            .file_stem()?
                            .to_str()));
                } else if uid == "cab" && key == "model" {
                    r["cab"]["mode"] = json!("model");
                }
                Ok(())
            },
        )?;
        let out = if uid == "amp" && key == "channel" {
            json!({"channel":v})
        } else if key == "gain" && (uid == "input" || uid == "output") {
            json!({"target":uid,"gain_db":v})
        } else if uid == "amp" {
            json!({"target":"amp","channel":self.session.rig()["amp"]["channel"],"param":canonical,"value":v})
        } else {
            json!({"target":result_target,"param":canonical,"value":v})
        };
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn moves_core_blocks_by_gap_and_preserves_order_after_reload() {
        let mut r = Rig::default();
        let before = r.chain().to_vec();
        r.chain_action(
            "chain.move",
            &json!({"target":"cab","position":"end"}),
            "ui",
        )
        .unwrap();
        assert_eq!(r.chain().last().unwrap()["uid"], "cab");
        r.chain_action(
            "chain.move",
            &json!({"target":"reverb","before":"amp"}),
            "ui",
        )
        .unwrap();
        assert_eq!(r.chain()[0]["uid"], "reverb");
        let moved = r.session.rig().clone();
        r.session.load(&moved, None, "Мои").unwrap();
        assert_eq!(r.chain(), list(&moved["chain"]));
        r.session.undo();
        r.session.undo();
        r.session.undo();
        assert_eq!(r.chain(), before);
        assert!(r
            .chain_action("chain.remove", &json!({"target":"cab"}), "ui")
            .is_err());
    }

    #[test]
    fn actions_apply_to_sound_and_preserve_undo() {
        let mut r = Rig::default();
        let uid = r
            .chain_action("chain.add", &json!({"pedal":"gc.clarinot-clean"}), "mcp")
            .unwrap()["added"]
            .as_str()
            .unwrap()
            .to_owned();
        r.set_param(&uid, "delay", &json!(555), "mcp", None)
            .unwrap();
        let audio = r.audio_rig(120.);
        assert_eq!(audio["chain"][0]["params"]["delay"], 400.0);
        assert_eq!(audio["chain"][0]["params"]["blend"], 0.5);
        r.chain_action("chain.move", &json!({"target":uid,"position":"end"}), "mcp")
            .unwrap();
        r.session.undo();
        assert_eq!(r.chain()[0]["uid"], uid);
        r.chain_action(
            "chain.replace",
            &json!({"target":uid,"pedal":"gc.silencer"}),
            "mcp",
        )
        .unwrap();
        assert_eq!(r.chain()[0]["pedal"], "gc.silencer");
        assert_eq!(r.chain()[0]["uid"], uid);
        r.session.undo();
        let saved = r.session.rig().clone();
        r.audition(&json!({"pedal":"gc.molten","replace":uid}))
            .unwrap();
        assert_eq!(r.audio_rig(90.)["audition"]["replaceUid"], uid);
        assert_eq!(r.session.rig(), &saved);
        assert!(r
            .chain_action("chain.remove", &json!({"target":"amp"}), "mcp")
            .is_err());
    }
}
