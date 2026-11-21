//! Presets, settings, user folders, plugin scan — same formats and locations as before.
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub struct Dirs { pub data: PathBuf, pub presets: PathBuf, pub irs: PathBuf, pub skins: PathBuf, pub backgrounds: PathBuf, pub pedals: PathBuf, pub extensions: PathBuf, pub recordings: PathBuf }
impl Dirs {
    pub fn new() -> Self {
        let docs = std::env::var_os("GC_DOCUMENTS_DIR").map(PathBuf::from).unwrap_or_else(|| dirs::document_dir().unwrap_or_default().join("GuitarCraft"));
        let data = std::env::var_os("GC_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| dirs::data_dir().unwrap_or_default().join("GuitarCraft"));
        let d = Dirs { presets: docs.join("Presets"), irs: docs.join("IR"), skins: docs.join("Skins"), backgrounds: docs.join("Backgrounds"), pedals: docs.join("Pedals"), extensions: docs.join("Extensions"),
            recordings: std::env::var("GC_RECORDINGS_DIR").map(PathBuf::from).unwrap_or_else(|_| docs.join("Recordings")), data };
        for p in [&d.data, &d.presets, &d.irs, &d.skins, &d.backgrounds, &d.pedals, &d.extensions, &d.recordings] { let _ = fs::create_dir_all(p); }
        d
    }
    pub fn kind(&self, k: &str) -> Option<&PathBuf> { match k { "presets" => Some(&self.presets), "irs" => Some(&self.irs), "skins" => Some(&self.skins), "backgrounds" => Some(&self.backgrounds), "pedals" => Some(&self.pedals), "extensions" => Some(&self.extensions), "recordings" => Some(&self.recordings), "data" => Some(&self.data), _ => None } }
    pub fn json(&self) -> Value { json!({ "data": self.data, "presets": self.presets, "irs": self.irs, "skins": self.skins, "backgrounds": self.backgrounds, "pedals": self.pedals, "extensions": self.extensions, "recordings": self.recordings }) }
    pub fn settings_file(&self) -> PathBuf { self.data.join("settings.json") }
    pub fn control_file(&self) -> PathBuf { self.data.join("control.json") }
}

pub fn safe(s: &str) -> String {
    let t: String = s.chars().map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c }).collect::<String>().trim().chars().take(120).collect();
    if t.is_empty() { "Untitled".into() } else { t }
}
pub fn walk(dir: &Path, ext: &[&str]) -> Vec<PathBuf> {
    let mut out = vec![];
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() { out.extend(walk(&p, ext)); }
            else if ext.is_empty() || ext.iter().any(|x| p.to_string_lossy().to_lowercase().ends_with(x)) { out.push(p); }
        }
    }
    out
}
pub fn list_presets(d: &Dirs) -> Value {
    let mut v = vec![];
    for f in walk(&d.presets, &[".gcpreset"]) {
        let Ok(text) = fs::read_to_string(&f) else { continue }; let Ok(j) = serde_json::from_str::<Value>(&text) else { continue };
        let parent = f.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let bank = if parent == d.presets { "Мои".to_string() } else { parent.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default() };
        let mtime = fs::metadata(&f).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0);
        v.push(json!({ "file": f, "name": j.get("name"), "bank": bank, "favorite": j.get("favorite").and_then(|x| x.as_bool()).unwrap_or(false), "mtime": mtime, "data": j }));
    }
    Value::Array(v)
}
pub fn save_preset(d: &Dirs, preset: &Value, bank: Option<&str>) -> Result<Value, String> {
    let dir = match bank { Some(b) if b != "Мои" => d.presets.join(safe(b)), _ => d.presets.clone() };
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = preset.get("name").and_then(|x| x.as_str()).unwrap_or("Untitled");
    let file = dir.join(format!("{}.gcpreset", safe(name)));
    fs::write(&file, serde_json::to_string_pretty(preset).unwrap()).map_err(|e| e.to_string())?;
    Ok(json!({ "file": file }))
}
pub fn delete_preset(d: &Dirs, file: &str) -> Result<bool, String> {
    let p = PathBuf::from(file);
    if !p.starts_with(&d.presets) { return Err("outside presets folder".into()); }
    fs::remove_file(p).map_err(|e| e.to_string())?; Ok(true)
}
pub fn load_settings(d: &Dirs) -> Value { fs::read_to_string(d.settings_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({})) }
pub fn save_settings(d: &Dirs, mut v: Value) -> Result<bool, String> {
    if let Some(o) = v.as_object_mut() { o.remove("_bufferAtLaunch"); }
    fs::write(d.settings_file(), serde_json::to_string_pretty(&v).unwrap()).map_err(|e| e.to_string())?; Ok(true)
}
pub fn import_to(d: &Dirs, file: &str, kind: &str) -> Result<String, String> {
    let dir = d.kind(kind).ok_or("bad kind")?;
    let src = PathBuf::from(file); let dest = dir.join(src.file_name().ok_or("bad file")?);
    if src != dest { fs::copy(&src, &dest).map_err(|e| e.to_string())?; }
    Ok(dest.to_string_lossy().to_string())
}
pub fn scan_plugins(d: &Dirs) -> Value {
    let mut roots: Vec<(&str, PathBuf, &str)> = Vec::new();
    #[cfg(target_os = "macos")]
    for (fmt, dir, ext) in [("VST3", "VST3", ".vst3"), ("AU", "Components", ".component"), ("VST2", "VST", ".vst"), ("CLAP", "CLAP", ".clap")] {
        let base = Path::new("Library/Audio/Plug-Ins").join(dir);
        roots.push((fmt, Path::new("/").join(&base), ext));
        if let Some(home) = dirs::home_dir() { roots.push((fmt, home.join(&base), ext)); }
    }
    #[cfg(windows)]
    for base in [std::env::var_os("CommonProgramFiles").map(PathBuf::from), dirs::data_local_dir().map(|p| p.join("Programs/Common"))].into_iter().flatten() {
        roots.push(("VST3", base.join("VST3"), ".vst3"));
        roots.push(("CLAP", base.join("CLAP"), ".clap"));
    }
    let mut out = vec![];
    for (fmt, dir, ext) in roots.drain(..) {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.to_lowercase().ends_with(ext) {
                    out.push(json!({ "format": fmt, "name": &name[..name.len() - ext.len()], "path": entry.path() }));
                }
            }
        }
    }
    for f in walk(&d.extensions, &[".gcext.js"]) { out.push(json!({ "format": "GCEXT", "name": f.file_name().map(|x| x.to_string_lossy().replace(".gcext.js", "")), "path": f })); }
    Value::Array(out)
}
