use cosmic_text::{Buffer, Color, FontSystem, PhysicalGlyph, Renderer, SwashCache};
use tiny_skia::{Mask, Pixmap, PremultipliedColorU8};

/// Rasterize an already shaped text buffer into the canvas bitmap.
pub fn blit_shaped_buffer(
    pixmap: &mut Pixmap,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    buffer: &mut Buffer,
    x: f32,
    y: f32,
    color: Color,
    clip: Option<&Mask>,
) {
    struct TextRenderer<'a> {
        pixmap: &'a mut Pixmap,
        font_system: &'a mut FontSystem,
        swash_cache: &'a mut SwashCache,
        origin: (i32, i32),
        alpha: u32,
        clip: Option<&'a Mask>,
    }

    impl Renderer for TextRenderer<'_> {
        fn rectangle(&mut self, x: i32, y: i32, w: u32, h: u32, color: Color) {
            blit_rect(
                self.pixmap,
                self.origin.0 + x,
                self.origin.1 + y,
                w,
                h,
                color,
                self.alpha,
                self.clip,
            );
        }

        fn glyph(&mut self, glyph: PhysicalGlyph, color: Color) {
            let origin = self.origin;
            let alpha = self.alpha;
            let clip = self.clip;
            let pixmap = &mut self.pixmap;
            self.swash_cache.with_pixels(
                self.font_system,
                glyph.cache_key,
                color,
                |x, y, pixel_color| {
                    blit_rect(
                        pixmap,
                        origin.0 + glyph.x + x,
                        origin.1 + glyph.y + y,
                        1,
                        1,
                        pixel_color,
                        alpha,
                        clip,
                    );
                },
            );
        }
    }

    buffer.render(
        &mut TextRenderer {
            pixmap,
            font_system,
            swash_cache,
            origin: (x as i32, y as i32),
            alpha: color.a() as u32,
            clip,
        },
        color,
    );
}

#[allow(clippy::too_many_arguments)]
fn blit_rect(
    pixmap: &mut Pixmap,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    color: Color,
    alpha: u32,
    clip: Option<&Mask>,
) {
    let source_alpha = color.a() as u32 * alpha / 255;
    if source_alpha == 0 {
        return;
    }
    let bitmap_width = pixmap.width() as i32;
    let bitmap_height = pixmap.height() as i32;
    let mask = clip.map(Mask::data);
    let pixels = pixmap.pixels_mut();
    for dy in 0..height as i32 {
        let py = y + dy;
        if py < 0 || py >= bitmap_height {
            continue;
        }
        for dx in 0..width as i32 {
            let px = x + dx;
            if px < 0 || px >= bitmap_width {
                continue;
            }
            let index = py as usize * bitmap_width as usize + px as usize;
            let a = source_alpha * mask.map_or(255, |m| m[index] as u32) / 255;
            if a == 0 {
                continue;
            }
            let inverse = 255 - a;
            let dest = &mut pixels[index];
            let r = (color.r() as u32 * a + dest.red() as u32 * inverse) / 255;
            let g = (color.g() as u32 * a + dest.green() as u32 * inverse) / 255;
            let b = (color.b() as u32 * a + dest.blue() as u32 * inverse) / 255;
            let out_a = a + dest.alpha() as u32 * inverse / 255;
            if let Some(pixel) = PremultipliedColorU8::from_rgba(r as u8, g as u8, b as u8, out_a as u8) {
                *dest = pixel;
            }
        }
    }
}
