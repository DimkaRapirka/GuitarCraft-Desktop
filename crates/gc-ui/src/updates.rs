//! Update offer, progress and the manual "check for updates" used by Settings.
use crate::{dom::*, host::command, i18n, presets};
use std::{cell::Cell, rc::Rc};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::Element;

const STEPS: [(&str, &str); 3] = [
    ("download", "Загрузка"),
    ("install", "Установка"),
    ("restart", "Перезапуск"),
];

pub async fn lookup() -> Result<Option<String>, JsValue> {
    Ok(command("check_update", &JsValue::UNDEFINED)
        .await?
        .as_string())
}

/// Startup check: stays silent unless there is something to install.
pub async fn check() -> Result<(), JsValue> {
    if let Some(version) = lookup().await? {
        offer(&version)?;
    }
    Ok(())
}

/// Settings → About: always answers, including when the check itself fails.
pub async fn check_now(current: &str) -> String {
    match lookup().await {
        Ok(Some(version)) => {
            let _ = offer(&version);
            i18n::message("Доступен GuitarCraft {}", &[&version])
        }
        Ok(None) => i18n::message("Установлена последняя версия ({})", &[current]),
        Err(err) => i18n::message("Не удалось проверить обновления: {}", &[&reason(&err)]),
    }
}

fn reason(err: &JsValue) -> String {
    err.as_string()
        .or_else(|| get(err, "message").as_string())
        .unwrap_or_else(|| "?".into())
}

struct Notice {
    root: Element,
    title: Element,
    detail: Element,
    fill: Element,
    steps: Vec<(String, Element)>,
    install: Element,
    later: Element,
}

impl Notice {
    fn stage(&self, active: &str) {
        let at = STEPS.iter().position(|s| s.0 == active).unwrap_or(0);
        for (n, (_, li)) in self.steps.iter().enumerate() {
            let class = if n < at { "done" } else if n == at { "now" } else { "" };
            li.set_class_name(class);
        }
    }
    fn bar(&self, fraction: Option<f64>) {
        match fraction {
            Some(f) => {
                let _ = self.root.class_list().remove_1("indeterminate");
                style(&self.fill, "width", &format!("{:.1}%", (f * 100.).clamp(0., 100.)));
            }
            None => {
                let _ = self.root.class_list().add_1("indeterminate");
                style(&self.fill, "width", "");
            }
        }
    }
    fn mode(&self, mode: &str) {
        self.root.set_class_name(&format!("update-notice is-{mode}"));
    }
}

fn megabytes(bytes: f64) -> String {
    format!("{:.1}", bytes / 1_048_576.)
}

fn offer(version: &str) -> Result<(), JsValue> {
    if let Some(old) = doc().get_element_by_id("update-notice") {
        old.remove();
    }
    let root = child(&doc().body().unwrap(), "aside", "update-notice is-offer", "");
    root.set_id("update-notice");
    root.set_attribute("aria-live", "polite")?;
    let text = child(&root, "div", "upd-text", "");
    let title = child(&text, "strong", "upd-title", "");
    title.set_text_content(Some(&i18n::message("Доступен GuitarCraft {}", &[version])));
    let detail = child(&text, "span", "upd-detail", "");
    let track = child(&root, "div", "upd-bar", "");
    let fill = child(&track, "i", "", "");
    let list = child(&root, "ol", "upd-steps", "");
    let steps = STEPS
        .iter()
        .map(|(id, name)| (id.to_string(), child(&list, "li", "", name)))
        .collect();
    let actions = child(&root, "div", "upd-actions", "");
    let install = child(&actions, "button", "primary", "Обновить и перезапустить");
    let later = child(&actions, "button", "", "Позже");
    let notice = Rc::new(Notice { root: root.clone(), title, detail, fill, steps, install, later });
    let closing = root.clone();
    listen(&notice.later, "click", move |_| closing.remove());
    let n = notice.clone();
    listen(&notice.install, "click", move |_| {
        if get(&n.install, "disabled").is_truthy() {
            return;
        }
        let n = n.clone();
        spawn_local(run(n));
    });
    Ok(())
}

async fn run(n: Rc<Notice>) {
    set(&n.install, "disabled", &true.into());
    set(&n.later, "disabled", &true.into());
    let outcome = async {
        if !presets::discard().await? {
            return Ok::<bool, JsValue>(false);
        }
        n.mode("busy");
        n.stage("download");
        n.bar(None);
        n.title.set_text_content(Some(i18n::text("Загрузка обновления")));
        n.detail.set_text_content(Some(i18n::text("Подготовка…")));
        let unlisten = subscribe(n.clone()).await;
        let result = command("install_update", &JsValue::UNDEFINED).await;
        if let Some(f) = unlisten {
            let _ = f.call0(&JsValue::UNDEFINED);
        }
        result.map(|_| true)
    }
    .await;
    match outcome {
        Ok(_) => {}
        Err(err) => {
            web_sys::console::warn_1(&err);
            n.mode("error");
            n.title.set_text_content(Some(i18n::text(
                "Не удалось обновить GuitarCraft. Попробуйте ещё раз.",
            )));
            n.detail.set_text_content(Some(&reason(&err)));
            n.install.set_text_content(Some(i18n::text("Повторить")));
        }
    }
    set(&n.install, "disabled", &false.into());
    set(&n.later, "disabled", &false.into());
}

async fn subscribe(n: Rc<Notice>) -> Option<js_sys::Function> {
    let events = get(&get(&web_sys::window()?.into(), "__TAURI__"), "event");
    let started = Cell::new(crate::host::now());
    let callback = Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
        let payload = get(&event, "payload");
        let stage = string(&payload, "stage");
        match stage.as_str() {
            "download" => {
                n.stage("download");
                let done = num(&payload, "done", 0.);
                let total = get(&payload, "total").as_f64().filter(|t| *t > 0.);
                n.title.set_text_content(Some(i18n::text("Загрузка обновления")));
                let seconds = ((crate::host::now() - started.get()) / 1000.).max(0.001);
                let speed = format!("{} {}", megabytes(done / seconds), i18n::text("МБ/с"));
                n.bar(total.map(|t| done / t));
                let line = match total {
                    Some(t) => i18n::message(
                        "{} из {} МБ · {}%",
                        &[&megabytes(done), &megabytes(t), &format!("{:.0}", done / t * 100.)],
                    ),
                    None => format!("{} {}", megabytes(done), i18n::text("МБ")),
                };
                n.detail.set_text_content(Some(&format!("{line} · {speed}")));
            }
            "install" => {
                n.stage("install");
                n.bar(None);
                n.title.set_text_content(Some(i18n::text("Установка обновления…")));
                n.detail.set_text_content(Some(i18n::text(
                    "Установщик запущен. GuitarCraft закроется и откроется снова сам.",
                )));
            }
            "restart" => {
                n.stage("restart");
                n.bar(None);
                n.title.set_text_content(Some(i18n::text("Перезапуск…")));
                n.detail.set_text_content(None);
            }
            _ => {}
        }
    })
    .into_js_value();
    let promise = invoke(&events, "listen", &["update:progress".into(), callback]).ok()?;
    crate::host::resolved(promise).await.ok()?.dyn_into().ok()
}
