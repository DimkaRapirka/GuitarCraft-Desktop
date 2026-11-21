//! Display strings use Russian source text as a fallback. Locale files never
//! change identifiers, preset data, or the language of the control protocol.
use std::{borrow::Cow, cell::RefCell, collections::HashMap};

pub struct Locale {
    pub id: &'static str,
    pub name: &'static str,
    catalog: &'static str,
}

pub const LANGUAGES: &[Locale] = &[
    Locale {
        id: "en",
        name: "English",
        catalog: include_str!("../locales/en.tsv"),
    },
    Locale {
        id: "ru",
        name: "Русский",
        catalog: "",
    },
];
thread_local! {
    static MESSAGES: RefCell<HashMap<&'static str, &'static str>> = RefCell::default();
}

fn locale(code: &str) -> &'static Locale {
    LANGUAGES
        .iter()
        .find(|entry| entry.id == code)
        .unwrap_or(&LANGUAGES[0])
}

pub fn language(code: &str) -> &'static str {
    locale(code).id
}

pub fn init(code: &str) {
    let locale = locale(code);
    MESSAGES.with(|messages| {
        *messages.borrow_mut() = locale
            .catalog
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .collect();
    });
    if let Some(root) = crate::dom::doc().document_element() {
        let _ = root.set_attribute("lang", locale.id);
    }
}

pub fn text(source: &str) -> &str {
    MESSAGES
        .with(|messages| messages.borrow().get(source).copied())
        .unwrap_or(source)
}

pub fn template(source: &str) -> Cow<'_, str> {
    if MESSAGES.with(|messages| messages.borrow().is_empty()) {
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
