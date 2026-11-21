//! Localized wear on painted aluminum enclosures. The patterns stay stable across renders.
use std::cell::RefCell;
use wasm_bindgen::JsValue;
use web_sys::CanvasRenderingContext2d;

const VARIANTS: usize = 6;

thread_local! {
    static TEXTURES: RefCell<[Option<String>; VARIANTS]> = const { RefCell::new([const { None }; VARIANTS]) };
}

struct Marks(u32);
impl Marks {
    fn between(&mut self, low: f64, high: f64) -> f64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        low + (high - low) * f64::from(self.0 >> 8) / f64::from(1 << 24)
    }
}

/// Shared by the board, the catalog detail and the builder preview.
pub fn texture(model: &str) -> Result<String, JsValue> {
    let hash = model.bytes().fold(2_166_136_261u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
    });
    let variant = hash as usize % VARIANTS;
    TEXTURES.with(|textures| {
        let mut textures = textures.borrow_mut();
        if let Some(url) = &textures[variant] {
            return Ok(url.clone());
        }
        let url = draw(variant as u32)?;
        textures[variant] = Some(url.clone());
        Ok(url)
    })
}

fn draw(variant: u32) -> Result<String, JsValue> {
    const WIDTH: u32 = 320;
    const HEIGHT: u32 = 514;
    let (canvas, ctx) = crate::assets::canvas(WIDTH, HEIGHT)?;
    let mut marks = Marks(0x724d_67a3 ^ (variant + 1) * 0x9e37);
    ctx.set_line_cap("round");

    // Exposed metal clusters stay along the bevel, clear of the artwork and captions.
    for (cx, cy, sx, sy) in [
        (34., 31., 8., 11.),
        (288., 63., 7., 23.),
        (25., 182., 4., 37.),
        (293., 297., 5., 32.),
        (37., 462., 11., 24.),
        (275., 484., 17., 10.),
        (174., 504., 55., 3.),
    ] {
        let x0 = cx + marks.between(-2., 2.);
        let y0 = cy + marks.between(-8., 8.);
        for _ in 0..12 {
            let x = x0 + marks.between(-sx, sx);
            let y = y0 + marks.between(-sy, sy);
            let width = marks.between(1., 4.5);
            let height = marks.between(0.8, 3.8);
            let outline = [
                (x - width * 0.35, y),
                (x, y - height * 0.7),
                (x + width * 0.75, y - height * 0.28),
                (x + width * 0.44, y + height * 0.55),
                (x - width * 0.15, y + height * 0.7),
            ];
            ctx.set_global_alpha(marks.between(0.35, 0.67));
            flake(&ctx, &outline, 0.8, "#171817");
            ctx.set_global_alpha(marks.between(0.35, 0.82));
            flake(&ctx, &outline, 0., "#bebdb7");
        }
    }

    // Two-step scratches catch the light on one side. Short groups suggest rubbing against gear.
    for (cx, cy) in [(52., 151.), (265., 211.), (53., 377.), (265., 441.)] {
        let angle = marks.between(-1.1, -0.35);
        for _ in 0..7 {
            let x = cx + marks.between(-13., 13.);
            let y = cy + marks.between(-22., 22.);
            let velocity = marks.between(3., 20.);
            let dx = velocity * angle.cos();
            let dy = velocity * angle.sin();
            let width = marks.between(0.65, 1.35);
            ctx.set_line_width(width + 0.35);
            ctx.set_global_alpha(marks.between(0.18, 0.43));
            stroke(&ctx, x, y + 0.8, dx, dy, "#191815");
            ctx.set_line_width(width);
            ctx.set_global_alpha(marks.between(0.24, 0.66));
            stroke(&ctx, x, y, dx, dy, "#dedbd0");
        }
    }

    // Heel marks collect around the switch and stop short of the printed name.
    for _ in 0..31 {
        let x = marks.between(102., 222.);
        let y = marks.between(467., 490.);
        ctx.set_line_width(marks.between(0.7, 2.1));
        ctx.set_global_alpha(marks.between(0.045, 0.15));
        stroke(
            &ctx,
            x,
            y,
            marks.between(-9., 12.),
            marks.between(-2., 3.),
            "#241e16",
        );
    }

    canvas.to_data_url()
}

fn flake(ctx: &CanvasRenderingContext2d, outline: &[(f64, f64)], offset: f64, color: &str) {
    ctx.set_fill_style_str(color);
    ctx.begin_path();
    ctx.move_to(outline[0].0, outline[0].1 + offset);
    for &(x, y) in &outline[1..] {
        ctx.line_to(x, y + offset);
    }
    ctx.close_path();
    ctx.fill();
}

fn stroke(ctx: &CanvasRenderingContext2d, x: f64, y: f64, dx: f64, dy: f64, color: &str) {
    ctx.set_stroke_style_str(color);
    ctx.begin_path();
    ctx.move_to(x, y);
    ctx.quadratic_curve_to(x + dx * 0.62, y + dy * 0.45, x + dx, y + dy);
    ctx.stroke();
}
