//! Scene-specific background settings and the user's shared image library.
use crate::{
    dom::{get, invoke, style},
    host::command,
    js,
    preferences::save_settings,
    read,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::Element;

pub const SCENES: [(&str, &str); 3] = [
    ("studio", "Усилитель и кабинет"),
    ("board", "Педалборд"),
    ("rack", "Рэки"),
];

#[derive(Deserialize, Serialize)]
pub struct Background {
    pub id: String,
    pub path: String,
}

impl Background {
    pub fn label(&self) -> &str {
        self.id.strip_prefix("Фон · ").unwrap_or(&self.id)
    }
}

pub async fn list() -> Result<Vec<Background>, JsValue> {
    read(command("backgrounds_list", &JsValue::UNDEFINED).await?)
}

pub fn selected(settings: &Value, scene: &str) -> Option<String> {
    settings["sceneBackgrounds"][scene]
        .as_str()
        .map(str::to_owned)
}

fn check_scene(scene: &str) -> Result<(), JsValue> {
    if SCENES.iter().any(|(id, _)| *id == scene) {
        Ok(())
    } else {
        Err(crate::err("Неизвестная сцена".into()))
    }
}

pub async fn apply(app: &JsValue) -> Result<(), JsValue> {
    let images = list().await?;
    let settings: Value = read(get(app, "settings"))?;
    let root: Element = get(app, "root").dyn_into()?;
    let api = get(&get(&web_sys::window().unwrap(), "__TAURI__"), "core");
    for (scene, _) in SCENES {
        let property = format!("--gc-background-{scene}");
        if let Some(image) =
            selected(&settings, scene).and_then(|id| images.iter().find(|item| item.id == id))
        {
            let url = invoke(&api, "convertFileSrc", &[image.path.clone().into()])?
                .as_string()
                .unwrap_or_default();
            let url = url.replace('\\', "%5C").replace('"', "%22");
            style(&root, &property, &format!("url(\"{url}\")"));
        } else {
            style(&root, &property, "");
        }
    }
    Ok(())
}

async fn save(app: &JsValue, scene: &str, id: Option<&str>) -> Result<(), JsValue> {
    let settings: Value = read(get(app, "settings"))?;
    let mut choices = settings["sceneBackgrounds"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    if let Some(id) = id {
        choices.insert(scene.to_owned(), json!(id));
    } else {
        choices.remove(scene);
    }
    save_settings(app.clone(), js(&json!({"sceneBackgrounds": choices}))?).await?;
    Ok(())
}

pub async fn select(app: &JsValue, scene: &str, id: Option<&str>) -> Result<(), JsValue> {
    check_scene(scene)?;
    let id = id.filter(|s| !s.is_empty());
    if let Some(id) = id {
        if !list().await?.iter().any(|item| item.id == id) {
            return Err(crate::err("Фон отсутствует в библиотеке".into()));
        }
    }
    save(app, scene, id).await
}

pub async fn import(app: &JsValue, scene: &str, file: &str) -> Result<Background, JsValue> {
    check_scene(scene)?;
    let image: Background = read(command("background_import", &js(&json!({"file":file}))?).await?)?;
    save(app, scene, Some(&image.id)).await?;
    Ok(image)
}

pub async fn pick(app: &JsValue, scene: &str) -> Result<(), JsValue> {
    let file = command(
        "file_open",
        &js(&json!({"title":"Изображение для фона","filters":[{"name":"PNG, JPEG, WebP","extensions":["png","jpg","jpeg","webp"]}]}))?,
    )
    .await?;
    if let Some(file) = file.as_string() {
        let image = import(app, scene, &file).await?;
        invoke(
            app,
            "toast",
            &[crate::i18n::message("Фон выбран · {}", &[&image.label()]).into()],
        )?;
    }
    Ok(())
}
