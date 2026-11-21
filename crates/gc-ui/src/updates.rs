use crate::{dom::*, host::command, i18n, presets};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

pub async fn check() -> Result<(), JsValue> {
    let Some(version) = command("check_update", &JsValue::UNDEFINED)
        .await?
        .as_string()
    else {
        return Ok(());
    };
    let notice = child(&doc().body().unwrap(), "aside", "update-notice", "");
    notice.set_id("update-notice");
    notice.set_attribute("aria-live", "polite")?;
    let caption = i18n::message("Доступен GuitarCraft {}", &[&version]);
    let status = child(&notice, "strong", "", &caption);
    let install = child(&notice, "button", "primary", "Обновить и перезапустить");
    let later = child(&notice, "button", "", "Позже");
    let container = notice.clone();
    listen(&later, "click", move |_| container.remove());
    let button = install.clone();
    listen(&install, "click", move |_| {
        if get(&button, "disabled").is_truthy() {
            return;
        }
        let (button, later, status) = (button.clone(), later.clone(), status.clone());
        set(&button, "disabled", &true.into());
        set(&later, "disabled", &true.into());
        spawn_local(async move {
            let result = async {
                if presets::discard().await? {
                    status.set_text_content(Some(i18n::text("Установка обновления…")));
                    command("install_update", &JsValue::UNDEFINED).await?;
                }
                Ok::<(), JsValue>(())
            }
            .await;
            if let Err(err) = result {
                web_sys::console::warn_1(&err);
                status.set_text_content(Some(i18n::text(
                    "Не удалось обновить GuitarCraft. Попробуйте ещё раз.",
                )));
            }
            set(&button, "disabled", &false.into());
            set(&later, "disabled", &false.into());
        });
    });
    Ok(())
}
