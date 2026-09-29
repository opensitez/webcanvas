//! Owned canvas bitmap with the same 2D drawing state as a DOM `<canvas>`.

use tiny_skia::{IntSize, Pixmap};

use super::{Canvas, CanvasContextMode, CanvasSurfaces, TinySkiaCanvas};

/// The bitmap moved out by `OffscreenCanvas.transferToImageBitmap()`.
/// Pixels are premultiplied RGBA, like the canvas backing store.
#[derive(Debug)]
pub struct ImageBitmap {
    width: u32,
    height: u32,
    pixels: Option<Vec<u8>>,
    origin_clean: bool,
}

impl ImageBitmap {
    pub fn take_pixels(&mut self) -> Option<(u32, u32, Vec<u8>, bool)> {
        self.pixels
            .take()
            .map(|pixels| (self.width, self.height, pixels, self.origin_clean))
    }

    pub fn origin_clean(&self) -> bool {
        self.origin_clean
    }
    pub fn width(&self) -> u32 {
        if self.pixels.is_some() {
            self.width
        } else {
            0
        }
    }

    pub fn height(&self) -> u32 {
        if self.pixels.is_some() {
            self.height
        } else {
            0
        }
    }

    pub fn pixels(&self) -> Option<&[u8]> {
        self.pixels.as_deref()
    }

    pub fn close(&mut self) {
        self.pixels = None;
    }
}

/// A canvas whose bitmap is owned independently of the DOM and renderer.
/// The 2D context uses the same state machine as an HTMLCanvasElement.
pub struct OffscreenCanvas {
    width: u32,
    height: u32,
    bitmap_width: u32,
    bitmap_height: u32,
    pixels: Vec<u8>,
    surfaces: CanvasSurfaces,
    context_mode: Option<CanvasContextMode>,
    detached: bool,
}

impl OffscreenCanvas {
    pub fn new(width: u32, height: u32) -> Result<Self, &'static str> {
        Ok(Self {
            width,
            height,
            bitmap_width: width,
            bitmap_height: height,
            pixels: blank_bitmap(width, height)?,
            surfaces: CanvasSurfaces::default(),
            context_mode: None,
            detached: false,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn get_context_2d(&mut self) -> Result<bool, &'static str> {
        if self.detached {
            return Err("InvalidStateError");
        }
        if self.context_mode == Some(CanvasContextMode::BitmapRenderer) {
            return Ok(false);
        }
        self.context_mode = Some(CanvasContextMode::TwoD);
        Ok(true)
    }

    pub fn get_bitmap_context(&mut self) -> Result<bool, &'static str> {
        if self.detached {
            return Err("InvalidStateError");
        }
        if self.context_mode == Some(CanvasContextMode::TwoD) {
            return Ok(false);
        }
        self.context_mode = Some(CanvasContextMode::BitmapRenderer);
        Ok(true)
    }

    pub fn with_context_2d<R>(
        &mut self,
        f: impl FnOnce(&mut dyn Canvas) -> R,
    ) -> Result<R, &'static str> {
        if !self.get_context_2d()? {
            return Err("InvalidStateError");
        }
        self.surfaces
            .with_context(0, &mut self.pixels, self.width, self.height, f)
            .ok_or("InvalidStateError")
    }

    pub fn set_width(&mut self, width: u32) -> Result<(), &'static str> {
        self.resize(width, self.height)
    }

    pub fn set_height(&mut self, height: u32) -> Result<(), &'static str> {
        self.resize(self.width, height)
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<(), &'static str> {
        if self.detached {
            return Err("InvalidStateError");
        }
        let pixels = blank_bitmap(width, height)?;
        self.width = width;
        self.height = height;
        self.bitmap_width = width;
        self.bitmap_height = height;
        self.pixels = pixels;
        self.surfaces.reset(0);
        Ok(())
    }

    /// Move the current bitmap out, allocate a transparent replacement, and
    /// preserve the 2D drawing state on the context.
    pub fn transfer_to_image_bitmap(&mut self) -> Result<ImageBitmap, &'static str> {
        if self.detached || self.context_mode.is_none() {
            return Err("InvalidStateError");
        }
        let replacement = blank_bitmap(self.width, self.height)?;
        let image = ImageBitmap {
            width: self.bitmap_width,
            height: self.bitmap_height,
            pixels: Some(std::mem::replace(&mut self.pixels, replacement)),
            origin_clean: self.surfaces.origin_clean(0),
        };
        self.surfaces.set_origin_clean(0, true);
        self.bitmap_width = self.width;
        self.bitmap_height = self.height;
        Ok(image)
    }

    pub fn transfer_from_image_bitmap(
        &mut self,
        image: Option<&mut ImageBitmap>,
    ) -> Result<(), &'static str> {
        if !self.get_bitmap_context()? {
            return Err("InvalidStateError");
        }
        if let Some(image) = image {
            let (width, height, pixels, origin_clean) =
                image.take_pixels().ok_or("InvalidStateError")?;
            self.bitmap_width = width;
            self.bitmap_height = height;
            self.pixels = pixels;
            self.surfaces.set_origin_clean(0, origin_clean);
        } else {
            self.pixels = blank_bitmap(self.width, self.height)?;
            self.bitmap_width = self.width;
            self.bitmap_height = self.height;
            self.surfaces.set_origin_clean(0, true);
        }
        Ok(())
    }

    /// Synchronous encoding core for the `convertToBlob()` Promise binding.
    /// Unsupported MIME types fall back to PNG as required by HTML.
    pub fn encode_blob(&self, mime: &str, quality: Option<f32>) -> Result<Vec<u8>, &'static str> {
        if self.detached {
            return Err("InvalidStateError");
        }
        if !self.surfaces.origin_clean(0) {
            return Err("SecurityError");
        }
        let size =
            IntSize::from_wh(self.bitmap_width, self.bitmap_height).ok_or("IndexSizeError")?;
        let mut pixmap = Pixmap::from_vec(self.pixels.clone(), size).ok_or("InvalidStateError")?;
        let canvas = TinySkiaCanvas::new(&mut pixmap);
        let mime = super::canvas_encoder_mime(mime);
        canvas.to_blob(mime, quality).ok_or("EncodingError")
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Structured transfer is only allowed before a rendering context exists.
    pub fn transfer(&mut self) -> Result<Self, &'static str> {
        if self.detached || self.context_mode.is_some() {
            return Err("InvalidStateError");
        }
        let pixels = blank_bitmap(self.width, self.height)?;
        let received = Self {
            width: self.width,
            height: self.height,
            bitmap_width: self.bitmap_width,
            bitmap_height: self.bitmap_height,
            pixels,
            surfaces: CanvasSurfaces::default(),
            context_mode: None,
            detached: false,
        };
        self.detached = true;
        self.pixels.clear();
        Ok(received)
    }
}

fn blank_bitmap(width: u32, height: u32) -> Result<Vec<u8>, &'static str> {
    let len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or("RangeError")?;
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(len).map_err(|_| "RangeError")?;
    pixels.resize(len, 0);
    Ok(pixels)
}

#[cfg(test)]
mod tests {
    use super::OffscreenCanvas;
    use crate::canvas::{Color, Image, Paint};

    #[test]
    fn taint_follows_image_bitmap_and_new_offscreen_bitmap_starts_clean() {
        let mut canvas = OffscreenCanvas::new(1, 1).unwrap();
        let image = Image::from_rgba(1, 1, vec![255, 0, 0, 255]).with_origin_clean(false);
        canvas
            .with_context_2d(|ctx| ctx.draw_image(&image, 0.0, 0.0, 1.0, 1.0))
            .unwrap();
        assert_eq!(canvas.encode_blob("image/png", None), Err("SecurityError"));
        let mut bitmap = canvas.transfer_to_image_bitmap().unwrap();
        assert!(!bitmap.origin_clean());
        assert!(canvas.encode_blob("image/png", None).is_ok());

        let mut receiver = OffscreenCanvas::new(1, 1).unwrap();
        receiver
            .transfer_from_image_bitmap(Some(&mut bitmap))
            .unwrap();
        assert_eq!(
            receiver.encode_blob("image/png", None),
            Err("SecurityError")
        );
        receiver.transfer_from_image_bitmap(None).unwrap();
        assert!(receiver.encode_blob("image/png", None).is_ok());
    }

    #[test]
    fn context_persists_and_resize_resets_even_at_the_same_size() {
        let mut canvas = OffscreenCanvas::new(2, 1).unwrap();
        canvas
            .with_context_2d(|ctx| ctx.set_fill_color(Color::rgb(255, 0, 0)))
            .unwrap();
        canvas
            .with_context_2d(|ctx| ctx.fill_rect(0.0, 0.0, 1.0, 1.0))
            .unwrap();
        assert_eq!(&canvas.pixels()[0..4], &[255, 0, 0, 255]);
        canvas.set_width(2).unwrap();
        assert!(canvas.pixels().iter().all(|byte| *byte == 0));
        let fill = canvas
            .with_context_2d(|ctx| ctx.drawing_state().fill.clone())
            .unwrap();
        assert_eq!(fill, Paint::Color(Color::BLACK));
    }

    #[test]
    fn transfer_moves_pixels_and_keeps_the_context_state() {
        let mut canvas = OffscreenCanvas::new(2, 1).unwrap();
        assert!(canvas.transfer_to_image_bitmap().is_err());
        canvas
            .with_context_2d(|ctx| {
                ctx.set_fill_color(Color::rgb(0, 255, 0));
                ctx.fill_rect(0.0, 0.0, 2.0, 1.0);
            })
            .unwrap();
        let mut image = canvas.transfer_to_image_bitmap().unwrap();
        assert_eq!((image.width(), image.height()), (2, 1));
        assert_eq!(&image.pixels().unwrap()[0..4], &[0, 255, 0, 255]);
        assert!(canvas.pixels().iter().all(|byte| *byte == 0));
        canvas
            .with_context_2d(|ctx| ctx.fill_rect(0.0, 0.0, 1.0, 1.0))
            .unwrap();
        assert_eq!(&canvas.pixels()[0..4], &[0, 255, 0, 255]);
        image.close();
        assert_eq!((image.width(), image.height()), (0, 0));
        assert!(image.pixels().is_none());
    }

    #[test]
    fn zero_sized_context_and_blob_rules() {
        let mut canvas = OffscreenCanvas::new(0, 4).unwrap();
        canvas
            .with_context_2d(|ctx| ctx.set_fill_color(Color::rgb(255, 0, 0)))
            .unwrap();
        let pixels = canvas
            .with_context_2d(|ctx| ctx.get_image_data(0, 0, 1, 1).unwrap().data)
            .unwrap();
        assert_eq!(pixels, vec![0, 0, 0, 0]);
        assert_eq!(canvas.encode_blob("image/png", None), Err("IndexSizeError"));
    }

    #[test]
    fn encoding_and_structured_transfer() {
        let mut canvas = OffscreenCanvas::new(1, 1).unwrap();
        let mut received = canvas.transfer().unwrap();
        assert_eq!(canvas.get_context_2d(), Err("InvalidStateError"));
        received
            .with_context_2d(|ctx| {
                ctx.set_fill_color(Color::rgb(0, 0, 255));
                ctx.fill_rect(0.0, 0.0, 1.0, 1.0);
            })
            .unwrap();
        assert!(received.transfer().is_err());
        let encoded = received.encode_blob("unsupported/type", None).unwrap();
        assert_eq!(&encoded[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        let decoded = image::load_from_memory(&encoded).unwrap().to_rgba8();
        assert_eq!(decoded.get_pixel(0, 0).0, [0, 0, 255, 255]);
    }

    #[test]
    fn structured_transfer_initializes_a_transparent_receiving_bitmap() {
        let mut canvas = OffscreenCanvas::new(1, 1).unwrap();
        canvas.pixels.copy_from_slice(&[255, 0, 0, 255]);
        let received = canvas.transfer().unwrap();
        assert_eq!(received.pixels(), &[0, 0, 0, 0]);
        assert!(canvas.pixels().is_empty());
    }

    #[test]
    fn bitmap_renderer_consumes_an_image_and_locks_the_context_mode() {
        let mut source = OffscreenCanvas::new(2, 1).unwrap();
        source
            .with_context_2d(|ctx| {
                ctx.set_fill_color(Color::rgb(255, 0, 0));
                ctx.fill_rect(0.0, 0.0, 2.0, 1.0);
            })
            .unwrap();
        let mut image = source.transfer_to_image_bitmap().unwrap();
        let mut target = OffscreenCanvas::new(1, 1).unwrap();
        assert_eq!(target.get_bitmap_context(), Ok(true));
        assert_eq!(target.get_context_2d(), Ok(false));
        target.transfer_from_image_bitmap(Some(&mut image)).unwrap();
        assert_eq!((image.width(), image.height()), (0, 0));
        assert_eq!(&target.pixels()[0..4], &[255, 0, 0, 255]);
        let transferred = target.transfer_to_image_bitmap().unwrap();
        assert_eq!((transferred.width(), transferred.height()), (2, 1));
        assert_eq!(target.pixels().len(), 4);
        assert!(target.pixels().iter().all(|byte| *byte == 0));
        target.transfer_from_image_bitmap(None).unwrap();
        assert_eq!(target.pixels().len(), 4);
    }
}
