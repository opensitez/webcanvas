# webcanvas

`webcanvas` is a Rust 2D Canvas implementation that renders into an application-owned
pixel buffer. It does not create a window or require a DOM. A browser, desktop
application, game engine, or test runner can use the same drawing surface and
present the resulting pixels through its own display system.

**Dual-licensed:** GPLv3-or-later for compatible open-source projects, or a
separate commercial license for proprietary use. See [License](#license).

The `webcanvas::canvas` module provides paths, fills and strokes, gradients,
patterns, images, text, transforms, clipping, compositing, shadows, filters,
image data, recording, and offscreen bitmaps. `TinySkiaCanvas` draws immediately
to a `tiny_skia::Pixmap`; `CanvasSurfaces` manages persistent 2D context state
for multiple application-owned surfaces.

## Quick Start

```toml
[dependencies]
webcanvas = "0.1"
tiny-skia = "0.12"
```

```rust
use webcanvas::canvas::{Canvas, Color, TinySkiaCanvas};

let mut bitmap = tiny_skia::Pixmap::new(320, 180).unwrap();
{
    let mut context = TinySkiaCanvas::new(&mut bitmap);
    context.set_fill_color(Color::rgb(25, 105, 180));
    context.fill_rect(20.0, 20.0, 280.0, 140.0);
}
bitmap.save_png("canvas.png").unwrap();
```

Run the complete [embedding example](examples/embed.rs) from this repository:

```sh
cargo run --manifest-path crates/webcanvas/Cargo.toml --example embed -- canvas.png
```

The example creates a host-owned RGBA buffer, draws through `CanvasSurfaces`,
then writes the result as a PNG. In a windowed application, hand that buffer to
your window or GPU texture instead of saving it. Tiny-skia stores **premultiplied
RGBA8** pixels; convert them if your display API expects a different format.

## Embed a Persistent Context

Keep a `CanvasSurfaces` instance for as long as the host view exists. Each
numeric surface ID retains its drawing state between calls, while the bitmap
remains owned by the host:

```rust
use webcanvas::canvas::{CanvasSurfaces, Color};

let (width, height) = (320_u32, 180_u32);
let mut pixels = vec![0; (width * height * 4) as usize];
let mut surfaces = CanvasSurfaces::default();
let surface_id = 1;

surfaces.with_context(surface_id, &mut pixels, width, height, |ctx| {
    ctx.set_fill_color(Color::rgb(240, 80, 60));
    ctx.fill_rect(12.0, 12.0, 80.0, 80.0);
}).expect("valid bitmap dimensions");

// Present `pixels` using the host application's windowing or graphics API.
```

The buffer must contain `width * height * 4` bytes. After resizing, allocate
a new buffer and call `surfaces.reset(surface_id)` to reset the context state.

## CSS Values

Typed drawing calls work without a CSS engine. For CSS string properties such
as `fillStyle`, `font`, and `filter`, supply parsers through `CanvasSyntax` and
call `with_context_syntax`. This keeps color and font parsing consistent with
the embedding browser rather than shipping a second, conflicting CSS parser
inside the rasterizer. Webcore and Widgets use this integration point.

## Copyright

Copyright (c) 2026 OpenSitez.com and Youness El Andaloussi. All rights reserved.

## License

`webcanvas` is **dual-licensed**. Choose the license that fits your project:

**Open source (GPLv3 or later).** If you are building an open-source application
under a compatible license, you may use `webcanvas` under the terms of the
[GNU General Public License v3.0](LICENSE-GPL) or later.

**Commercial license.** To use `webcanvas` in a proprietary, closed-source
product without the requirements of GPLv3, you must purchase a separate
commercial license. Contact OpenSitez.com for pricing and terms.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in `webcanvas` by you, as defined in the Apache-2.0 license,
shall be dedicated to the public domain (or equivalent, such as the CC0 1.0
Universal public domain dedication). This allows contributions to be used in
both the GPLv3 and commercial releases.
