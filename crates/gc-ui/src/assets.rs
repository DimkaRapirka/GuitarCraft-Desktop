//! Normalize the transparent margins of the existing hardware artwork for the CSS skins.
use crate::dom::{doc, style};
use js_sys::Promise;
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Blob, CanvasRenderingContext2d, HtmlCanvasElement, HtmlImageElement, Url};

pub(crate) fn canvas(
    w: u32,
    h: u32,
) -> Result<(HtmlCanvasElement, CanvasRenderingContext2d), JsValue> {
    let c: HtmlCanvasElement = doc().create_element("canvas")?.dyn_into()?;
    c.set_width(w);
    c.set_height(h);
    let context = c
        .get_context("2d")?
        .ok_or_else(|| js_sys::Error::new("Canvas 2D недоступен"))?
        .dyn_into()?;
    Ok((c, context))
}
fn bounds(pixels: &[u8], w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            if pixels[((y as usize * w as usize + x as usize) * 4) + 3] > 24 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    (x1 > x0 && y1 > y0).then(|| (x0, y0, x1 - x0 + 2, y1 - y0 + 2))
}
async fn trim(path: String) -> Result<(String, u32, u32), JsValue> {
    let img = HtmlImageElement::new()?;
    img.set_src(&path);
    JsFuture::from(img.decode()).await?;
    let (w, h) = (img.natural_width(), img.natural_height());
    let (src, ctx) = canvas(w, h)?;
    ctx.draw_image_with_html_image_element(&img, 0., 0.)?;
    let data = ctx.get_image_data(0., 0., w.into(), h.into())?.data();
    let Some((x, y, cw, ch)) = bounds(&data.0, w, h) else {
        return Ok((path, w, h));
    };
    let (out, ctx) = canvas(cw, ch)?;
    ctx.draw_image_with_html_canvas_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
        &src,
        x.into(),
        y.into(),
        cw.into(),
        ch.into(),
        0.,
        0.,
        cw.into(),
        ch.into(),
    )?;
    let p = Promise::new(&mut |resolve, reject| {
        if let Err(e) = out.to_blob(&resolve) {
            let _ = reject.call1(&JsValue::UNDEFINED, &e);
        }
    });
    let blob: Blob = JsFuture::from(p).await?.dyn_into()?;
    Ok((Url::create_object_url_with_blob(&blob)?, cw, ch))
}
#[wasm_bindgen(js_name=prepareAssets)]
pub async fn prepare_assets() -> Result<(), JsValue> {
    let root = doc().document_element().unwrap();
    let body = doc().body().unwrap();
    for (key, name) in [
        ("enclosure", "pedal_enclosure"),
        ("footswitch", "footswitch"),
        ("knob-pedal", "knob_pedal"),
        ("knob-amp", "knob_amp"),
        ("mic", "mic_head"),
        ("mic-dyn57", "mic_dyn57"),
        ("mic-ribbon121", "mic_ribbon121"),
        ("mic-cond414", "mic_cond414"),
        ("mic-dyn421", "mic_dyn421"),
    ] {
        let original = format!("assets/gear/{name}.png");
        match trim(original.clone()).await {
            Ok((url, w, h)) => {
                style(&root, &format!("--img-{key}"), &format!("url({url})"));
                if key == "enclosure" {
                    style(&root, "--enc-ar", &format!("{w} / {h}"));
                    body.class_list().add_1("real-gear")?;
                }
            }
            Err(e) => {
                style(&root, &format!("--img-{key}"), &format!("url({original})"));
                web_sys::console::warn_1(&e);
            }
        }
    }
    for (key, file) in [
        ("studio", "bg_studio"),
        ("board", "bg_pedalboard"),
        ("speaker", "cab_speaker"),
        ("rack", "rack_unit"),
    ] {
        style(
            &root,
            &format!("--img-{key}"),
            &format!("url(assets/gear/{file}.png)"),
        );
    }
    Ok(())
}
