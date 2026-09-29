//! Per-element `<canvas>` drawing state, and the fonts canvas text is drawn
//! with.
//!
//! A `<canvas>` is reached from a page the way every element is —
//! `getElementById`, then `getContext("2d")`, then draw. Each of those draw
//! calls arrives on its own, so something has to hold the context's state
//! between them. This is that something: one [`CanvasState`] per canvas
//! element, keyed by node id.
//!
//! **The pixels are not here.** They live on the element, in
//! `WebCore::image_data`, which the parser already allocates for a `<canvas>`
//! and the display-list builder already knows how to paint. Keeping one
//! bitmap rather than two is what makes `getImageData` and a rendered frame
//! agree by construction, and the alternative — recording the calls and
//! replaying them later — cannot answer `getImageData`, `toBlob` or
//! `isPointInPath` at all, because at the moment the page asks there are no
//! pixels to read.

use std::collections::HashMap;

use cosmic_text::{FontSystem, SwashCache};
use tiny_skia::{IntSize, Pixmap};

use super::{Canvas, CanvasState, TinySkiaCanvas};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasContextMode {
    TwoD,
    BitmapRenderer,
}

/// The drawing state of every `<canvas>` in one document.
#[derive(Default)]
pub struct CanvasSurfaces {
    /// node id → the context state that survives between calls. A canvas with
    /// no entry has never been drawn to, which is indistinguishable from one
    /// whose state is all defaults — so entries are made on demand.
    states: HashMap<u32, CanvasState>,
    modes: HashMap<u32, CanvasContextMode>,
    /// Fonts for canvas text, created on the first `fillText`.
    ///
    /// Separate from `Renderer::font_system` because a canvas is drawn when
    /// the PAGE calls it and the renderer's fonts exist only while a frame is
    /// being painted — reaching for them at call time would find nothing, and
    /// `fillText` would silently draw nothing at all, which is the failure
    /// that hides longest.
    ///
    /// Built lazily: constructing a `FontSystem` enumerates the system fonts,
    /// and a document with no canvas text should not pay for that.
    fonts: Option<Box<(FontSystem, SwashCache)>>,
}

// Note for anyone extending this: an entry OUTLIVES its element. Nothing here
// is notified when a node is removed from the tree, so a page that creates and
// discards canvases accumulates one `CanvasState` each — small, but it can
// carry a pixmap-sized clip `Mask`. Removing an entry needs a hook on node
// destruction, which the DOM does not have yet.

impl CanvasSurfaces {
    pub fn origin_clean(&self, node_id: u32) -> bool {
        self.states
            .get(&node_id)
            .is_none_or(CanvasState::origin_clean)
    }

    pub fn set_origin_clean(&mut self, node_id: u32, clean: bool) {
        self.states
            .entry(node_id)
            .or_default()
            .set_origin_clean(clean);
    }

    pub fn select_mode(&mut self, node_id: u32, mode: CanvasContextMode) -> bool {
        match self.modes.get(&node_id) {
            Some(current) => *current == mode,
            None => {
                self.modes.insert(node_id, mode);
                true
            }
        }
    }

    pub fn mode(&self, node_id: u32) -> Option<CanvasContextMode> {
        self.modes.get(&node_id).copied()
    }

    /// Run `f` against the canvas for `node_id`, over `pixels`.
    ///
    /// `pixels` is the element's own bitmap, moved in and moved back out —
    /// `Pixmap` owns its buffer, so lending it to tiny-skia and taking it back
    /// costs two moves and never copies the surface.
    ///
    /// `None` when the buffer does not match the declared size, which would
    /// mean the element's bitmap and its `width`/`height` had drifted apart.
    pub fn with_context<R>(
        &mut self,
        node_id: u32,
        pixels: &mut Vec<u8>,
        width: u32,
        height: u32,
        f: impl FnOnce(&mut dyn Canvas) -> R,
    ) -> Option<R> {
        self.with_context_syntax(node_id, pixels, width, height, super::CanvasSyntax::default(), f)
    }

    pub fn with_context_syntax<R>(
        &mut self,
        node_id: u32,
        pixels: &mut Vec<u8>,
        width: u32,
        height: u32,
        syntax: super::CanvasSyntax,
        f: impl FnOnce(&mut dyn Canvas) -> R,
    ) -> Option<R> {
        if width == 0 || height == 0 {
            if !pixels.is_empty() {
                return None;
            }
            let mut scratch = Pixmap::new(1, 1)?;
            let saved = self.states.remove(&node_id).unwrap_or_default();
            let fonts = self
                .fonts
                .get_or_insert_with(|| Box::new((FontSystem::new(), SwashCache::new())));
            let (font_system, swash_cache) = &mut **fonts;
            let mut canvas =
                TinySkiaCanvas::resume_empty(&mut scratch, saved, Some((font_system, swash_cache)));
            canvas.set_syntax(syntax);
            let out = f(&mut canvas);
            self.states.insert(node_id, canvas.suspend());
            return Some(out);
        }
        let size = IntSize::from_wh(width, height)?;
        let len = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        if pixels.len() != len {
            return None;
        }
        let mut pixmap = Pixmap::from_vec(std::mem::take(pixels), size)?;

        let saved = self.states.remove(&node_id).unwrap_or_default();
        let fonts = self
            .fonts
            .get_or_insert_with(|| Box::new((FontSystem::new(), SwashCache::new())));
        let (font_system, swash_cache) = &mut **fonts;

        let mut canvas =
            TinySkiaCanvas::resume(&mut pixmap, saved, Some((font_system, swash_cache)));
        canvas.set_syntax(syntax);
        let out = f(&mut canvas);
        self.states.insert(node_id, canvas.suspend());

        *pixels = pixmap.take();
        Some(out)
    }

    /// Drop the drawing state for one canvas, so the next call starts from the
    /// defaults.
    ///
    /// [HTML §4.12.5.1](https://html.spec.whatwg.org/multipage/canvas.html#the-canvas-element)
    /// requires this whenever `width` or `height` is assigned — **even when
    /// the value does not change** — and for `reset()`. The bitmap is cleared
    /// by the caller that owns it; this is the other half.
    pub fn reset(&mut self, node_id: u32) {
        self.states.remove(&node_id);
    }
}

impl std::fmt::Debug for CanvasSurfaces {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CanvasSurfaces")
            .field("canvases", &self.states.len())
            .field("fonts_loaded", &self.fonts.is_some())
            .finish()
    }
}
