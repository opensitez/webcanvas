//! Draw into a host-owned bitmap and export it as a PNG.

use std::{env, error::Error};

use webcanvas::canvas::{CanvasSurfaces, Color, Paint};

fn main() -> Result<(), Box<dyn Error>> {
    const WIDTH: u32 = 640;
    const HEIGHT: u32 = 360;
    const SURFACE_ID: u32 = 1;

    let output = env::args().nth(1).unwrap_or_else(|| "canvas.png".into());
    let mut pixels = vec![0; (WIDTH * HEIGHT * 4) as usize];
    let mut surfaces = CanvasSurfaces::default();

    surfaces
        .with_context(SURFACE_ID, &mut pixels, WIDTH, HEIGHT, |ctx| {
            ctx.set_fill_color(Color::rgb(245, 248, 250));
            ctx.fill_rect(0.0, 0.0, WIDTH as f32, HEIGHT as f32);

            let mut sky = ctx.create_linear_gradient(0.0, 0.0, 0.0, HEIGHT as f32);
            sky.add_color_stop(0.0, Color::rgb(25, 95, 142)).unwrap();
            sky.add_color_stop(1.0, Color::rgb(88, 188, 201)).unwrap();
            ctx.set_fill_paint(&Paint::Gradient(sky));
            ctx.fill_rect(24.0, 24.0, 592.0, 312.0);

            ctx.set_fill_color(Color::rgba(255, 224, 135, 235));
            ctx.begin_path();
            ctx.arc(490.0, 105.0, 44.0, 0.0, std::f32::consts::TAU, false);
            ctx.fill();

            ctx.set_stroke_color(Color::rgb(255, 255, 255));
            ctx.set_line_width(5.0);
            ctx.begin_path();
            ctx.move_to(72.0, 255.0);
            ctx.line_to(190.0, 155.0);
            ctx.line_to(295.0, 230.0);
            ctx.line_to(410.0, 125.0);
            ctx.stroke();
        })
        .ok_or("invalid canvas bitmap dimensions")?;

    let size = tiny_skia::IntSize::from_wh(WIDTH, HEIGHT).ok_or("invalid size")?;
    let bitmap = tiny_skia::Pixmap::from_vec(pixels, size).ok_or("invalid bitmap")?;
    bitmap.save_png(&output)?;
    println!("Saved {output}");
    Ok(())
}
