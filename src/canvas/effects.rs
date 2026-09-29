//! Shadow and filter — the two things a drawing passes through before it lands.
//!
//! HTML §4.12.5.1.13 defines one drawing model for every canvas operation, and
//! it is not "paint the shape". The shape is rendered to its own bitmap, that
//! bitmap is FILTERED, a shadow is derived from the filtered bitmap's alpha,
//! and only then is the pair composited onto the canvas. Doing it any other way
//! gets observable things wrong — a shadow of an already-shadowed shape, a
//! filter that misses the shadow, a `globalAlpha` applied twice.
//!
//! ## Why the blur lives here
//!
//! webcore had no blur at all. `display_list_replay.rs` says
//! `0 => {} // blur — needs convolution, skipped`, and `PaintCmd::BoxShadow`
//! and `PaintCmd::TextShadow` both destructure `blur: _`. So `shadowBlur`,
//! `filter: blur()`, CSS `box-shadow` and CSS `text-shadow` were four features
//! waiting on one missing primitive. [`blur_pixmap`] is that primitive; the
//! canvas uses it here, and the three renderer sites are now one call away from
//! using it too.

use tiny_skia::{Pixmap, PremultipliedColorU8};

use super::{Color, filters::{CssFilters, FilterOp, apply_color_matrix}};

pub use super::blur::blur_pixmap;

/// Replace every pixel's colour with `color`, keeping the shape's own alpha.
///
/// This is what makes a shadow a SHADOW rather than a copy: the spec derives it
/// from the alpha channel of the drawing alone, so a multicoloured shape casts
/// a single-coloured one.
pub fn tint_to(pixmap: &mut Pixmap, color: Color) {
    let alpha_scale = color.a as u32;
    for px in pixmap.pixels_mut().iter_mut() {
        let a = (px.alpha() as u32 * alpha_scale / 255).min(255);
        if a == 0 {
            *px = PremultipliedColorU8::from_rgba(0, 0, 0, 0).expect("transparent is valid");
            continue;
        }
        // Premultiplied by the combined alpha, so the channels stay <= alpha.
        let r = (color.r as u32 * a / 255) as u8;
        let g = (color.g as u32 * a / 255) as u8;
        let b = (color.b as u32 * a / 255) as u8;
        if let Some(p) = PremultipliedColorU8::from_rgba(r, g, b, a as u8) {
            *px = p;
        }
    }
}

/// Apply a parsed CSS filter list to a pixmap, in order.
///
/// Order matters and is the author's: `blur(2px) brightness(2)` is not
/// `brightness(2) blur(2px)`, because the second brightens what the blur
/// already averaged.
pub fn apply_filter_list(pixmap: &mut Pixmap, filters: &CssFilters) {
    for op in &filters.ops {
        apply_filter_op(pixmap, op);
    }
}

/// One filter function.
///
/// Colour-matrix and spatial filters share the same drawing-model pipeline.
pub fn apply_filter_op(pixmap: &mut Pixmap, op: &FilterOp) {
    match op {
        // CSS `blur(r)` names the standard deviation directly, unlike
        // `shadowBlur`, which names twice it.
        FilterOp::Blur(radius) => blur_pixmap(pixmap, *radius),
        FilterOp::DropShadow {
            dx,
            dy,
            blur,
            color,
        } => drop_shadow(pixmap, *dx, *dy, *blur, *color),
        other => apply_color_matrix(pixmap, other),
    }
}

/// `drop-shadow(dx dy blur color)` — a shadow of the pixmap, drawn beneath it.
///
/// Unlike `shadowBlur`, CSS names the standard deviation directly here, so the
/// radius is passed through rather than halved.
pub fn drop_shadow(pixmap: &mut Pixmap, dx: f32, dy: f32, blur: f32, color: Color) {
    let Some(shadow) = shadow_layer(
        pixmap,
        color,
        blur,
    ) else {
        return;
    };
    let Some(mut out) = Pixmap::new(pixmap.width(), pixmap.height()) else {
        return;
    };
    let paint = tiny_skia::PixmapPaint::default();
    out.draw_pixmap(
        dx.round() as i32,
        dy.round() as i32,
        shadow.as_ref(),
        &paint,
        tiny_skia::Transform::identity(),
        None,
    );
    out.draw_pixmap(
        0,
        0,
        pixmap.as_ref(),
        &paint,
        tiny_skia::Transform::identity(),
        None,
    );
    *pixmap = out;
}

/// The shadow cast by `source`: its alpha, tinted and blurred.
///
/// `std_dev` is already a standard deviation — `shadowBlur` is TWICE this, and
/// halving it is the caller's job, because CSS `drop-shadow()` and canvas
/// `shadowBlur` disagree about which of the two their argument names.
pub fn shadow_layer(source: &Pixmap, color: Color, std_dev: f32) -> Option<Pixmap> {
    let mut layer = source.to_owned();
    tint_to(&mut layer, color);
    blur_pixmap(&mut layer, std_dev);
    Some(layer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opaque_square() -> Pixmap {
        let mut p = Pixmap::new(41, 41).expect("a pixmap");
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(tiny_skia::Color::from_rgba8(255, 0, 0, 255));
        p.fill_rect(
            tiny_skia::Rect::from_xywh(15.0, 15.0, 11.0, 11.0).expect("a rect"),
            &paint,
            tiny_skia::Transform::identity(),
            None,
        );
        p
    }


    #[test]
    fn replay_blur_filter_uses_the_shared_blur_primitive() {
        let mut p = opaque_square();
        assert_eq!(p.pixel(10, 20).expect("in bounds").alpha(), 0);
        apply_filter_op(&mut p, &FilterOp::Blur(4.0));
        assert!(
            p.pixel(10, 20).expect("in bounds").alpha() > 0,
            "display-list blur should spread alpha just like canvas blur"
        );
    }

    #[test]
    fn drop_shadow_paints_offset_shadow_under_source() {
        let mut p = opaque_square();
        drop_shadow(&mut p, 10.0, 0.0, 0.0, Color::rgba(0, 0, 255, 255));

        let offset = p.pixel(30, 20).expect("in bounds");
        assert!(
            offset.blue() > 0 && offset.alpha() > 0,
            "offset shadow should be painted at the requested dx"
        );
        let source = p.pixel(20, 20).expect("in bounds");
        assert_eq!(source.red(), 255, "source must remain above the shadow");
    }

    #[test]
    fn a_blurred_pixel_stays_a_valid_premultiplied_colour() {
        // Averaging the four planes independently can push a colour channel
        // above its own alpha, which is not representable. The clamp in
        // `from_planes` is what this checks.
        let mut p = Pixmap::new(20, 20).expect("a pixmap");
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(tiny_skia::Color::from_rgba8(255, 255, 255, 40));
        p.fill_rect(
            tiny_skia::Rect::from_xywh(5.0, 5.0, 10.0, 10.0).expect("a rect"),
            &paint,
            tiny_skia::Transform::identity(),
            None,
        );
        blur_pixmap(&mut p, 2.0);
        for px in p.pixels() {
            assert!(
                px.red() <= px.alpha() && px.green() <= px.alpha() && px.blue() <= px.alpha(),
                "channel above alpha: {} {} {} / {}",
                px.red(),
                px.green(),
                px.blue(),
                px.alpha()
            );
        }
    }

    #[test]
    fn tinting_keeps_the_shape_and_replaces_the_colour() {
        let mut p = opaque_square();
        assert_eq!(p.pixel(20, 20).expect("in bounds").red(), 255, "red first");
        tint_to(&mut p, Color::rgb(0, 0, 255));
        let inside = p.pixel(20, 20).expect("in bounds");
        assert_eq!(inside.red(), 0);
        assert_eq!(inside.blue(), 255);
        assert_eq!(inside.alpha(), 255, "the shape is unchanged");
        assert_eq!(
            p.pixel(2, 2).expect("in bounds").alpha(),
            0,
            "and nothing appeared outside it"
        );
    }

    #[test]
    fn tinting_with_a_translucent_colour_scales_the_alpha() {
        let mut p = opaque_square();
        tint_to(&mut p, Color::rgba(0, 0, 255, 128));
        assert_eq!(p.pixel(20, 20).expect("in bounds").alpha(), 128);
    }
}
