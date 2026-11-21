use crate::{EQUIPMENT, PEDALS};
use serde_json::{json, Value};
use std::sync::LazyLock;

pub static SEARCH: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../data/search.json")).expect("search metadata")
});

pub fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or_default()
}
pub fn list(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or_default()
}
fn norm(s: &str) -> String {
    s.to_lowercase().replace('ё', "е")
}

pub fn compile_pedal(raw: &Value) -> Result<Value, String> {
    if raw["format"] != "guitarcraft-pedal" {
        return Err("Неверный формат педали".into());
    }
    let id = raw["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("Нет идентификатора педали")?;
    let name = raw["name"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or("Нет названия педали")?;
    let id = if id.starts_with("user.") {
        id.to_owned()
    } else {
        format!("user.{id}")
    };
    let mut params = vec![];
    for knob in list(&raw["knobs"]) {
        let key = knob["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or("Нет идентификатора ручки")?;
        params.push(json!({"id":key,"name":knob["name"].as_str().unwrap_or(key),"min":0,"max":1,"def":knob["def"].as_f64().unwrap_or(0.5)}));
    }
    Ok(
        json!({"id":id,"name":name,"type":raw["type"].as_str().unwrap_or("Custom"),"category":raw["category"].as_str().unwrap_or("utility"),
        "color":raw["color"].as_str().unwrap_or("#6b7280"),"character":list(&raw["character"]),"desc":raw["desc"].as_str().unwrap_or("Педаль из конструктора DSP-блоков"),
        "source":"user","supported":true,"params":params,"blocks":raw["blocks"],"json":raw}),
    )
}

#[derive(Default)]
pub struct Preferences {
    pub favorites: Vec<String>,
    pub recent: Vec<String>,
}

pub struct Catalog {
    pub items: Vec<Value>,
    pub prefs: Preferences,
}
impl Default for Catalog {
    fn default() -> Self {
        let items = PEDALS
            .iter()
            .cloned()
            .map(|mut p| {
                p["source"] = json!("builtin");
                p["supported"] = json!(true);
                p
            })
            .collect();
        Self {
            items,
            prefs: Preferences::default(),
        }
    }
}
impl Catalog {
    pub fn get(&self, id: &str) -> Option<&Value> {
        self.items.iter().find(|d| d["id"] == id)
    }
    pub fn supported(&self, id: &str) -> Result<&Value, String> {
        let d = self
            .get(id)
            .ok_or_else(|| format!("Педаль «{id}» не найдена"))?;
        if d["supported"] == true {
            Ok(d)
        } else {
            Err(format!(
                "Формат «{}» доступен только для поиска",
                text(d, "type")
            ))
        }
    }
    pub fn add(&mut self, def: Value) {
        if let Some(old) = self.items.iter_mut().find(|d| d["id"] == def["id"]) {
            *old = def;
        } else {
            self.items.push(def);
        }
    }
    pub fn favorite(&mut self, id: &str) -> bool {
        if let Some(i) = self.prefs.favorites.iter().position(|s| s == id) {
            self.prefs.favorites.remove(i);
            false
        } else {
            self.prefs.favorites.push(id.into());
            true
        }
    }
    pub fn touch(&mut self, id: &str) {
        self.prefs.recent.retain(|s| s != id);
        self.prefs.recent.insert(0, id.into());
        self.prefs.recent.truncate(12);
    }
    pub fn search(&self, query: &str, filters: &Value) -> Vec<&Value> {
        self.search_localized(query, filters, |label| label)
    }
    pub fn search_localized(
        &self,
        query: &str,
        filters: &Value,
        translate: impl Fn(&str) -> &str,
    ) -> Vec<&Value> {
        let query = norm(query);
        let tokens: Vec<_> = query
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .map(|word| {
                let count = word.chars().count();
                let stem: String = if count > 5 {
                    word.chars().take(count - 2).collect()
                } else {
                    word.into()
                };
                (word, stem)
            })
            .collect();
        let mut hits = vec![];
        for d in &self.items {
            let id = text(d, "id");
            let fav = self.prefs.favorites.iter().any(|s| s == id);
            if ["category", "source"].iter().any(|k| {
                filters[*k]
                    .as_str()
                    .is_some_and(|s| !s.is_empty() && d[*k] != s)
            }) || filters["favorites"] == true && !fav
            {
                continue;
            }
            let recent = self.prefs.recent.iter().position(|x| x == id);
            if filters["recent"] == true && recent.is_none() {
                continue;
            }
            let name = norm(text(d, "name"));
            let typ = norm(text(d, "type"));
            let syn = norm(text(&SEARCH["synonyms"], text(d, "type")));
            let category = list(&EQUIPMENT["categories"])
                .iter()
                .find(|c| c["id"] == d["category"]);
            let desc = norm(&format!(
                "{} {} {} {}",
                text(d, "desc"),
                text(d, "format"),
                category.map(|c| text(c, "name")).unwrap_or_default(),
                category.map(|c| text(c, "en")).unwrap_or_default()
            ));
            let chars = list(&d["character"])
                .iter()
                .filter_map(Value::as_str)
                .flat_map(|tag| [norm(tag), norm(translate(tag))])
                .collect::<Vec<_>>();
            let mut score = 0;
            for (token, stem) in &tokens {
                let points = if name.contains(token) {
                    10
                } else if typ.contains(token) || syn.contains(stem) {
                    7
                } else if chars.iter().any(|c| c.contains(stem)) {
                    5
                } else if desc.contains(stem) || name.contains(stem) || typ.contains(stem) {
                    2
                } else {
                    0
                };
                if points == 0 {
                    score = 0;
                    break;
                }
                score += points;
            }
            if tokens.is_empty() {
                score = 1;
            } else if score > 0 {
                score += i32::from(fav);
                if d["supported"] != true {
                    score -= 3;
                }
            }
            if score > 0 {
                hits.push((score, d, recent));
            }
        }
        let n = tokens.len() as i32;
        if n > 0 && hits.iter().any(|(s, _, _)| *s >= 7 * n) {
            hits.retain(|(s, _, _)| *s >= 5 * n);
        }
        hits.sort_by(|(a, x, rx), (b, y, ry)| {
            b.cmp(a)
                .then_with(|| (y["supported"] == true).cmp(&(x["supported"] == true)))
                .then_with(|| {
                    if filters["recent"] == true {
                        rx.cmp(ry)
                    } else {
                        norm(text(x, "name")).cmp(&norm(text(y, "name")))
                    }
                })
        });
        hits.into_iter()
            .take(filters["limit"].as_u64().unwrap_or(60) as usize)
            .map(|(_, d, _)| d)
            .collect()
    }
    pub fn summary(&self, d: &Value) -> Value {
        let params=list(&d["params"]).iter().map(|p|json!({"id":p["id"],"name":p["name"],"min":p["min"].as_f64().unwrap_or(0.),"max":p["max"].as_f64().unwrap_or(1.),"default":p["def"],"unit":text(p,"unit")})).collect::<Vec<_>>();
        json!({"id":d["id"],"name":d["name"],"type":d["type"],"source":d["source"],"supported":d["supported"],"category":d["category"],"character":d["character"],"description":text(d,"desc"),"favorite":self.prefs.favorites.iter().any(|s|d["id"]==s.as_str()),"params":params})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn descriptors_filters_and_builder() {
        let mut c = Catalog::default();
        assert_eq!(
            c.search("теплый овердрайв", &Value::Null)[0]["id"],
            "gc.verdant"
        );
        assert_eq!(
            c.search("clarinot", &Value::Null)[0]["artwork"],
            "assets/gear/prints/cassette.png"
        );
        c.favorite("gc.clarinot-clean");
        assert_eq!(c.search("", &json!({"favorites":true})).len(), 1);
        let raw = json!({"format":"guitarcraft-pedal","id":"test","name":"Test","blocks":[],"knobs":[{"id":"a","def":0}]});
        let def = compile_pedal(&raw).unwrap();
        assert_eq!(def["params"][0]["def"], 0.0);
        assert_eq!(def["json"], raw);
    }
}
