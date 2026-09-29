use rayon::prelude::*;
use tiny_skia::{Pixmap, PremultipliedColorU8};
/// Gaussian blur, by three successive box blurs.
///
/// **Three boxes, not a true Gaussian kernel**, and that is the specified
/// algorithm rather than a shortcut: SVG's `feGaussianBlur` — which is what
/// `shadowBlur` and CSS `blur()` are both defined in terms of — says outright
/// that three box blurs approximate a Gaussian closely enough, and gives this
/// box size for a given standard deviation. A real Gaussian convolution would
/// be slower and no more correct.
///
/// Operates on PREMULTIPLIED pixels, which is why it can average the four
/// channels alike. Blurring un-premultiplied colour bleeds the RGB of fully
/// transparent pixels into visible ones and haloes every soft edge.
pub fn blur_pixmap(pixmap: &mut Pixmap, std_dev: f32) {
    if std_dev <= 0.0 || !std_dev.is_finite() {
        return;
    }
    // SVG's own formula for the box width that approximates `std_dev`.
    let box_size = (std_dev * 3.0 * (2.0 * std::f32::consts::PI).sqrt() / 4.0 + 0.5).floor();
    let radius = (box_size as i32 / 2).max(1);

    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    if w == 0 || h == 0 {
        return;
    }
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0;
    let mut max_y = 0;
    for (i, pixel) in pixmap.pixels().iter().enumerate() {
        if pixel.alpha() != 0 {
            let x = i % w;
            let y = i / w;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x + 1);
            max_y = max_y.max(y + 1);
        }
    }
    if min_x == w {
        return;
    }
    // Three box passes can spread each pixel by at most three radii. Cropping
    // transparent tile margins before allocating channel planes preserves the
    // exact filter result while avoiding a full-tile convolution for small art.
    let spread = (radius as usize).saturating_mul(3);
    let left = min_x.saturating_sub(spread);
    let top = min_y.saturating_sub(spread);
    let right = max_x.saturating_add(spread).min(w);
    let bottom = max_y.saturating_add(spread).min(h);
    let crop_w = right - left;
    let crop_h = bottom - top;
    if crop_w.saturating_mul(crop_h) < w.saturating_mul(h) * 3 / 4 {
        if let Some(mut cropped) = Pixmap::new(crop_w as u32, crop_h as u32) {
            for y in 0..crop_h {
                let src = ((top + y) * w + left) * 4;
                let dst = y * crop_w * 4;
                cropped.data_mut()[dst..dst + crop_w * 4]
                    .copy_from_slice(&pixmap.data()[src..src + crop_w * 4]);
            }
            blur_pixmap_full(&mut cropped, radius);
            pixmap.fill(tiny_skia::Color::TRANSPARENT);
            for y in 0..crop_h {
                let src = y * crop_w * 4;
                let dst = ((top + y) * w + left) * 4;
                pixmap.data_mut()[dst..dst + crop_w * 4]
                    .copy_from_slice(&cropped.data()[src..src + crop_w * 4]);
            }
            return;
        }
    }
    blur_pixmap_full(pixmap, radius);
}

fn blur_pixmap_full(pixmap: &mut Pixmap, radius: i32) {
    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    // Work in u32 channel planes: repeated averaging on u8 loses a level each
    // pass, and three passes of that is visible banding on a soft shadow.
    let mut channels = to_planes(pixmap);
    channels.par_iter_mut().for_each(|plane| {
        // A zero channel remains zero under convolution. Black shadows only
        // need the alpha plane, not three scratch buffers and eighteen RGB passes.
        if plane.iter().all(|&value| value == 0) {
            return;
        }
        let mut scratch = vec![0u32; w * h];
        for _ in 0..3 {
            box_blur_horizontal(plane, &mut scratch, w, h, radius);
            box_blur_vertical(&mut scratch, plane, w, h, radius);
        }
    });
    from_planes(pixmap, &channels);
}

fn to_planes(pixmap: &Pixmap) -> [Vec<u32>; 4] {
    let pixels = pixmap.pixels();
    let mut planes = [
        vec![0u32; pixels.len()],
        vec![0u32; pixels.len()],
        vec![0u32; pixels.len()],
        vec![0u32; pixels.len()],
    ];
    for (i, px) in pixels.iter().enumerate() {
        planes[0][i] = px.red() as u32;
        planes[1][i] = px.green() as u32;
        planes[2][i] = px.blue() as u32;
        planes[3][i] = px.alpha() as u32;
    }
    planes
}

fn from_planes(pixmap: &mut Pixmap, planes: &[Vec<u32>; 4]) {
    for (i, px) in pixmap.pixels_mut().iter_mut().enumerate() {
        let a = planes[3][i].min(255) as u8;
        // Averaging the planes independently can leave a channel above its own
        // alpha, which is not a representable premultiplied pixel. Clamping to
        // alpha is what keeps the result valid.
        let r = planes[0][i].min(planes[3][i]).min(255) as u8;
        let g = planes[1][i].min(planes[3][i]).min(255) as u8;
        let b = planes[2][i].min(planes[3][i]).min(255) as u8;
        if let Some(p) = PremultipliedColorU8::from_rgba(r, g, b, a) {
            *px = p;
        }
    }
}

/// One box-blur pass along x, using a running sum so the cost is independent of
/// the radius.
fn box_blur_horizontal(src: &[u32], dst: &mut [u32], w: usize, h: usize, radius: i32) {
    let span = (radius * 2 + 1) as u32;
    for y in 0..h {
        let row = y * w;
        // Seed the window at x = 0, with the left half clamped to the edge
        // pixel — the same edge handling `feGaussianBlur` uses (`duplicate`).
        let mut sum: u32 = 0;
        for k in -radius..=radius {
            let x = k.clamp(0, w as i32 - 1) as usize;
            sum += src[row + x];
        }
        for x in 0..w {
            dst[row + x] = sum / span;
            let leaving = (x as i32 - radius).clamp(0, w as i32 - 1) as usize;
            let entering = (x as i32 + radius + 1).clamp(0, w as i32 - 1) as usize;
            sum = sum + src[row + entering] - src[row + leaving];
        }
    }
}

/// The same pass along y. Separable: a 2D Gaussian is the product of two 1D
/// ones, so two passes give the 2D result for a fraction of the work.
fn box_blur_vertical(src: &[u32], dst: &mut [u32], w: usize, h: usize, radius: i32) {
    let span = (radius * 2 + 1) as u32;
    for x in 0..w {
        let mut sum: u32 = 0;
        for k in -radius..=radius {
            let y = k.clamp(0, h as i32 - 1) as usize;
            sum += src[y * w + x];
        }
        for y in 0..h {
            dst[y * w + x] = sum / span;
            let leaving = (y as i32 - radius).clamp(0, h as i32 - 1) as usize;
            let entering = (y as i32 + radius + 1).clamp(0, h as i32 - 1) as usize;
            sum = sum + src[entering * w + x] - src[leaving * w + x];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zero_channel_blur_matches_unconditional_convolution() {
        for color in [(0, 0, 0, 140), (255, 0, 0, 190), (0, 80, 210, 255)] {
            let mut actual = Pixmap::new(41, 41).unwrap();
            let mut paint = tiny_skia::Paint::default();
            paint.set_color_rgba8(color.0, color.1, color.2, color.3);
            actual.fill_rect(
                tiny_skia::Rect::from_xywh(10.0, 10.0, 20.0, 20.0).unwrap(),
                &paint,
                tiny_skia::Transform::identity(),
                None,
            );
            let mut expected = actual.clone();
            let mut channels = to_planes(&expected);
            for plane in &mut channels {
                let mut scratch = vec![0; 41 * 41];
                for _ in 0..3 {
                    box_blur_horizontal(plane, &mut scratch, 41, 41, 3);
                    box_blur_vertical(&mut scratch, plane, 41, 41, 3);
                }
            }
            from_planes(&mut expected, &channels);
            blur_pixmap_full(&mut actual, 3);
            assert_eq!(actual.data(), expected.data());
        }
    }

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
    fn a_blur_spreads_alpha_outside_the_original_shape() {
        let mut p = opaque_square();
        assert_eq!(p.pixel(5, 20).expect("in bounds").alpha(), 0, "clear first");
        blur_pixmap(&mut p, 4.0);
        assert!(
            p.pixel(10, 20).expect("in bounds").alpha() > 0,
            "alpha reached outside the square"
        );
        assert!(
            p.pixel(20, 20).expect("in bounds").alpha() < 255,
            "and the middle softened"
        );
    }

    #[test]
    fn a_blur_conserves_roughly_the_total_alpha() {
        // A blur redistributes coverage, it does not create or destroy it.
        // Getting this wrong is how a blurred shadow comes out too faint.
        let before: u32 = opaque_square()
            .pixels()
            .iter()
            .map(|px| px.alpha() as u32)
            .sum();
        let mut p = opaque_square();
        blur_pixmap(&mut p, 3.0);
        let after: u32 = p.pixels().iter().map(|px| px.alpha() as u32).sum();
        let drift = (before as f32 - after as f32).abs() / before as f32;
        assert!(drift < 0.15, "before {before}, after {after}");
    }

    #[test]
    fn a_zero_blur_changes_nothing() {
        let mut p = opaque_square();
        let before: Vec<u8> = p.data().to_vec();
        blur_pixmap(&mut p, 0.0);
        assert_eq!(p.data(), before.as_slice());
    }

    #[test]
    fn cropped_blur_matches_full_tile_at_edges_and_center() {
        for (x, y) in [(0.0, 0.0), (61.0, 61.0), (119.0, 119.0)] {
            let mut cropped = Pixmap::new(128, 128).unwrap();
            let mut paint = tiny_skia::Paint::default();
            paint.set_color(tiny_skia::Color::from_rgba8(120, 40, 220, 200));
            cropped.fill_rect(
                tiny_skia::Rect::from_xywh(x, y, 8.0, 8.0).unwrap(),
                &paint,
                tiny_skia::Transform::identity(),
                None,
            );
            let mut full = cropped.clone();
            for std_dev in [1.0, 3.0, 8.0] {
                let radius = (((std_dev * 3.0 * (2.0 * std::f32::consts::PI).sqrt() / 4.0 + 0.5)
                    .floor() as i32)
                    / 2)
                .max(1);
                blur_pixmap_full(&mut full, radius);
                blur_pixmap(&mut cropped, std_dev);
                assert_eq!(cropped.data(), full.data(), "x={x}, y={y}, blur={std_dev}");
            }
        }
    }
}
