//! Compare drawing cost with retained and per-operation 2D context access.

use std::{sync::Arc, time::Instant};

use webcanvas::canvas::{CanvasSurfaces, Color, Image};

const WIDTH: u32 = 800;
const HEIGHT: u32 = 480;
const SEGMENTS: usize = 150;
const FRAMES: usize = 6;

fn segment(i: usize) -> (f32, f32, f32, f32) {
    let x = (i * 3 % 480) as f32 + 40.0;
    let y = (i * 17 % 288) as f32 + 96.0;
    (x, y, x + 8.0, y + 4.0)
}

fn main() {
    for retained in [true, false] {
        let mut pixels = vec![0; (WIDTH * HEIGHT * 4) as usize];
        let mut surfaces = CanvasSurfaces::default();
        let started = Instant::now();
        for _ in 0..FRAMES {
            if retained {
                surfaces
                    .with_context(1, &mut pixels, WIDTH, HEIGHT, |ctx| {
                        ctx.set_stroke_color(Color::rgb(86, 208, 224));
                        for i in 0..SEGMENTS {
                            let (x1, y1, x2, y2) = segment(i);
                            ctx.begin_path();
                            ctx.move_to(x1, y1);
                            ctx.line_to(x2, y2);
                            ctx.stroke();
                        }
                    })
                    .unwrap();
            } else {
                surfaces
                    .with_context(1, &mut pixels, WIDTH, HEIGHT, |ctx| {
                        ctx.set_stroke_color(Color::rgb(86, 208, 224));
                    })
                    .unwrap();
                for i in 0..SEGMENTS {
                    let (x1, y1, x2, y2) = segment(i);
                    surfaces.with_context(1, &mut pixels, WIDTH, HEIGHT, |ctx| ctx.begin_path()).unwrap();
                    surfaces.with_context(1, &mut pixels, WIDTH, HEIGHT, |ctx| ctx.move_to(x1, y1)).unwrap();
                    surfaces.with_context(1, &mut pixels, WIDTH, HEIGHT, |ctx| ctx.line_to(x2, y2)).unwrap();
                    surfaces.with_context(1, &mut pixels, WIDTH, HEIGHT, |ctx| ctx.stroke()).unwrap();
                }
            }
        }
        std::hint::black_box(&pixels);
        println!("{}: {:?} for {} segments", if retained { "retained" } else { "per-operation" }, started.elapsed(), FRAMES * SEGMENTS);
    }

    let mut pixels = vec![0; (WIDTH * HEIGHT * 4) as usize];
    let mut surfaces = CanvasSurfaces::default();
    let image = Image {
        width: 320,
        height: 200,
        pixels: Arc::new(vec![180; 320 * 200 * 4]),
        origin_clean: true,
    };
    let started = Instant::now();
    let mut first = None;
    for frame in 0..60 {
        surfaces
            .with_context(1, &mut pixels, WIDTH, HEIGHT, |ctx| {
                ctx.draw_image(&image, 0.0, 0.0, 320.0, 200.0);
            })
            .unwrap();
        if frame == 0 {
            first = Some(started.elapsed());
        }
    }
    std::hint::black_box(&pixels);
    println!("image blit: {:?} first, {:?} for 60 frames", first.unwrap(), started.elapsed());
}
