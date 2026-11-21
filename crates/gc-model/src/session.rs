//! A document and its undo history. The UI and remote controls edit the same session.
use crate::{default_rig, normalize};
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Arc;
use web_time::{Duration, Instant};

#[derive(Clone)]
struct Origin {
    file: Option<String>,
    bank: String,
    saved: Value,
}

pub struct Session {
    rig: Value,
    origin: Arc<Origin>,
    past: VecDeque<Entry>,
    future: Vec<Entry>,
    gesture: Option<(String, String, Instant)>,
}

struct Entry {
    label: String,
    source: String,
    rig: Value,
    origin: Arc<Origin>,
}

impl Default for Session {
    fn default() -> Self {
        let rig = default_rig("Default");
        Self {
            origin: Arc::new(Origin {
                file: None,
                bank: "Мои".into(),
                saved: rig.clone(),
            }),
            rig,
            past: VecDeque::new(),
            future: Vec::new(),
            gesture: None,
        }
    }
}

impl Session {
    pub fn rig(&self) -> &Value {
        &self.rig
    }
    pub fn saved(&self) -> &Value {
        &self.origin.saved
    }
    pub fn file(&self) -> Option<&str> {
        self.origin.file.as_deref()
    }
    pub fn bank(&self) -> &str {
        &self.origin.bank
    }
    pub fn dirty(&self) -> bool {
        let (a, b) = (
            self.rig.as_object().unwrap(),
            self.origin.saved.as_object().unwrap(),
        );
        a.iter()
            .any(|(k, v)| k != "favorite" && b.get(k) != Some(v))
            || b.keys().any(|k| k != "favorite" && !a.contains_key(k))
    }
    pub fn undo_labels(&self) -> impl DoubleEndedIterator<Item = (&str, &str)> {
        self.past
            .iter()
            .map(|e| (e.label.as_str(), e.source.as_str()))
    }
    pub fn redo_labels(&self) -> impl DoubleEndedIterator<Item = &str> {
        self.future.iter().map(|e| e.label.as_str())
    }

    fn record(&mut self, label: &str, source: &str, next: Value) {
        self.past.push_back(Entry {
            label: label.to_owned(),
            source: source.to_owned(),
            rig: std::mem::replace(&mut self.rig, next),
            origin: self.origin.clone(),
        });
        if self.past.len() > 200 {
            self.past.pop_front();
        }
        self.future.clear();
    }

    /// A failed edit leaves both the document and history untouched. Continuous edits
    /// from one physical knob are coalesced without absorbing changes from another client.
    pub fn edit<T>(
        &mut self,
        label: &str,
        source: &str,
        key: Option<&str>,
        change: impl FnOnce(&mut Value) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut next = self.rig.clone();
        let result = change(&mut next)?;
        if next == self.rig {
            return Ok(result);
        }
        let now = Instant::now();
        let merge = self.gesture.as_ref().is_some_and(|(k, s, t)| {
            Some(k.as_str()) == key
                && s == source
                && (k.starts_with("drag/") || now.duration_since(*t) < Duration::from_millis(900))
        });
        if merge {
            self.rig = next;
        } else {
            self.record(label, source, next);
        }
        self.gesture = key.map(|k| (k.to_owned(), source.to_owned(), now));
        Ok(result)
    }
    pub fn load(&mut self, preset: &Value, file: Option<String>, bank: &str) -> Result<(), String> {
        let rig = normalize(preset)?;
        let origin = Arc::new(Origin {
            file,
            bank: bank.into(),
            saved: rig.clone(),
        });
        self.record(
            &format!("Загрузка «{}»", rig["name"].as_str().unwrap_or_default()),
            "ui",
            rig,
        );
        self.origin = origin;
        self.gesture = None;
        Ok(())
    }
    pub fn clear_history(&mut self) {
        self.past.clear();
        self.future.clear();
        self.gesture = None;
    }
    pub fn mark_saved(&mut self, file: String, bank: &str) {
        self.mark_snapshot(file, bank, self.rig.clone());
    }
    pub fn mark_snapshot(&mut self, file: String, bank: &str, saved: Value) {
        let replacement = Arc::new(Origin {
            file: Some(file),
            bank: bank.into(),
            saved,
        });
        for entry in self.past.iter_mut().chain(self.future.iter_mut()) {
            if Arc::ptr_eq(&entry.origin, &self.origin) {
                entry.origin = replacement.clone();
            }
        }
        self.origin = replacement;
        self.gesture = None;
    }
    fn swap(&mut self, mut entry: Entry) -> Entry {
        std::mem::swap(&mut self.rig, &mut entry.rig);
        std::mem::swap(&mut self.origin, &mut entry.origin);
        self.gesture = None;
        entry
    }
    pub fn undo(&mut self) -> Option<String> {
        let entry = self.past.pop_back()?;
        let label = entry.label.clone();
        let redo = self.swap(entry);
        self.future.push(redo);
        Some(label)
    }
    pub fn redo(&mut self) -> Option<String> {
        let entry = self.future.pop()?;
        let label = entry.label.clone();
        let undo = self.swap(entry);
        self.past.push_back(undo);
        Some(label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn gesture_and_history_survive_save_and_switch_documents() {
        let mut s = Session::default();
        for x in [0.2, 0.3] {
            s.edit("Gain", "ui", Some("gain"), |r| {
                r["input"]["gain"] = json!(x);
                Ok(())
            })
            .unwrap();
        }
        s.mark_saved("old.gcpreset".into(), "Банк");
        assert!(!s.dirty());
        assert_eq!(s.undo_labels().count(), 1);
        s.undo();
        assert!(s.dirty());
        s.redo();
        assert!(!s.dirty());
        s.load(&json!({"name":"B"}), Some("other.gcpreset".into()), "Мои")
            .unwrap();
        s.undo();
        assert_eq!(s.file(), Some("old.gcpreset"));
        assert!(!s.dirty());
        let status = s.edit("fail", "rpc", None, |r| {
            r["input"] = Value::Null;
            Err::<(), _>("bad edit".into())
        });
        assert!(status.is_err());
        assert!(!s.dirty());
        assert!(s.redo().is_some());
    }
}
