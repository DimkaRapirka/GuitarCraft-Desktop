//! Combo designer: amp fronts drawn from a few choices (format, tolex, grille cloth, control
//! panel, piping, logo, corners) instead of a picture. A designed skin is an ordinary user skin
//! with a `design` object; its picture is drawn on a canvas when it is shown.
//! The designer page is not offered in the skin picker for now (kept for later); saved designs still render.
#![allow(dead_code)]
use crate::{
    assets::canvas,
    dom::*,
    editor::{attr, button, on, report},
};
use serde_json::{json, Value};
use std::{cell::RefCell, collections::HashMap, f64::consts::PI};
use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d as Ctx, Element};

thread_local! {
    static CACHE: RefCell<HashMap<String, String>> = RefCell::default();
    static DRAFT: RefCell<Value> = RefCell::new(Value::Null);
    static THUMBS: RefCell<HashMap<String, String>> = RefCell::default();
    static IDS: RefCell<HashMap<String, Value>> = RefCell::default();
    static TEX: RefCell<HashMap<String, web_sys::HtmlImageElement>> = RefCell::default();
}

pub(crate) const FORMATS: [(&str, &str); 4] = [
    ("1x12", "Комбо 1×12"),
    ("2x12", "Комбо 2×12"),
    ("1x10", "Мини 1×10"),
    ("head", "Голова"),
];
const TEXTURES: [(&str, &str); 3] = [("levant", "Левант"), ("tweed", "Твид"), ("smooth", "Гладкий")];
const TOLEX: [&str; 10] = [
    "#1b1a19", "#2d2a26", "#5a3a22", "#7a1f1f", "#1f3a5a", "#2f4a2e", "#d9c79c", "#e9e3d3", "#c9a227", "#6b2d5c",
];
const GRILLES: [(&str, &str); 6] = [
    ("oxblood", "Оксблад"),
    ("salt", "Соль и перец"),
    ("silver", "Серебро"),
    ("black", "Чёрная"),
    ("wheat", "Пшеничная"),
    ("brown", "Коричневая"),
];
const PANELS: [(&str, &str); 4] = [("chrome", "Хром"), ("gold", "Золото"), ("black", "Чёрная"), ("cream", "Кремовая")];
const PIPING: [(&str, &str); 4] = [("white", "Белый"), ("gold", "Золотой"), ("black", "Чёрный"), ("none", "Без канта")];
const LOGOS: [(&str, &str); 3] = [("script", "Рукописный"), ("block", "Рубленый"), ("badge", "Хром-шильд")];
const RINGS: [&str; 6] = ["#ea834d", "#f2a541", "#d64545", "#5fb4e8", "#9be07a", "#b07cff"];

pub(crate) fn default_design() -> Value {
    json!({"format":"1x12","tolex":"#1b1a19","texture":"levant","grille":"silver","panel":"chrome","piping":"white",
           "logo":"GuitarCraft","logoStyle":"script","corners":true,"ring":"#ea834d"})
}
fn s<'a>(d: &'a Value, k: &str, def: &'a str) -> &'a str {
    d[k].as_str().unwrap_or(def)
}

struct Geo {
    w: f64,
    h: f64,
    panel: (f64, f64),         // top and height, parts of h
    grille: (f64, f64, f64, f64), // x, y, w, h, parts
    speakers: Vec<(f64, f64, f64)>, // centre x, y (parts) and radius (part of w)
    knob: f64,
}
fn geo(format: &str) -> Geo {
    let w = 1600.;
    match format {
        "2x12" => Geo { w, h: w / 1.58, panel: (0.105, 0.165), grille: (0.05, 0.315, 0.9, 0.615), speakers: vec![(0.3, 0.61, 0.15), (0.7, 0.61, 0.15)], knob: 0.05 },
        "1x10" => Geo { w, h: w / 1.08, panel: (0.1, 0.155), grille: (0.07, 0.3, 0.86, 0.63), speakers: vec![(0.5, 0.6, 0.27)], knob: 0.066 },
        "head" => Geo { w, h: w / 2.4, panel: (0.52, 0.34), grille: (0.05, 0.17, 0.9, 0.28), speakers: vec![], knob: 0.048 },
        _ => Geo { w, h: w / 1.26, panel: (0.1, 0.16), grille: (0.06, 0.3, 0.88, 0.63), speakers: vec![(0.5, 0.605, 0.27)], knob: 0.058 },
    }
}
fn rng(seed: &str) -> impl FnMut() -> f64 {
    let mut x: u64 = seed.bytes().fold(1469598103934665603, |h, b| (h ^ b as u64).wrapping_mul(1099511628211)) | 1;
    move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
}
const TEXTURES_ALL: [&str; 10] = [
    "tolex_levant", "tolex_tweed", "tolex_smooth", "grille_oxblood", "grille_salt", "grille_silver",
    "grille_black", "grille_wheat", "grille_brown", "panel_brushed",
];
/// Load the photographed materials once; designs drawn before that use the procedural ones.
pub(crate) async fn preload() {
    for name in TEXTURES_ALL {
        if TEX.with(|t| t.borrow().contains_key(name)) {
            continue;
        }
        let Ok(img) = web_sys::HtmlImageElement::new() else { return };
        img.set_src(&format!("assets/gear/textures/{name}.png"));
        if wasm_bindgen_futures::JsFuture::from(img.decode()).await.is_ok() {
            TEX.with(|t| t.borrow_mut().insert(name.to_owned(), img));
        }
    }
}
fn pattern(c: &Ctx, name: &str) -> bool {
    let Some(img) = TEX.with(|t| t.borrow().get(name).cloned()) else { return false };
    match c.create_pattern_with_html_image_element(&img, "repeat") {
        Ok(Some(p)) => {
            c.set_fill_style_canvas_pattern(&p);
            true
        }
        _ => false,
    }
}
fn rrect(c: &Ctx, x: f64, y: f64, w: f64, h: f64, r: f64) {
    c.begin_path();
    c.move_to(x + r, y);
    c.arc_to(x + w, y, x + w, y + h, r).ok();
    c.arc_to(x + w, y + h, x, y + h, r).ok();
    c.arc_to(x, y + h, x, y, r).ok();
    c.arc_to(x, y, x + w, y, r).ok();
    c.close_path();
}
fn vgrad(c: &Ctx, y0: f64, y1: f64, stops: &[(f32, &str)]) {
    let g = c.create_linear_gradient(0., y0, 0., y1);
    for (o, col) in stops {
        let _ = g.add_color_stop(*o, col);
    }
    c.set_fill_style_canvas_gradient(&g);
}
fn tolex(c: &Ctx, d: &Value, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let base = s(d, "tolex", "#1b1a19");
    rrect(c, x, y, w, h, r);
    c.set_fill_style_str(base);
    c.fill();
    c.save();
    rrect(c, x, y, w, h, r);
    c.clip();
    let mut rnd = rng(&d.to_string());
    let texture = s(d, "texture", "levant");
    if texture == "tweed" && pattern(c, "tolex_tweed") {
        c.fill_rect(x, y, w, h);
        c.set_global_composite_operation("multiply").ok();
        c.set_global_alpha(0.35);
        c.set_fill_style_str(base);
        c.fill_rect(x, y, w, h);
        c.set_global_alpha(1.);
        c.set_global_composite_operation("source-over").ok();
    } else if pattern(c, if texture == "smooth" { "tolex_smooth" } else { "tolex_levant" }) {
        c.set_global_composite_operation("overlay").ok();
        c.fill_rect(x, y, w, h);
        c.set_global_composite_operation("source-over").ok();
    } else {
    match texture {
        "tweed" => {
            c.set_line_width(3.);
            let mut i = -h;
            while i < w {
                c.set_stroke_style_str(if (i as i64 / 9) % 2 == 0 { "rgba(70,45,20,.28)" } else { "rgba(255,240,200,.16)" });
                c.begin_path();
                c.move_to(x + i, y + h);
                c.line_to(x + i + h, y);
                c.stroke();
                i += 9.;
            }
        }
        "smooth" => {}
        _ => {
            for _ in 0..9000 {
                let (px, py, rr) = (x + rnd() * w, y + rnd() * h, 1. + rnd() * 3.);
                c.set_fill_style_str(if rnd() > 0.5 { "rgba(255,255,255,.05)" } else { "rgba(0,0,0,.22)" });
                c.begin_path();
                let _ = c.ellipse(px, py, rr * 1.6, rr, rnd() * PI, 0., 2. * PI);
                c.fill();
            }
        }
    }
    }
    vgrad(c, y, y + h, &[(0., "rgba(255,255,255,.10)"), (0.25, "rgba(255,255,255,0)"), (0.8, "rgba(0,0,0,0)"), (1., "rgba(0,0,0,.35)")]);
    c.fill_rect(x, y, w, h);
    c.restore();
}
fn grille(c: &Ctx, d: &Value, x: f64, y: f64, w: f64, h: f64, speakers: &[(f64, f64, f64)], cw: f64, ch: f64) {
    let (base, thread, alt) = match s(d, "grille", "silver") {
        "oxblood" => ("#4a1716", "rgba(214,170,90,.35)", "rgba(0,0,0,.35)"),
        "salt" => ("#3a3836", "rgba(235,230,220,.35)", "rgba(0,0,0,.4)"),
        "black" => ("#151515", "rgba(140,140,140,.22)", "rgba(0,0,0,.5)"),
        "wheat" => ("#b9a172", "rgba(255,240,200,.35)", "rgba(80,55,20,.35)"),
        "brown" => ("#4a3424", "rgba(200,160,110,.3)", "rgba(0,0,0,.35)"),
        _ => ("#a9a398", "rgba(255,255,255,.35)", "rgba(40,35,30,.35)"),
    };
    c.save();
    rrect(c, x, y, w, h, 10.);
    c.clip();
    let real = pattern(c, &format!("grille_{}", s(d, "grille", "silver")));
    if !real {
        c.set_fill_style_str(base);
    }
    c.fill_rect(x, y, w, h);
    for &(sx, sy, r) in speakers {
        let (px, py, pr) = (sx * cw, sy * ch, r * cw);
        if let Ok(g) = c.create_radial_gradient(px, py, pr * 0.15, px, py, pr * 1.05) {
            let _ = g.add_color_stop(0., "rgba(0,0,0,.55)");
            let _ = g.add_color_stop(0.75, "rgba(0,0,0,.35)");
            let _ = g.add_color_stop(1., "rgba(0,0,0,0)");
            c.set_fill_style_canvas_gradient(&g);
            c.fill_rect(px - pr * 1.1, py - pr * 1.1, pr * 2.2, pr * 2.2);
        }
    }
    let weave = s(d, "grille", "silver") == "wheat" && !real;
    if real {
        vgrad(c, y, y + h, &[(0., "rgba(0,0,0,.35)"), (0.12, "rgba(0,0,0,0)"), (0.85, "rgba(0,0,0,0)"), (1., "rgba(0,0,0,.3)")]);
        c.fill_rect(x, y, w, h);
        c.restore();
        return;
    }
    c.set_line_width(1.6);
    let mut i = 0.;
    while i < w.max(h) {
        let k = (i / 5.) as i64;
        c.set_stroke_style_str(if k % 2 == 0 { thread } else { alt });
        if i < w {
            c.begin_path();
            c.move_to(x + i, y);
            c.line_to(x + i, y + h);
            c.stroke();
        }
        if i < h {
            c.begin_path();
            c.move_to(x, y + i);
            c.line_to(x + w, y + i);
            c.stroke();
        }
        i += 5.;
    }
    if weave {
        c.set_fill_style_str("rgba(60,40,10,.18)");
        let mut yy = 0.;
        while yy < h {
            let mut xx = if (yy / 24.) as i64 % 2 == 0 { 0. } else { 24. };
            while xx < w {
                c.fill_rect(x + xx, y + yy, 24., 24.);
                xx += 48.;
            }
            yy += 24.;
        }
    }
    if s(d, "grille", "silver") == "salt" {
        let mut rnd = rng("salt");
        for _ in 0..5000 {
            c.set_fill_style_str(if rnd() > 0.5 { "rgba(255,255,255,.35)" } else { "rgba(0,0,0,.4)" });
            c.fill_rect(x + rnd() * w, y + rnd() * h, 2., 2.);
        }
    }
    vgrad(c, y, y + h, &[(0., "rgba(0,0,0,.35)"), (0.12, "rgba(0,0,0,0)"), (0.85, "rgba(0,0,0,0)"), (1., "rgba(0,0,0,.3)")]);
    c.fill_rect(x, y, w, h);
    c.restore();
}
fn panel(c: &Ctx, d: &Value, x: f64, y: f64, w: f64, h: f64) {
    let stops: &[(f32, &str)] = match s(d, "panel", "chrome") {
        "gold" => &[(0., "#f3dc9a"), (0.45, "#c9a24c"), (0.55, "#b38a36"), (1., "#e9cf86")],
        "black" => &[(0., "#2b2b2d"), (0.5, "#141415"), (1., "#232325")],
        "cream" => &[(0., "#f2ead6"), (0.5, "#e2d6b8"), (1., "#efe5cd")],
        _ => &[(0., "#f4f4f4"), (0.45, "#b9b9bb"), (0.55, "#9c9ca0"), (1., "#e6e6e8")],
    };
    rrect(c, x, y, w, h, 8.);
    vgrad(c, y, y + h, stops);
    c.fill();
    c.save();
    rrect(c, x, y, w, h, 8.);
    c.clip();
    if pattern(c, "panel_brushed") {
        c.set_global_composite_operation(if s(d, "panel", "chrome") == "black" { "screen" } else { "multiply" }).ok();
        c.set_global_alpha(if s(d, "panel", "chrome") == "black" { 0.18 } else { 0.85 });
        c.fill_rect(x, y, w, h);
        c.set_global_alpha(1.);
        c.set_global_composite_operation("source-over").ok();
    }
    let mut rnd = rng("brushed");
    for i in 0..(h as i64) {
        if i % 2 == 0 {
            c.set_fill_style_str(if rnd() > 0.5 { "rgba(255,255,255,.08)" } else { "rgba(0,0,0,.06)" });
            c.fill_rect(x, y + i as f64, w, 1.);
        }
    }
    c.restore();
    c.set_line_width(4.);
    c.set_stroke_style_str("rgba(0,0,0,.55)");
    rrect(c, x, y, w, h, 8.);
    c.stroke();
}
fn piping(c: &Ctx, d: &Value, x: f64, y: f64, w: f64, h: f64) {
    let col = match s(d, "piping", "white") {
        "gold" => "#d4af37",
        "black" => "#0b0b0b",
        "none" => return,
        _ => "#efe8d8",
    };
    c.set_line_width(7.);
    c.set_stroke_style_str("rgba(0,0,0,.5)");
    rrect(c, x - 5., y - 5., w + 10., h + 10., 14.);
    c.stroke();
    c.set_line_width(5.);
    c.set_stroke_style_str(col);
    rrect(c, x - 5., y - 6., w + 10., h + 10., 14.);
    c.stroke();
}
fn logo(c: &Ctx, d: &Value, x: f64, y: f64, size: f64, centre: bool) {
    let text = s(d, "logo", "GuitarCraft");
    if text.trim().is_empty() {
        return;
    }
    let style = s(d, "logoStyle", "script");
    let font = match style {
        "block" | "badge" => format!("900 {size}px 'Arial Black', 'Helvetica Neue', sans-serif"),
        _ => format!("italic 700 {}px 'Snell Roundhand', 'Brush Script MT', 'Segoe Script', cursive", size * 1.25),
    };
    c.set_font(&font);
    c.set_text_align(if centre { "center" } else { "left" });
    c.set_text_baseline("middle");
    let col = match s(d, "piping", "white") {
        "gold" => "#e2c160",
        "black" => "#e8e2d4",
        _ => "#f2ece0",
    };
    if style == "badge" {
        let tw = c.measure_text(text).map(|m| m.width()).unwrap_or(size * 4.);
        let bx = if centre { x - tw / 2. - size * 0.4 } else { x - size * 0.4 };
        rrect(c, bx, y - size * 0.7, tw + size * 0.8, size * 1.4, size * 0.3);
        vgrad(c, y - size * 0.7, y + size * 0.7, &[(0., "#f6f6f6"), (0.5, "#9a9a9e"), (1., "#dedee0")]);
        c.fill();
        c.set_fill_style_str("#1a1a1a");
        let _ = c.fill_text(text, x, y + size * 0.04);
        return;
    }
    c.set_fill_style_str("rgba(0,0,0,.55)");
    let _ = c.fill_text(text, x + 3., y + 4.);
    c.set_fill_style_str(col);
    let _ = c.fill_text(text, x, y);
}
fn corner(c: &Ctx, x: f64, y: f64, r: f64, sx: f64, sy: f64) {
    c.begin_path();
    c.move_to(x, y);
    c.line_to(x + sx * r, y);
    c.arc_to(x + sx * r, y + sy * r * 0.35, x + sx * r * 0.35, y + sy * r, r * 0.4).ok();
    c.line_to(x, y + sy * r);
    c.close_path();
    vgrad(c, y.min(y + sy * r), y.max(y + sy * r), &[(0., "#e8e8ea"), (0.5, "#8d8d92"), (1., "#c9c9cc")]);
    c.fill();
    c.set_stroke_style_str("rgba(0,0,0,.6)");
    c.set_line_width(2.);
    c.stroke();
}
fn draw(d: &Value) -> Result<String, JsValue> {
    let g = geo(s(d, "format", "1x12"));
    let (w, h) = (g.w, g.h);
    let (cv, c) = canvas(w as u32, h as u32)?;
    let top = h * if g.speakers.is_empty() { 0.1 } else { 0.075 };
    // handle
    let hw = w * 0.22;
    rrect(&c, (w - hw) / 2., top - h * 0.045, hw, h * 0.07, h * 0.03);
    vgrad(&c, top - h * 0.045, top + h * 0.03, &[(0., "#3a3633"), (1., "#0d0c0b")]);
    c.fill();
    for sx in [-1., 1.] {
        rrect(&c, w / 2. + sx * hw / 2. - if sx < 0. { 0. } else { w * 0.03 }, top - h * 0.01, w * 0.03, h * 0.03, 6.);
        vgrad(&c, top - h * 0.01, top + h * 0.02, &[(0., "#ddd"), (1., "#777")]);
        c.fill();
    }
    // body
    let (bx, by, bw, bh) = (w * 0.008, top, w * 0.984, h * 0.94 - top);
    c.set_shadow_color("rgba(0,0,0,.6)");
    c.set_shadow_blur(30.);
    c.set_shadow_offset_y(18.);
    rrect(&c, bx, by, bw, bh, w * 0.02);
    c.set_fill_style_str("#000");
    c.fill();
    c.set_shadow_color("rgba(0,0,0,0)");
    tolex(&c, d, bx, by, bw, bh, w * 0.02);
    // grille, piping, logo
    let (gx, gy, gw, gh) = (g.grille.0 * w, g.grille.1 * h, g.grille.2 * w, g.grille.3 * h);
    grille(&c, d, gx, gy, gw, gh, &g.speakers, w, h);
    piping(&c, d, gx, gy, gw, gh);
    let head = g.speakers.is_empty();
    let script = s(d, "logoStyle", "script") == "script";
    if head || !script {
        logo(&c, d, gx + gw / 2., gy + gh * if head { 0.5 } else { 0.13 }, h * if head { 0.11 } else { 0.06 }, true);
    } else {
        logo(&c, d, gx + gw * 0.06, gy + gh * 0.13, h * 0.065, false);
    }
    // control panel
    panel(&c, d, w * 0.045, g.panel.0 * h, w * 0.91, g.panel.1 * h);
    if s(d, "corners", "") != "" || d["corners"] == true {
        let r = w * 0.055;
        for (x, y, sx, sy) in [(bx, by, 1., 1.), (bx + bw, by, -1., 1.), (bx, by + bh, 1., -1.), (bx + bw, by + bh, -1., -1.)] {
            corner(&c, x, y, r, sx, sy);
        }
    }
    // feet
    for fx in [0.1, 0.9] {
        rrect(&c, w * fx - w * 0.03, by + bh - 4., w * 0.06, h * 0.035, 8.);
        c.set_fill_style_str("#111");
        c.fill();
    }
    let (small, sc) = canvas(240, (240. * h / w) as u32)?;
    sc.draw_image_with_html_canvas_element_and_dw_and_dh(&cv, 0., 0., 240., 240. * h / w)?;
    THUMBS.with(|t| t.borrow_mut().insert(d.to_string(), small.to_data_url().unwrap_or_default()));
    if TEX.with(|t| t.borrow().is_empty()) {
        // drawn before the materials loaded: let the next call draw the real ones
        THUMBS.with(|t| t.borrow_mut().remove(&d.to_string()));
    }
    cv.to_data_url()
}
/// Note which user skin carries which design (for the chain thumbnail).
pub(crate) fn remember(id: &str, design: &Value) {
    IDS.with(|m| m.borrow_mut().insert(id.to_owned(), design.clone()));
}
/// Small picture of the designed skin `id`, if it is one.
pub(crate) fn thumb(id: &str) -> Option<String> {
    let d = IDS.with(|m| m.borrow().get(id).cloned())?;
    if THUMBS.with(|t| !t.borrow().contains_key(&d.to_string())) {
        image(&d).ok()?;
    }
    THUMBS.with(|t| t.borrow().get(&d.to_string()).cloned())
}
/// Picture of a designed combo (cached: the same design draws once).
pub(crate) fn image(d: &Value) -> Result<String, JsValue> {
    let key = format!("{}#{}", d, TEX.with(|t| t.borrow().len()));
    if let Some(url) = CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return Ok(url);
    }
    let url = draw(d)?;
    CACHE.with(|c| c.borrow_mut().insert(key, url.clone()));
    Ok(url)
}
/// Skin fields (knob row, sizes, colours) that go with a design.
pub(crate) fn skin(d: &Value, id: &str, name: &str) -> Value {
    let g = geo(s(d, "format", "1x12"));
    let dark = s(d, "panel", "chrome") == "black";
    json!({"id":id,"name":name,"design":d,"panel":{"x0":0.07,"x1":0.94,"y":g.panel.0 + g.panel.1 * 0.42},"knob":g.knob,
           "text":if dark {"#efe6d6"} else {"#1c1a18"},"ring":s(d,"ring","#ea834d"),"pointer":if dark {"#ffffff"} else {"#111111"}})
}

fn row(root: &Element, title: &str) -> Element {
    let r = child(root, "div", "lp-row", "");
    child(&r, "span", "lp-title", title);
    child(&r, "div", "lp-items", "")
}
fn set_key(key: &str, v: Value) {
    DRAFT.with(|d| d.borrow_mut()[key] = v);
}
fn refresh(app: &JsValue, root: &Element) -> Result<(), JsValue> {
    let content = root.clone();
    let existing = get(&content, "gcSkinId").as_string();
    page(&content, app, None, existing)
}
fn chips(app: &JsValue, root: &Element, page: &Element, key: &'static str, items: &[(&'static str, &'static str)], cur: &str) {
    for &(id, name) in items {
        let b = button(root, if cur == id { "chip on" } else { "chip" }, "", name);
        attr(&b, "data-v", id);
        let page = page.clone();
        on(&b, "click", app, move |a, _| {
            set_key(key, json!(id));
            refresh(a, &page)
        });
    }
}
fn swatches(app: &JsValue, root: &Element, page: &Element, key: &'static str, colors: &[&'static str], cur: &str) {
    for &col in colors {
        let b = button(root, if cur.eq_ignore_ascii_case(col) { "swatch on" } else { "swatch" }, "", "");
        style(&b, "--sw", col);
        attr(&b, "aria-label", col);
        let page = page.clone();
        on(&b, "click", app, move |a, _| {
            set_key(key, json!(col));
            refresh(a, &page)
        });
    }
}
/// The designer inside the skins modal. `start` resets the draft (a new design or one to edit).
pub(crate) fn page(content: &Element, app: &JsValue, start: Option<Value>, skin_id: Option<String>) -> Result<(), JsValue> {
    if let Some(d) = start {
        DRAFT.with(|x| x.replace(d));
    }
    set(content, "gcSkinId", &skin_id.clone().map(JsValue::from).unwrap_or(JsValue::UNDEFINED));
    let d = DRAFT.with(|x| x.borrow().clone());
    if TEX.with(|t| t.borrow().is_empty()) {
        let (content, app, id) = (content.clone(), app.clone(), skin_id.clone());
        wasm_bindgen_futures::spawn_local(async move {
            preload().await;
            if content.is_connected() && TEX.with(|t| !t.borrow().is_empty()) {
                report(page(&content, &app, None, id));
            }
        });
    }
    content.set_text_content(None);
    let wrap = child(content, "div", "combo-designer", "");
    let pv = child(&wrap, "div", "cd-preview", "");
    let pic = child(&pv, "div", "card-img", "");
    let img = child(&pic, "img", "", "");
    attr(&img, "src", &image(&d)?);
    let sk = skin(&d, "draft", "");
    for (id, p) in crate::skins::layout(&sk).as_object().unwrap() {
        let el = child(&pic, "i", if id == "channel" { "card-chan" } else { "card-knob" }, "");
        crate::skins::place(&el, p);
        style(&el, "width", &format!("{}%", sk["knob"].as_f64().unwrap_or(0.06) * 100.));
        if id == "channel" {
            for _ in 0..4 {
                child(&el, "b", "", "");
            }
        }
    }
    let form = child(&wrap, "div", "cd-form", "");
    let name_row = row(&form, "Название");
    let name = child(&name_row, "input", "cd-name", "");
    attr(&name, "maxlength", "32");
    set(&name, "value", &s(&d, "name", crate::i18n::text("Мой комбик")).into());
    listen(&name, "input", |e| set_key("name", json!(string(&e.current_target().unwrap(), "value"))));
    chips(app, &row(&form, "Формат"), content, "format", &FORMATS, s(&d, "format", "1x12"));
    let t = row(&form, "Тольекс");
    t.class_list().add_1("cd-tolex")?;
    swatches(app, &t, content, "tolex", &TOLEX, s(&d, "tolex", ""));
    let pick = child(&t, "input", "lp-color", "");
    attr(&pick, "type", "color");
    set(&pick, "value", &s(&d, "tolex", "#1b1a19").into());
    let page_el = content.clone();
    on(&pick, "change", app, move |a, e| {
        set_key("tolex", json!(string(&e.current_target().unwrap(), "value")));
        refresh(a, &page_el)
    });
    chips(app, &row(&form, "Фактура"), content, "texture", &TEXTURES, s(&d, "texture", "levant"));
    let grille = row(&form, "Ткань сетки");
    grille.class_list().add_1("cd-grille")?;
    chips(app, &grille, content, "grille", &GRILLES, s(&d, "grille", "silver"));
    chips(app, &row(&form, "Панель"), content, "panel", &PANELS, s(&d, "panel", "chrome"));
    chips(app, &row(&form, "Кант"), content, "piping", &PIPING, s(&d, "piping", "white"));
    let l = row(&form, "Логотип");
    let logo = child(&l, "input", "cd-logo", "");
    attr(&logo, "maxlength", "18");
    set(&logo, "value", &s(&d, "logo", "").into());
    let page_el = content.clone();
    on(&logo, "change", app, move |a, e| {
        set_key("logo", json!(string(&e.current_target().unwrap(), "value")));
        refresh(a, &page_el)
    });
    chips(app, &l, content, "logoStyle", &LOGOS, s(&d, "logoStyle", "script"));
    swatches(app, &row(&form, "Подсветка ручек"), content, "ring", &RINGS, s(&d, "ring", "#ea834d"));
    let c = row(&form, "Уголки");
    let corners = d["corners"] == true;
    let b = button(&c, if corners { "chip on" } else { "chip" }, "", if corners { "Металлические" } else { "Без уголков" });
    let page_el = content.clone();
    on(&b, "click", app, move |a, _| {
        set_key("corners", json!(!corners));
        refresh(a, &page_el)
    });
    let btns = child(&form, "div", "lp-foot", "");
    let save = button(&btns, "primary", "", if skin_id.is_some() { "Сохранить изменения" } else { "Сохранить и включить" });
    save.set_id("btn-save-combo");
    on(&save, "click", app, move |a, _| {
        let d = DRAFT.with(|x| x.borrow().clone());
        let app = a.clone();
        let id = skin_id.clone();
        wasm_bindgen_futures::spawn_local(async move {
            report(crate::skins::save_design(app, d, id).await);
        });
        Ok(())
    });
    on(&button(&btns, "", "", "← Назад"), "click", app, |a, _| {
        let a = a.clone();
        wasm_bindgen_futures::spawn_local(async move {
            report(crate::skins::open_picker(a).await);
        });
        Ok(())
    });
    Ok(())
}
