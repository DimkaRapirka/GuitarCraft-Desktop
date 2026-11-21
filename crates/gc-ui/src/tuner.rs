use crate::{dom::*, host, js, read};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::spawn_local;
use web_sys::{Event, HtmlInputElement, KeyboardEvent};

#[derive(Clone, Serialize, Deserialize)]
struct Reading {
    note: String,
    octave: i32,
    cents: i32,
    freq: f64,
}

struct State {
    open: Cell<bool>,
    reference: Cell<f64>,
    muted: Cell<bool>,
    busy: Cell<bool>,
    last_hit: Cell<f64>,
    reading: RefCell<Option<Reading>>,
}
impl State {
    fn apply_mute(&self) {
        let args = js(&json!({"on":self.open.get() && self.muted.get()})).unwrap();
        spawn_local(async move {
            if let Err(e) = host::command("engine_mute", &args).await {
                web_sys::console::warn_1(&e)
            }
        });
    }
    fn close(&self) {
        self.open.set(false);
        if let Some(el) = doc().query_selector(".tuner").unwrap() {
            el.remove()
        }
        if let Some(el) = doc().get_element_by_id("btn-tuner") {
            let _ = el.class_list().remove_1("on");
        }
        self.apply_mute();
    }
    fn render(&self) {
        let reading = self.reading.borrow();
        let r = reading.as_ref();
        let tuned = r.is_some_and(|v| v.cents.abs() <= 3);
        let signed = |n: i32| {
            if n > 0 {
                format!("+{n}")
            } else {
                n.to_string()
            }
        };
        if let Some(mini) = doc().get_element_by_id("mini-tuner") {
            let text = r
                .map(|v| format!("{}{} {}¢", v.note, v.octave, signed(v.cents)))
                .unwrap_or_else(|| "—".into());
            mini.set_text_content(Some(&text));
            let _ = mini.class_list().toggle_with_force("in-tune", tuned);
        }
        if !self.open.get() {
            return;
        }
        if let Some(el) = doc().get_element_by_id("tn-note") {
            el.set_text_content(Some(r.map(|v| v.note.as_str()).unwrap_or("—")));
            if let Some(v) = r {
                child(&el, "sub", "", &v.octave.to_string());
            }
            let _ = el.class_list().toggle_with_force("ok", tuned);
        }
        if let Some(el) = doc().get_element_by_id("tn-needle") {
            style(
                &el,
                "left",
                &format!("{}%", 50 + r.map(|v| v.cents.clamp(-50, 50)).unwrap_or(0)),
            );
        }
        if let Some(el) = doc().get_element_by_id("tn-info") {
            let text = r
                .map(|v| {
                    crate::i18n::message(
                        "{} Гц · {} центов",
                        &[&v.freq.to_string(), &signed(v.cents)],
                    )
                })
                .unwrap_or_else(|| crate::i18n::text("Сыграйте открытую струну").into());
            el.set_text_content(Some(&text));
        }
    }
    fn poll(self: &Rc<Self>) {
        if self.busy.replace(true) {
            return;
        }
        let s = self.clone();
        spawn_local(async move {
            let args = js(&json!({"a4":s.reference.get()})).unwrap();
            if let Ok(v) = host::command("engine_tuner", &args).await {
                if let Ok(data) = read::<Option<Reading>>(v) {
                    let t = host::now();
                    if data.is_some() {
                        s.last_hit.set(t);
                        s.reading.replace(data);
                    } else if t - s.last_hit.get() > 250. {
                        s.reading.replace(None);
                    }
                    s.render();
                }
            }
            s.busy.set(false);
        });
    }
}
#[wasm_bindgen]
pub struct Tuner {
    state: Rc<State>,
    escape: Closure<dyn FnMut(Event)>,
    poll_timer: i32,
    _poll: Closure<dyn FnMut()>,
}
#[wasm_bindgen]
impl Tuner {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Tuner {
        let state = Rc::new(State {
            open: Cell::new(false),
            reference: Cell::new(440.),
            muted: Cell::new(true),
            busy: Cell::new(false),
            last_hit: Cell::new(0.),
            reading: RefCell::new(None),
        });
        let weak = Rc::downgrade(&state);
        let escape = Closure::wrap(Box::new(move |e: Event| {
            if e.dyn_ref::<KeyboardEvent>()
                .is_some_and(|k| k.key() == "Escape")
            {
                if let Some(s) = weak.upgrade().filter(|s| s.open.get()) {
                    s.close();
                }
            }
        }) as Box<dyn FnMut(Event)>);
        doc()
            .add_event_listener_with_callback("keydown", escape.as_ref().unchecked_ref())
            .unwrap();
        let weak = Rc::downgrade(&state);
        let poll = Closure::<dyn FnMut()>::new(move || {
            if let Some(state) = weak.upgrade() {
                state.poll();
            }
        });
        let poll_timer = web_sys::window()
            .unwrap()
            .set_interval_with_callback_and_timeout_and_arguments_0(
                poll.as_ref().unchecked_ref(),
                80,
            )
            .unwrap();
        Tuner { state, escape, poll_timer, _poll: poll }
    }
    #[wasm_bindgen(getter)]
    pub fn open(&self) -> bool {
        self.state.open.get()
    }
    #[wasm_bindgen(getter,js_name=ref)]
    pub fn reference(&self) -> f64 {
        self.state.reference.get()
    }
    #[wasm_bindgen(setter,js_name=ref)]
    pub fn set_reference(&self, v: f64) {
        self.state.reference.set(if v.is_finite() {
            v.clamp(415., 466.)
        } else {
            440.
        });
    }
    #[wasm_bindgen(getter)]
    pub fn mute(&self) -> bool {
        self.state.muted.get()
    }
    #[wasm_bindgen(setter)]
    pub fn set_mute(&self, v: bool) {
        self.state.muted.set(v);
        if self.open() {
            self.state.apply_mute();
        }
    }
    #[wasm_bindgen(getter)]
    pub fn reading(&self) -> Result<JsValue, JsValue> {
        js(&*self.state.reading.borrow())
    }
    pub fn toggle(&self, force: Option<bool>) {
        let s = &self.state;
        if !force.unwrap_or(!s.open.get()) {
            s.close();
            return;
        }
        if s.open.replace(true) {
            return;
        }
        s.apply_mute();
        if let Some(button) = doc().get_element_by_id("btn-tuner") {
            let _ = button.class_list().add_1("on");
        }
        let el = make("div", "tuner");
        child(&el, "div", "tn-note", "—").set_id("tn-note");
        let scale = child(&el, "div", "tn-scale", "");
        child(&scale, "div", "tn-needle", "").set_id("tn-needle");
        for mark in [-50, -25, 0, 25, 50] {
            style(
                &child(&scale, "span", "", &mark.to_string()),
                "left",
                &format!("{}%", mark + 50),
            );
        }
        child(&el, "div", "tn-info", "Сыграйте открытую струну").set_id("tn-info");
        let row = child(&el, "div", "row", "");
        let label = child(&row, "label", "", "A4 ");
        let input: HtmlInputElement = child(&label, "input", "", "").unchecked_into();
        input.set_type("number");
        input.set_min("415");
        input.set_max("466");
        input.set_value_as_number(s.reference.get());
        label
            .append_child(&doc().create_text_node(&format!(" {}", crate::i18n::text("Гц"))))
            .unwrap();
        let state = s.clone();
        listen(&input, "change", move |e| {
            if let Some(i) = e
                .target()
                .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
            {
                let v = i.value_as_number();
                let v = if v.is_finite() {
                    v.clamp(415., 466.)
                } else {
                    440.
                };
                state.reference.set(v);
                i.set_value_as_number(v);
            }
        });
        let mute = child(
            &row,
            "button",
            "tn-mute",
            if s.muted.get() {
                "MUTE"
            } else {
                crate::i18n::text("ЗВУК")
            },
        );
        let _ = mute.class_list().toggle_with_force("on", s.muted.get());
        let state = s.clone();
        listen(&mute, "click", move |e| {
            let on = !state.muted.get();
            state.muted.set(on);
            state.apply_mute();
            if let Some(b) = e
                .current_target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            {
                let _ = b.class_list().toggle_with_force("on", on);
                b.set_text_content(Some(if on {
                    "MUTE"
                } else {
                    crate::i18n::text("ЗВУК")
                }));
            }
        });
        let close = child(&row, "button", "", "Закрыть");
        let state = s.clone();
        listen(&close, "click", move |_| state.close());
        doc().body().unwrap().append_child(&el).unwrap();
        s.render();
    }

}
impl Drop for Tuner {
    fn drop(&mut self) {
        web_sys::window().unwrap().clear_interval_with_handle(self.poll_timer);
        let _ = doc()
            .remove_event_listener_with_callback("keydown", self.escape.as_ref().unchecked_ref());
        if self.state.open.get() {
            self.state.close();
        }
    }
}
