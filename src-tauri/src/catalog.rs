//! Discover equipment on disk; only the UI owns the live, searchable catalog.
use crate::files::{scan_plugins, walk, Dirs};
use gc_model::catalog::{compile_pedal, list, text};
use serde_json::{json, Value};

pub fn discover(dirs: &Dirs, source: &str) -> Result<Vec<Value>, String> {
    match source{
        "user"=> {
            let mut items=vec![];
            for path in walk(&dirs.pedals,&[".gcpedal.json"]){
                let pedal=(||->Result<Value,String>{
                    let bytes=std::fs::read(&path).map_err(|e|e.to_string())?;
                    let raw=serde_json::from_slice(&bytes).map_err(|e|e.to_string())?;
                    let mut d=compile_pedal(&raw)?;d["file"]=json!(path);Ok(d)
                })();
                match pedal{Ok(d)=>items.push(d),Err(e)=>eprintln!("GuitarCraft: {}: {e}",path.display())}
            }Ok(items)
        }
        "extension"=>Ok(walk(&dirs.extensions,&[".gcext.js"]).iter().filter_map(|path|{
            let file=path.file_name()?.to_string_lossy();
            Some(json!({"id":format!("ext.{file}"),"name":file.strip_suffix(".gcext.js").unwrap_or(&file),"type":"JS-расширение","category":"utility","source":"extension","supported":false,"params":[],"character":["расширение"],"desc":"JS-расширения работали в старом Web Audio движке. Rust-ядро их не исполняет: соберите педаль в конструкторе (.gcpedal.json). Ring Planet уже стал встроенной педалью."}))
        }).collect()),
        "plugin"=>Ok(list(&scan_plugins(dirs)).iter().filter(|p|p["format"]!="GCEXT").map(|p|{
            let format=text(p,"format");let name=text(p,"name");let path=text(p,"path");
            json!({"id":format!("plugin.{format}.{name}"),"name":name,"type":format!("{format} плагин"),"category":"utility","format":format,"source":"plugin","supported":false,"path":path,
                "character":["сторонний","plugin",format.to_lowercase()],"desc":format!("Найден в {path}. Формат {format} пока не обрабатывает звук в GuitarCraft: нужен нативный хост."),"params":[]})
        }).collect()),
        _=>Err(format!("Неизвестный источник каталога: {source}"))
    }
}
