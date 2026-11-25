//! The settings pages share persisted preferences with MIDI, the tuner and the control API.
use crate::{
    backgrounds, builder,
    dom::*,
    host::{command, resolved, sleep},
    js,
    preferences::{apply_audio, save_settings},
    read,
};
use gc_model::catalog::{list, text, SEARCH};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::spawn_local;
use web_sys::Element;
thread_local! { static OPEN:RefCell<Option<Rc<Panel>>>=const{RefCell::new(None)}; }
const TABS: [(&str, &str); 8] = [
    ("audio", "Аудио"),
    ("midi", "MIDI"),
    ("tuner", "Тюнер и темп"),
    ("appearance", "Оформление"),
    ("plugins", "Плагины и форматы"),
    ("agents", "Агенты (MCP)"),
    ("language", "Язык"),
    ("about", "О программе"),
];
struct Panel {
    app: JsValue,
    modal: Element,
    nav: Element,
    body: Element,
    tab: RefCell<String>,
    version: Cell<u64>,
}
fn attr(el: &Element, key: &str, value: &str) {
    let value = match key {
        "title" | "placeholder" | "aria-label" => crate::i18n::text(value),
        _ => value,
    };
    el.set_attribute(key, value).unwrap();
}
fn row(parent: &Element, label: &str, hint: &str) -> Element {
    let r = child(parent, "div", "set-row", "");
    let l = child(&r, "div", "set-label", label);
    if !hint.is_empty() {
        child(&l, "small", "", hint);
    }
    child(&r, "div", "set-ctl", "")
}
fn field(el: &Element, key: &str, id: &str) {
    attr(el, "data-setting", key);
    if !id.is_empty() {
        el.set_id(id);
    }
}
fn opt(el: &Element, v: &str, label: &str) {
    attr(&child(el, "option", "", label), "value", v);
}
fn select(
    parent: &Element,
    key: &str,
    id: &str,
    options: Vec<(String, String)>,
    value: &str,
) -> Element {
    let el = child(parent, "select", "", "");
    field(&el, key, id);
    for (k, label) in options {
        opt(&el, &k, &label);
    }
    set(&el, "value", &value.into());
    el
}
fn check(parent: &Element, key: &str, id: &str, value: bool, switch: bool) -> Element {
    let p = if switch {
        child(parent, "label", "switch", "")
    } else {
        parent.clone()
    };
    let el = child(&p, "input", "", "");
    attr(&el, "type", "checkbox");
    field(&el, key, id);
    set(&el, "checked", &value.into());
    if switch {
        child(&p, "span", "", "");
    }
    el
}
fn number(
    parent: &Element,
    key: &str,
    value: impl ToString,
    min: f64,
    max: f64,
    range: bool,
) -> Element {
    let e = child(parent, "input", "", "");
    field(&e, key, "");
    attr(&e, "type", if range { "range" } else { "number" });
    attr(&e, "min", &min.to_string());
    attr(&e, "max", &max.to_string());
    set(&e, "value", &value.to_string().into());
    e
}
fn button(parent: &Element, action: &str, id: &str, label: &str, on: bool) -> Element {
    let e = child(parent, "button", if on { "on" } else { "" }, label);
    attr(&e, "data-cmd", action);
    if !id.is_empty() {
        e.set_id(id);
    }
    e
}
async fn call(o: &JsValue, name: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
    resolved(invoke(o, name, args)?).await
}
fn i(v: &Value, key: &str, d: i64) -> i64 {
    v[key]
        .as_i64()
        .or_else(|| v[key].as_f64().map(|n| n as i64))
        .unwrap_or(d)
}
impl Panel {
    fn toast(&self, msg: &str) {
        let _ = invoke(&self.app, "toast", &[msg.into()]);
    }
    fn report(&self, e: JsValue) {
        self.toast(
            &get(&e, "message")
                .as_string()
                .or_else(|| e.as_string())
                .unwrap_or_else(|| "Ошибка настроек".into()),
        );
    }
    fn go(self: &Rc<Self>, tab: &str) {
        if !self.modal.is_connected() {
            return;
        }
        self.tab.replace(tab.into());
        self.nav.set_text_content(None);
        for (id, name) in TABS {
            let b = child(
                &self.nav,
                "button",
                if tab == id { "set-tab on" } else { "set-tab" },
                name,
            );
            attr(&b, "data-set-tab", id);
        }
        self.body.set_text_content(None);
        let version = self.version.get() + 1;
        self.version.set(version);
        let p = self.clone();
        let tab = tab.to_owned();
        spawn_local(async move {
            let root = make("div", "");
            let result = p.render(&root, &tab).await;
            if version != p.version.get() || !p.modal.is_connected() {
                return;
            }
            if let Err(e) = result {
                p.report(e);
                return;
            }
            while let Some(n) = root.first_child() {
                p.body.append_child(&n).unwrap();
            }
            if tab == "audio" {
                p.meter(version);
            }
        });
    }
    fn meter(self: &Rc<Self>, version: u64) {
        let Some(m) = self.body.query_selector(".set-meter").ok().flatten() else {
            return;
        };
        let bar = m.first_element_child().unwrap();
        let weak = Rc::downgrade(self);
        spawn_local(async move {
            while sleep(50.).await.is_ok() {
                let Some(p) = weak.upgrade() else {
                    break;
                };
                if version != p.version.get() || !m.is_connected() {
                    break;
                }
                let level = invoke(&get(&p.app, "engine"), "peak", &["raw".into()])
                    .ok()
                    .and_then(|n| n.as_f64())
                    .unwrap_or(0.);
                style(
                    &bar,
                    "width",
                    &format!(
                        "{}%",
                        ((20. * (level + 1e-6).log10() + 60.) / 60. * 100.).clamp(0., 100.)
                    ),
                );
                let _ = m.class_list().toggle_with_force("clip", level > 0.95);
            }
        });
    }
    async fn render(&self, p: &Element, tab: &str) -> Result<(), JsValue> {
        let s: Value = read(get(&self.app, "settings"))?;
        match tab {
            "audio" => self.audio(p, &s).await?,
            "midi" => self.midi(p, &s).await?,
            "plugins" => self.plugins(p).await?,
            "appearance" => self.appearance(p, &s).await?,
            "agents" => self.agents(p, &s).await?,
            "language" => {
                select(
                    &row(
                        p,
                        "Язык интерфейса",
                        "Язык изменится при следующем запуске.",
                    ),
                    "language",
                    "set-language",
                    crate::i18n::LANGUAGES
                        .iter()
                        .map(|locale| (locale.id.to_owned(), locale.name.to_owned()))
                        .collect(),
                    crate::i18n::language(text(&s, "language")),
                );
            }
            "tuner" => {
                child(p, "h3", "", "Тюнер");
                number(
                    &row(p, "Эталон A4", "Гц"),
                    "tunerRef",
                    i(&s, "tunerRef", 440),
                    415.,
                    466.,
                    false,
                );
                check(
                    &row(p, "Глушить выход при настройке", ""),
                    "tunerMute",
                    "",
                    s["tunerMute"] == true,
                    false,
                );
                child(p, "h3", "", "Темп и метроном");
                check(
                    &row(
                        p,
                        "Темп из пресета",
                        "Пресет хранит свой BPM для Delay Sync. Выключите, чтобы темп был общим.",
                    ),
                    "presetTempo",
                    "",
                    s["presetTempo"] != false,
                    true,
                );
                number(
                    &row(p, "Темп", "BPM, также Tap в нижней панели"),
                    "bpm",
                    i(&s, "bpm", 120),
                    30.,
                    300.,
                    false,
                );
                attr(
                    &number(
                        &row(p, "Громкость метронома", ""),
                        "metronomeLevel",
                        s["metronomeLevel"].as_f64().unwrap_or(0.5),
                        0.,
                        1.,
                        true,
                    ),
                    "step",
                    "0.05",
                );
            }
            _ => {
                child(p, "h3", "", "GuitarCraft");
                child(p,"p","muted","Усилитель, педали и кабинеты для электрогитары. DSP на Rust, 4× оверсэмплинг в перегрузах.");
                let info: Value = read(command("app_info", &JsValue::UNDEFINED).await?)?;
                let version = info["version"].as_str().unwrap_or_default();
                child(p, "h3", "", "Обновления");
                let r = row(
                    p,
                    &crate::i18n::message("Версия {}", &[version]),
                    "Приложение проверяет обновления при запуске",
                );
                let b = button(&r, "check-update", "btn-check-update", "Проверить обновления", false);
                attr(&b, "data-version", version);
                child(p, "p", "muted small", "").set_id("update-status");
            }
        }
        Ok(())
    }
    async fn audio(&self, p: &Element, s: &Value) -> Result<(), JsValue> {
        let engine = get(&self.app, "engine");
        let status: Value = read(call(&engine, "refreshStatus", &[]).await?)?;
        let dev: Value = read(call(&engine, "devices", &[]).await?)?;
        let sr = num(&get(&engine, "ctx"), "sampleRate", 48000.);
        let live = s["inputMode"] == "live";
        let lat = child(p, "div", "lat-box", "");
        child(
            &lat,
            "b",
            "",
            &crate::i18n::message(
                "Оценка задержки: ≈{} мс",
                &[&format!("{:.0}", num(&engine, "latencyMs", 0.))],
            ),
        );
        let delay = |key: &str| format!("{:.1}", status[key].as_f64().unwrap_or(0.));
        child(
            &lat, "span", "",
            &if status["live"] == true {
                crate::i18n::message(
                    "Вход {} мс + очередь {} мс + выход {} мс",
                    &[&delay("capture_ms"), &delay("queue_ms"), &delay("out_ms")],
                )
            } else {
                crate::i18n::message("Выход {} мс", &[&delay("out_ms")])
            },
        );
        child(p, "h3", "", "Источник");
        let seg = child(&row(p, "Что подаётся на вход", ""), "div", "seg", "");
        button(
            &seg,
            "live",
            "set-mode-live",
            "Гитара (аудиоинтерфейс)",
            live,
        );
        button(&seg, "di", "set-mode-di", "DI-записи", !live);
        let devices = |key: &str, default: &str| {
            let name = text(&dev, default);
            let mut v = vec![(
                "default".into(),
                if name.is_empty() {
                    "По умолчанию".into()
                } else {
                    crate::i18n::message("По умолчанию ({})", &[&name])
                },
            )];
            v.extend(
                list(&dev[key])
                    .iter()
                    .map(|x| (text(x, "id").into(), text(x, "label").into())),
            );
            v
        };
        child(p, "h3", "", "Вход");
        select(
            &row(p, "Устройство входа", ""),
            "inputDevice",
            "set-input-device",
            devices("inputs", "defaultInput"),
            s["inputDevice"].as_str().unwrap_or("default"),
        );
        let live_info = get(&engine, "liveInfo");
        let count = num(&live_info, "channels", 1.) as i64;
        let hint = if live_info.is_object() {
            crate::i18n::message("Устройство отдаёт каналов: {}", &[&count.to_string()])
        } else {
            "Список каналов появится после включения живого входа".into()
        };
        select(
            &row(p, "Канал интерфейса", &hint),
            "inputChannel",
            "set-input-channel",
            (0..count)
                .map(|n| {
                    (
                        n.to_string(),
                        crate::i18n::message("Вход {}", &[&(n + 1).to_string()]),
                    )
                })
                .collect(),
            &i(s, "inputChannel", 0).to_string(),
        );
        let r = if live {
            row(p,"Сигнал интерфейса (до INPUT)","Если краснеет, уменьшите Gain на самом аудиоинтерфейсе. Ручка INPUT сверху только подаёт уровень в усилитель и не спасает от перегруза входа")
        } else {
            row(p,"Уровень DI-записи","Уровень самой DI-записи. Громкость, с которой она идёт в усилитель, задаёт ручка INPUT сверху")
        };
        let wrap = child(&r, "div", "row", "");
        style(&wrap, "flex-wrap", "nowrap");
        style(&wrap, "width", "100%");
        child(&child(&wrap, "div", "set-meter", ""), "i", "", "");
        child(p, "h3", "", "Выход");
        select(
            &row(p, "Устройство выхода", ""),
            "outputDevice",
            "set-output-device",
            devices("outputs", "defaultOutput"),
            s["outputDevice"].as_str().unwrap_or("default"),
        );
        let active_output = text(&status, "output");
        let fault = text(&status, "outputError");
        let out_channels = i(&status, "outChannels", 0);
        let route = if fault.is_empty() && out_channels > 0 {
            let first = i(&status, "outputPair", 0) * 2 + 1;
            let channels = if first < out_channels {
                format!("{first} / {}", first + 1)
            } else {
                first.to_string()
            };
            crate::i18n::message("{} · каналы {}", &[active_output, &channels])
        } else if fault.is_empty() {
            crate::i18n::text("Аудиовыход не открыт").into()
        } else {
            fault.to_owned()
        };
        child(&row(p, "Активный выход", ""), "span", "mono", &route).set_id("active-output");
        let ch = out_channels.max(1);
        select(
            &row(p, "Каналы выхода", ""),
            "outputPair",
            "",
            (0..(ch + 1) / 2)
                .map(|n| {
                    (
                        n.to_string(),
                        if n * 2 + 2 > ch {
                            (n * 2 + 1).to_string()
                        } else {
                            format!("{} / {}", n * 2 + 1, n * 2 + 2)
                        },
                    )
                })
                .collect(),
            &i(&status, "outputPair", i(s, "outputPair", 0)).to_string(),
        );
        select(
            &row(p, "Режим выхода", ""),
            "mono",
            "",
            vec![
                ("stereo".into(), "Стерео".into()),
                ("mono".into(), "Моно".into()),
            ],
            if s["mono"] == true { "mono" } else { "stereo" },
        );
        button(
            &row(p, "Проверка", ""),
            "tone",
            "set-test-tone",
            "♪ Тестовый сигнал 440 Гц",
            false,
        );
        child(p, "h3", "", "Движок");
        let mut rates = list(&status["sampleRates"])
            .iter()
            .filter_map(Value::as_f64)
            .collect::<Vec<_>>();
        if rates.is_empty() {
            rates.push(sr);
        }
        select(
            &row(
                p,
                "Частота дискретизации",
                "Частоты, которые поддерживает устройство выхода",
            ),
            "sampleRate",
            "set-sr",
            rates
                .iter()
                .map(|r| {
                    (
                        r.to_string(),
                        crate::i18n::message("{} кГц", &[&(r / 1000.).to_string()]),
                    )
                })
                .collect(),
            &sr.to_string(),
        );
        select(
            &row(
                p,
                "Размер буфера",
                if status["outputDefaultBuffer"] == true {
                    "Для выхода используется размер буфера драйвера."
                } else {
                    "Фактический буфер зависит от драйвера. Если слышны щелчки, увеличьте размер."
                },
            ),
            "bufferSize",
            "set-buffer",
            [64, 128, 256, 512, 1024]
                .into_iter()
                .map(|n| {
                    (
                        n.to_string(),
                        crate::i18n::message(
                            "{} сэмплов · {} мс",
                            &[&n.to_string(), &format!("{:.1}", n as f64 / sr * 1000.)],
                        ),
                    )
                })
                .collect(),
            &i(s, "bufferSize", 128).to_string(),
        );
        child(
            &row(p, "Движок", ""),
            "span",
            "mono",
            &format!("Rust · cpal · {}", text(&status, "output")),
        );
        Ok(())
    }
    async fn midi(&self, p: &Element, s: &Value) -> Result<(), JsValue> {
        let midi = get(&self.app, "midi");
        resolved(get(&midi, "ready")).await?;
        child(p, "h3", "", "MIDI-устройства");
        let ports: Vec<Value> = read(get(&midi, "inputs"))?;
        if ports.is_empty() {
            child(p,"p","muted","MIDI-контроллеры не найдены. Подключите футконтроллер или клавиатуру по USB: список обновится сам.");
        } else {
            for port in ports {
                let e = row(p, text(&port, "name"), "");
                e.class_list().add_1("muted")?;
                e.set_text_content(Some(if port["state"] == "connected" {
                    crate::i18n::text("● подключено")
                } else {
                    text(&port, "state")
                }));
            }
        }
        check(
            &row(
                p,
                "Program Change переключает пресеты",
                "PC 0 — первый пресет в списке, PC 1 — второй…",
            ),
            "midiPC",
            "",
            s["midiPC"] != false,
            true,
        );
        let maps = list(&s["midiMappings"]);
        child(
            p,
            "h3",
            "",
            &crate::i18n::message("Назначения ({})", &[&maps.len().to_string()]),
        );
        child(p,"p","muted small","Чтобы назначить: правый клик по любой ручке или блоку цепочки → «MIDI Learn», затем покрутите регулятор или нажмите педаль контроллера.");
        for m in maps {
            let r = child(&row(p, text(m, "name"), ""), "div", "row", "");
            attr(&r, "data-mapping", text(m, "id"));
            child(
                &r,
                "span",
                "mono",
                &format!("{} → {}", text(m, "label"), text(m, "name")),
            );
            let dev = text(m, "device");
            let mut opts = vec![("*".into(), "любое устройство".into())];
            if !dev.is_empty() && dev != "*" {
                opts.insert(
                    0,
                    (dev.to_owned(), crate::i18n::message("только {}", &[dev])),
                );
            }
            attr(
                &select(
                    &r,
                    "mapping-device",
                    "",
                    opts,
                    if dev.is_empty() { "*" } else { dev },
                ),
                "title",
                "С какого контроллера принимать",
            );
            button(&r, "unmap", "", "✕", false).set_class_name("icon");
        }
        child(p, "h3", "", "Program Change → пресет");
        child(p,"p","muted small","Закрепите номер PC за пресетом: переключение не зависит от порядка в списке. Если таблица заполнена, незакреплённый PC ничего не переключает и показывает сообщение. Пустая таблица означает, что PC n выбирает n-й пресет.");
        for pr in read::<Vec<Value>>(get(&self.app, "presets"))? {
            let cur = s["midiPCMap"]
                .as_object()
                .and_then(|m| m.iter().find(|(_, v)| **v == pr["file"]))
                .map(|(k, _)| k.as_str())
                .unwrap_or("");
            let e = number(
                &row(p, text(&pr, "name"), text(&pr, "bank")),
                "pc-map",
                cur,
                0.,
                127.,
                false,
            );
            attr(&e, "placeholder", "—");
            attr(&e, "data-file", text(&pr, "file"));
            attr(&e, "style", "width:90px;min-width:90px");
        }
        let last: Option<Value> = read(get(&midi, "last"))?;
        if let Some(last) = last {
            child(
                p,
                "p",
                "muted small",
                &crate::i18n::message(
                    "Последнее сообщение: [{}] от {}",
                    &[
                        &list(&last["data"])
                            .iter()
                            .map(Value::to_string)
                            .collect::<Vec<_>>()
                            .join(", "),
                        text(&last, "from"),
                    ],
                ),
            );
        }
        Ok(())
    }
    async fn plugins(&self, p: &Element) -> Result<(), JsValue> {
        let cat = get(&self.app, "catalog");
        call(&cat, "scanPlugins", &[]).await?;
        let all: Vec<Value> = read(invoke(&cat, "all", &[])?)?;
        let found = all
            .iter()
            .filter(|v| v["source"] == "plugin")
            .collect::<Vec<_>>();
        let dirs: Value = read(command("fs_dirs", &JsValue::UNDEFINED).await?)?;
        child(p, "h3", "", "Какие форматы работают");
        let table = child(p, "table", "formats-table", "");
        for f in list(&SEARCH["formats"]) {
            let tr = child(&table, "tr", "", "");
            child(&child(&tr, "td", "", ""), "b", "", text(f, "format"));
            child(
                &tr,
                "td",
                if f["status"] == "работает" {
                    "st-ok"
                } else {
                    "st-no"
                },
                text(f, "status"),
            );
            child(&tr, "td", "muted", text(f, "note"));
        }
        child(
            p,
            "h3",
            "",
            &crate::i18n::message("Найдено при сканировании: {}", &[&found.len().to_string()]),
        );
        let list = child(p, "div", "muted small", "");
        for d in found {
            child(
                &list,
                "div",
                "",
                &format!("{} · {}", text(d, "format"), text(d, "name")),
            );
        }
        let r = child(p, "div", "row", "");
        style(&r, "margin-top", "12px");
        for (k, title) in [
            ("extensions", "Папка расширений"),
            ("pedals", "Папка моих педалей"),
        ] {
            attr(
                &button(&r, "reveal", "", title, false),
                "data-path",
                text(&dirs, k),
            );
        }
        button(&r, "reload", "", "Перезагрузить расширения", false);
        button(&r, "builder", "", "Конструктор педалей…", false);
        Ok(())
    }
    async fn agents(&self, p: &Element, s: &Value) -> Result<(), JsValue> {
        let info: Value = read(command("app_info", &JsValue::UNDEFINED).await?)?;
        child(p, "h3", "", "Агенты через MCP");
        child(p,"p","muted","GuitarCraft может принимать команды от AI-агента (Claude Code, Codex и других MCP-клиентов). Агент ищет педали в том же каталоге, меняет цепочку и параметры, слушает звук, сохраняет пресеты. Любой его шаг отменяется ⌘Z.");
        let on = s["mcpEnabled"] == true;
        check(
            &row(
                p,
                "Разрешить агентам управлять GuitarCraft",
                "Только локально, с этого компьютера",
            ),
            "mcpEnabled",
            "set-mcp",
            on,
            true,
        );
        let panel = check(
            &row(p, "Показывать панель действий агента", ""),
            "showAgentPanel",
            "set-agent-panel",
            s["showAgentPanel"] == true,
            true,
        );
        set(&panel, "disabled", &(!on).into());
        if on {
            let r = child(p, "div", "", "");
            child(&r, "h3", "", "Подключение");
            child(&r,"p","muted small","Добавьте сервер в конфигурацию MCP-клиента (например, .mcp.json проекта или настройки Claude Code):");
            let snippet = serde_json::to_string_pretty(
                &json!({"mcpServers":{"guitarcraft":{"command":info["mcpServer"],"args":[]}}}),
            )
            .unwrap();
            let area = child(&r, "textarea", "prompt-box", &snippet);
            attr(&area, "readonly", "");
            style(&area, "height", "120px");
            let cmds = child(&r, "div", "row", "");
            style(&cmds, "margin-top", "8px");
            button(&cmds, "copy", "", "Скопировать конфигурацию", false);
            let last = get(&self.app, "lastAgentCall");
            let msg = if last.is_truthy() {
                crate::i18n::message(
                    "Последнее обращение агента: {}",
                    &[&js_sys::Date::new(&last)
                        .to_locale_time_string(crate::i18n::language(text(s, "language")))
                        .as_string()
                        .unwrap_or_default()],
                )
            } else {
                "Агент ещё не подключался в этой сессии.".into()
            };
            child(&r, "p", "muted small", &msg);
        }
        Ok(())
    }
    async fn appearance(&self, p: &Element, s: &Value) -> Result<(), JsValue> {
        let images = backgrounds::list().await?;
        child(p, "h3", "", "Фоны рабочей области");
        child(
            p,
            "p",
            "muted",
            "Выберите фон для каждой сцены. Поддерживаются PNG, JPEG и WebP.",
        );
        for (scene, label) in backgrounds::SCENES {
            let controls = child(
                &row(p, label, ""),
                "div",
                "row scene-background-controls",
                "",
            );
            let mut options = vec![(String::new(), "Стандартный фон".into())];
            options.extend(
                images
                    .iter()
                    .map(|image| (image.id.clone(), image.label().to_owned())),
            );
            let id = backgrounds::selected(s, scene)
                .filter(|id| images.iter().any(|image| image.id == *id));
            let field = select(
                &controls,
                "sceneBackgrounds",
                &format!("background-{scene}"),
                options,
                id.as_deref().unwrap_or(""),
            );
            attr(&field, "data-scene", scene);
            attr(&field, "aria-label", label);
            let import = button(
                &controls,
                "background-import",
                &format!("import-background-{scene}"),
                "Импорт…",
                false,
            );
            attr(&import, "data-scene", scene);
        }
        Ok(())
    }
    async fn event(self: &Rc<Self>, el: Element, kind: &str) -> Result<(), JsValue> {
        let tab = self.tab.borrow().clone();
        let engine = get(&self.app, "engine");
        let mut patch = json!({});
        if kind == "click" {
            if el == self.modal {
                close_settings();
                return Ok(());
            }
            if let Some(b) = el.closest("[data-set-tab]")? {
                self.go(&b.get_attribute("data-set-tab").unwrap());
                return Ok(());
            }
            let Some(b) = el.closest("[data-cmd]")? else {
                return Ok(());
            };
            match b.get_attribute("data-cmd").as_deref() {
                Some("close") => close_settings(),
                Some("tone") => {
                    if let Err(error) = call(&engine, "testTone", &[]).await {
                        let message = get(&error, "message").as_string().or_else(|| error.as_string());
                        if let Some(message) = message {
                            self.toast(&crate::i18n::message("Аудио: {}", &[crate::i18n::text(&message)]));
                        } else {
                            return Err(error);
                        }
                        self.go("audio");
                    }
                }
                Some("background-import") => {
                    let scene = b.get_attribute("data-scene").unwrap_or_default();
                    backgrounds::pick(&self.app, &scene).await?;
                    if self.tab.borrow().as_str() == "appearance" {
                        self.go("appearance");
                    }
                }
                Some("builder") => {
                    close_settings();
                    builder::open_builder(self.app.clone(), JsValue::UNDEFINED)?;
                }
                Some("reveal") => {
                    command(
                        "reveal",
                        &js(&json!({"file":b.get_attribute("data-path")}))?,
                    )
                    .await?;
                }
                Some("reload") => {
                    let cat = get(&self.app, "catalog");
                    call(&cat, "loadExtensions", &[]).await?;
                    call(&cat, "loadUserPedals", &[]).await?;
                    self.toast("Расширения перезагружены");
                }
                Some("check-update") => {
                    let status = self.body.query_selector("#update-status")?;
                    let current = b.get_attribute("data-version").unwrap_or_default();
                    set(&b, "disabled", &true.into());
                    if let Some(s) = &status {
                        s.set_text_content(Some(crate::i18n::text("Проверяем…")));
                    }
                    let answer = crate::updates::check_now(&current).await;
                    set(&b, "disabled", &false.into());
                    if let Some(s) = status {
                        s.set_text_content(Some(&answer));
                    }
                }
                Some("copy") => {
                    if let Some(area) = self.body.query_selector("textarea")? {
                        command(
                            "clipboard_write",
                            &js(&json!({"text":string(&area,"value")}))?,
                        )
                        .await?;
                        self.toast("Конфигурация MCP скопирована");
                    }
                }
                Some("unmap") => {
                    if let Some(r) = b.closest("[data-mapping]")? {
                        call(
                            &get(&self.app, "midi"),
                            "unmap",
                            &[r.get_attribute("data-mapping").unwrap().into()],
                        )
                        .await?;
                        self.go("midi");
                    }
                }
                Some(mode @ ("live" | "di")) => {
                    save_settings(self.app.clone(), js(&json!({"inputMode":mode}))?).await?;
                    if mode == "di" {
                        invoke(&engine, "stopLive", &[])?;
                        invoke(&self.app, "toggleDI", &[true.into()])?;
                    } else {
                        apply_audio(self.app.clone()).await?;
                        invoke(&self.app, "renderFooter", &[])?;
                    }
                    if *self.tab.borrow() == tab {
                        self.go(&tab);
                    }
                }
                _ => {}
            }
            return Ok(());
        }
        let Some(key) = el.get_attribute("data-setting") else {
            return Ok(());
        };
        let value = string(&el, "value");
        if key == "sceneBackgrounds" {
            let scene = el.get_attribute("data-scene").unwrap_or_default();
            backgrounds::select(&self.app, &scene, Some(&value)).await?;
            return Ok(());
        }
        let s: Value = read(get(&self.app, "settings"))?;
        match key.as_str() {
            "pc-map" => {
                let file = el.get_attribute("data-file").unwrap_or_default();
                let mut map = s["midiPCMap"].as_object().cloned().unwrap_or_default();
                if !value.is_empty() && value.parse::<u8>().ok().filter(|n| *n <= 127).is_none() {
                    self.toast("Номер Program Change должен быть от 0 до 127");
                    self.go(&tab);
                    return Ok(());
                }
                map.retain(|_, v| v != &file);
                if !value.is_empty() {
                    map.insert(value, json!(file));
                }
                patch["midiPCMap"] = json!(map);
            }
            "mapping-device" => {
                let id = el
                    .closest("[data-mapping]")?
                    .and_then(|e| e.get_attribute("data-mapping"))
                    .unwrap_or_default();
                let mut maps = list(&s["midiMappings"]).to_vec();
                for m in &mut maps {
                    if m["id"] == id {
                        m["device"] = json!(value);
                    }
                }
                patch["midiMappings"] = json!(maps);
            }
            "language" => patch[&key] = json!(crate::i18n::language(&value)),
            "mono" => patch["mono"] = json!(value == "mono"),
            "mcpEnabled" | "showAgentPanel" | "presetTempo" | "midiPC" | "tunerMute" => {
                patch[&key] = json!(get(&el, "checked").is_truthy())
            }
            "inputDevice" => {
                patch[&key] = json!(value);
                patch["inputMode"] = json!("live");
            }
            "outputDevice" => {
                patch[&key] = json!(value);
                patch["outputPair"] = json!(0);
            }
            "inputChannel" | "outputPair" | "bufferSize" | "sampleRate" | "tunerRef" | "bpm"
            | "metronomeLevel" => {
                let n = value.parse::<f64>().unwrap_or(match key.as_str() {
                    "bpm" => 120.,
                    "tunerRef" => 440.,
                    _ => 0.,
                });
                patch[&key] = if key == "sampleRate" && n == 0. {
                    Value::Null
                } else {
                    json!(n)
                };
            }
            _ => return Ok(()),
        }
        let settings = save_settings(self.app.clone(), js(&patch)?).await?;
        match key.as_str() {
            "tunerRef" => set(&get(&self.app, "tuner"), "ref", &get(&settings, "tunerRef")),
            "tunerMute" => set(
                &get(&self.app, "tuner"),
                "mute",
                &get(&settings, "tunerMute"),
            ),
            "metronomeLevel" if get(&engine, "metronome").is_truthy() => {
                invoke(
                    &engine,
                    "setMetronome",
                    &[
                        true.into(),
                        get(&settings, "bpm"),
                        get(&settings, "metronomeLevel"),
                    ],
                )?;
            }
            "bpm" => {
                invoke(&self.app, "renderFooter", &[])?;
            }
            "mcpEnabled" => {
                if *self.tab.borrow() == tab {
                    self.go(&tab);
                }
            }
            _ if tab == "audio" => {
                if key != "inputChannel" || s["inputMode"] == "live" {
                    apply_audio(self.app.clone()).await?;
                }
                invoke(&self.app, "renderFooter", &[])?;
                if *self.tab.borrow() == tab {
                    self.go(&tab);
                }
            }
            _ => {}
        }
        Ok(())
    }
}
#[wasm_bindgen(js_name=closeSettings)]
pub fn close_settings() {
    if let Some(p) = OPEN.with(|o| o.borrow_mut().take()) {
        p.modal.remove();
    }
}
#[wasm_bindgen(js_name=openSettings)]
pub fn open_settings(app: JsValue, tab: Option<String>) -> Result<(), JsValue> {
    close_settings();
    let modal = make("div", "modal-bg settings-bg");
    let root = child(&modal, "div", "modal settings", "");
    let head = child(&root, "div", "cat-head", "");
    child(&head, "h2", "", "Настройки");
    button(&head, "close", "", "✕", false).set_class_name("icon close");
    let wrap = child(&root, "div", "set-wrap", "");
    let p = Rc::new(Panel {
        app,
        nav: child(&wrap, "div", "set-nav", ""),
        body: child(&wrap, "div", "set-body", ""),
        modal,
        tab: Default::default(),
        version: Default::default(),
    });
    for kind in ["click", "change"] {
        let weak = Rc::downgrade(&p);
        listen(&p.modal, kind, move |ev| {
            if let (Some(p), Some(el)) = (
                weak.upgrade(),
                ev.target().and_then(|t| t.dyn_into::<Element>().ok()),
            ) {
                spawn_local(async move {
                    if let Err(e) = p.event(el, kind).await {
                        p.report(e);
                    }
                });
            }
        });
    }
    doc().body().unwrap().append_child(&p.modal)?;
    p.go(tab.as_deref().unwrap_or("audio"));
    OPEN.with(|o| o.replace(Some(p)));
    Ok(())
}
