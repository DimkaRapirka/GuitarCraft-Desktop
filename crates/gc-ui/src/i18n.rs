//! Display strings use Russian source text as a fallback. Locale files never
//! change identifiers, preset data, or the language of the control protocol.
use std::{borrow::Cow, cell::Cell, collections::HashMap, sync::LazyLock};

pub const LANGUAGES: &[(&str, &str)] = &[("en", "English"), ("ru", "Русский")];
static ENGLISH: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    include_str!("../locales/en.tsv")
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .collect()
});
thread_local! {
    static MESSAGES: Cell<Option<&'static HashMap<&'static str, &'static str>>> = const { Cell::new(None) };
}

pub fn language(code: &str) -> &str {
    LANGUAGES
        .iter()
        .find(|(id, _)| *id == code)
        .map_or("en", |(id, _)| *id)
}

pub fn init(code: &str) {
    MESSAGES.set(match language(code) {
        "en" => Some(&ENGLISH),
        _ => None,
    });
    if let Some(root) = crate::dom::doc().document_element() {
        let _ = root.set_attribute("lang", language(code));
    }
}

pub fn text(source: &str) -> &str {
    MESSAGES
        .get()
        .and_then(|messages| messages.get(source))
        .copied()
        .unwrap_or(source)
}

pub fn template(source: &str) -> Cow<'_, str> {
    if MESSAGES.get().is_none() {
        return Cow::Borrowed(source);
    }
    // Apply translation only to the explicit static strings in embedded HTML.
    let mut html = source.to_owned();
    for key in [
        "Слушаете сохранённую версию (A). Ручки показывают её и не редактируются. Нажмите B, чтобы вернуться к правке.",
        "MCP-агент",
        "＋ Собрать свою педаль…",
        "Название, тип или характер звука: «тёплый овердрайв», «эхо», «фузз»…",
    ] {
        html = html.replace(key, text(key));
    }
    Cow::Owned(html)
}

pub fn message(source: &str, values: &[&str]) -> String {
    let mut parts = text(source).split("{}");
    let mut output = parts.next().unwrap_or_default().to_owned();
    for (value, part) in values.iter().zip(parts) {
        output.push_str(value);
        output.push_str(part);
    }
    output
}
