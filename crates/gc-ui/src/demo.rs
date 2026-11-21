//! Recording automation acts on the same DOM controls as the musician.
use crate::{dom::*, host, js, read};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{cell::Cell, rc::Rc};
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::{
    Element, Event, EventInit, HtmlElement, KeyboardEvent, KeyboardEventInit, MouseEvent,
    MouseEventInit, PointerEvent, PointerEventInit, ScrollIntoViewOptions, ScrollLogicalPosition,
};

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Step {
    wait: Option<f64>,
    say: Option<String>,
    say_ms: Option<f64>,
    #[serde(rename = "move")]
    move_to: Option<String>,
    click: Option<String>,
    #[serde(rename = "type")]
    type_into: Option<String>,
    text: String,
    cps: Option<f64>,
    drag: Option<String>,
    drop_on: Option<String>,
    dy: f64,
    dur: Option<f64>,
    key: Option<String>,
    hide_cursor: bool,
    call: Option<String>,
    args: Vec<Value>,
    after: Option<f64>,
}
struct Screen {
    cursor: Element,
    caption: Element,
    x: Cell<f64>,
    y: Cell<f64>,
    caption_id: Cell<u64>,
}
impl Screen {
    fn position(&self, x: f64, y: f64) {
        self.x.set(x);
        self.y.set(y);
        style(
            &self.cursor,
            "transform",
            &format!("translate({x}px, {y}px)"),
        );
    }
    async fn point(&self, sel: &str, ms: f64) -> Result<Element, JsValue> {
        let el = doc()
            .query_selector(sel)?
            .ok_or_else(|| js_sys::Error::new(&format!("Элемент не найден: {sel}")))?;
        let opts = ScrollIntoViewOptions::new();
        opts.set_block(ScrollLogicalPosition::Nearest);
        opts.set_inline(ScrollLogicalPosition::Nearest);
        el.scroll_into_view_with_scroll_into_view_options(&opts);
        let rect = el.get_bounding_client_rect();
        let (tx, ty) = (
            rect.left() + rect.width() / 2.,
            rect.top() + rect.height() / 2.,
        );
        let (x, y, start) = (self.x.get(), self.y.get(), host::now());
        self.cursor.class_list().remove_1("hidden")?;
        loop {
            let t = if ms > 0. {
                ((host::now() - start) / ms).min(1.)
            } else {
                1.
            };
            let k = if t < 0.5 {
                2. * t * t
            } else {
                1. - (-2. * t + 2.).powi(2) / 2.
            };
            self.position(x + (tx - x) * k, y + (ty - y) * k);
            if t >= 1. {
                return Ok(el);
            }
            host::sleep(16.).await?;
        }
    }
    async fn click(&self, sel: &str, ms: f64) -> Result<(), JsValue> {
        let el = self.point(sel, ms).await?;
        self.cursor.class_list().add_1("press")?;
        host::sleep(110.).await?;
        self.cursor.class_list().remove_1("press")?;
        let ev = MouseEventInit::new();
        ev.set_bubbles(true);
        ev.set_client_x(self.x.get() as i32);
        ev.set_client_y(self.y.get() as i32);
        let click = MouseEvent::new_with_mouse_event_init_dict("click", &ev)?;
        el.dispatch_event(&click)?;
        host::sleep(120.).await
    }
    async fn type_text(&self, sel: &str, text: &str, cps: f64) -> Result<(), JsValue> {
        let el = self.point(sel, 400.).await?;
        if let Some(el) = el.dyn_ref::<HtmlElement>() {
            el.focus()?
        }
        let init = EventInit::new();
        init.set_bubbles(true);
        for c in text.chars() {
            let mut v = string(&el, "value");
            v.push(c);
            set(&el, "value", &v.into());
            let input = Event::new_with_event_init_dict("input", &init)?;
            el.dispatch_event(&input)?;
            host::sleep(1000. / cps.max(1.)).await?;
        }
        Ok(())
    }
    async fn drag(&self, sel: &str, dy: f64, ms: f64) -> Result<(), JsValue> {
        let el = self.point(sel, 500.).await?;
        let body = el.query_selector(".knob-body")?.unwrap_or(el);
        let send = |kind: &str, y: f64| -> Result<(), JsValue> {
            let e = PointerEventInit::new();
            e.set_bubbles(true);
            e.set_client_x(self.x.get() as i32);
            e.set_client_y(y as i32);
            e.set_pointer_id(1);
            e.set_is_primary(true);
            let event = PointerEvent::new_with_event_init_dict(kind, &e)?;
            body.dispatch_event(&event)?;
            Ok(())
        };
        let y = self.y.get();
        send("pointerdown", y)?;
        self.cursor.class_list().add_1("press")?;
        let steps = (ms / 16.).round().max(8.) as usize;
        for i in 1..=steps {
            let dest = y + dy * i as f64 / steps as f64;
            self.position(self.x.get(), dest);
            send("pointermove", dest)?;
            host::sleep(ms / steps as f64).await?;
        }
        send("pointerup", self.y.get())?;
        self.cursor.class_list().remove_1("press")?;
        Ok(())
    }
    async fn drag_block(&self, from: &str, to: &str, ms: f64) -> Result<(), JsValue> {
        let source = self.point(from, 300.).await?;
        let event = |name: &str| -> Result<Event, JsValue> {
            let options = PointerEventInit::new();
            options.set_bubbles(true);
            options.set_cancelable(true);
            options.set_pointer_id(1);
            options.set_is_primary(true);
            options.set_client_x(self.x.get().round() as i32);
            options.set_client_y(self.y.get().round() as i32);
            PointerEvent::new_with_event_init_dict(name, &options).map(Into::into)
        };
        source.dispatch_event(&event("pointerdown")?)?;
        if let Err(error) = self.point(to, ms).await {
            source.dispatch_event(&event("pointercancel")?)?;
            return Err(error);
        }
        source.dispatch_event(&event("pointermove")?)?;
        source.dispatch_event(&event("pointerup")?)?;
        Ok(())
    }
    fn say(self: &Rc<Self>, text: &str, ms: Option<f64>) {
        let id = self.caption_id.get().wrapping_add(1);
        self.caption_id.set(id);
        self.caption.set_text_content(Some(text));
        let _ = self
            .caption
            .class_list()
            .toggle_with_force("hidden", text.is_empty());
        if let Some(ms) = ms.filter(|n| *n > 0.) {
            let weak = Rc::downgrade(self);
            wasm_bindgen_futures::spawn_local(async move {
                let _ = host::sleep(ms).await;
                if let Some(s) = weak.upgrade().filter(|s| s.caption_id.get() == id) {
                    let _ = s.caption.class_list().add_1("hidden");
                }
            });
        }
    }
}
#[wasm_bindgen]
pub struct Demo {
    app: JsValue,
    screen: Rc<Screen>,
}
#[wasm_bindgen]
impl Demo {
    #[wasm_bindgen(constructor)]
    pub fn new(app: JsValue) -> Demo {
        let cursor = make("div", "demo-cursor hidden");
        cursor.set_inner_html(r##"<svg width="26" height="26" viewBox="0 0 24 24"><path d="M4 2 L4 20 L9 15 L12.5 22 L15.5 20.5 L12 13.5 L19 13.5 Z" fill="#fff" stroke="#111" stroke-width="1.5" stroke-linejoin="round"/></svg>"##);
        let caption = make("div", "demo-caption hidden");
        let body = doc().body().unwrap();
        body.append_child(&cursor).unwrap();
        body.append_child(&caption).unwrap();
        let window = web_sys::window().unwrap();
        let (x, y) = (
            num(&window, "innerWidth", 0.) / 2.,
            num(&window, "innerHeight", 0.) / 2.,
        );
        Demo {
            app,
            screen: Rc::new(Screen {
                cursor,
                caption,
                x: Cell::new(x),
                y: Cell::new(y),
                caption_id: Cell::new(0),
            }),
        }
    }
    pub async fn run(&self, steps: JsValue) -> Result<JsValue, JsValue> {
        for s in read::<Vec<Step>>(steps)? {
            if let Some(ms) = s.wait {
                host::sleep(ms).await?;
            }
            if let Some(text) = s.say {
                self.screen.say(&text, s.say_ms);
            }
            if let Some(sel) = s.move_to {
                self.screen.point(&sel, s.dur.unwrap_or(600.)).await?;
            }
            if let Some(sel) = s.click {
                self.screen.click(&sel, s.dur.unwrap_or(600.)).await?;
            }
            if let Some(sel) = s.type_into {
                self.screen
                    .type_text(&sel, &s.text, s.cps.unwrap_or(14.))
                    .await?;
            }
            if let Some(sel) = s.drag {
                if let Some(target) = s.drop_on {
                    self.screen
                        .drag_block(&sel, &target, s.dur.unwrap_or(450.))
                        .await?;
                } else {
                    self.screen.drag(&sel, s.dy, s.dur.unwrap_or(900.)).await?;
                }
            }
            if let Some(key) = s.key {
                let init = KeyboardEventInit::new();
                init.set_bubbles(true);
                init.set_key(&key);
                let e = KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init)?;
                doc().dispatch_event(&e)?;
            }
            if s.hide_cursor {
                self.screen.cursor.class_list().add_1("hidden")?;
            }
            if let Some(name) = s.call {
                let args = s.args.iter().map(js).collect::<Result<Vec<_>, _>>()?;
                host::resolved(invoke(&self.app, &name, &args)?).await?;
            }
            if let Some(ms) = s.after {
                host::sleep(ms).await?;
            }
        }
        js(&json!({"ok":true}))
    }
}
