use std::sync::Arc;
use std::sync::Mutex;

use crate::color::Color32;

#[derive(Copy, Clone, Debug)]
pub enum PixelFormat {
    RGBA8888,
    RGB888,
    GREY8,
    //BGRA8888,
    //ARGB8888,
}

impl PixelFormat {
    /// Returns the number of bytes per pixel for the format
    pub fn bpp(&self) -> usize {
        match self {
            PixelFormat::RGBA8888 => 4,
            PixelFormat::RGB888 => 3,
            PixelFormat::GREY8 => 1,
        }
    }
}

pub enum FramebufferClearMode {
    /// Clears the framebuffer to the given color. Grey framebuffers will clear to the red channel.
    Solid(Color32),
    Checkerboard(u8, u8),
}

pub trait FramebufferInfo {
    fn width(&self) -> usize;
    fn height(&self) -> usize;
    fn format(&self) -> PixelFormat;
}

/// Ref-counted framebuffer that can loan out access to
/// the underlying data (single owner / accessor)
///
/// The intention is that the application allocates a persistent
/// chain of framebuffers that will be cycled through in a fifo
/// order (the number allocated will depend on the length of the
/// post-processing pipeline).
///
/// At each stage of the pipeline that stage will 'rent' access
/// to the data and the rental will be returned before the
/// framebuffer moves on to the next processor.
#[derive(Clone, Debug)]
pub struct Framebuffer {
    width: usize,
    height: usize,
    format: PixelFormat,
    inner: Arc<Mutex<FramebufferInner>>,
}

impl Default for Framebuffer {
    fn default() -> Self {
        Framebuffer::empty()
    }
}

impl FramebufferInfo for Framebuffer {
    fn width(&self) -> usize {
        self.width
    }

    fn height(&self) -> usize {
        self.height
    }

    fn format(&self) -> PixelFormat {
        self.format
    }
}

#[derive(Debug)]
pub struct FramebufferInner {
    data: Option<Vec<u8>>,
}
pub struct FramebufferDataRental {
    owner: Framebuffer,
    pub data: Vec<u8>,
}

impl Default for FramebufferDataRental {
    fn default() -> Self {
        Self {
            owner: Framebuffer::empty(),
            data: vec![],
        }
    }
}

/// Cloning a `FramebufferDataRental` will result in a new
/// owning framebuffer being created for the rental
impl Clone for FramebufferDataRental {
    fn clone(&self) -> Self {
        let width = self.owner.width;
        let height = self.owner.height;
        let format = self.owner.format;
        let new_owner = Framebuffer {
            width,
            height,
            format,
            inner: Arc::new(Mutex::new(FramebufferInner {
                data: Some(vec![0u8; width * height * format.bpp()]),
            })),
        };
        Self {
            owner: new_owner,
            data: self.data.clone(),
        }
    }
}

impl Drop for FramebufferDataRental {
    fn drop(&mut self) {
        let data = std::mem::take(&mut self.data);
        self.owner.return_rental_data(data);
    }
}

impl FramebufferInfo for FramebufferDataRental {
    fn width(&self) -> usize {
        self.owner.width
    }

    fn height(&self) -> usize {
        self.owner.height
    }

    fn format(&self) -> PixelFormat {
        self.owner.format
    }
}

impl FramebufferDataRental {
    /// Gets a reference to the owning framebuffer associated with the data rental
    pub fn owner(&self) -> Framebuffer {
        self.owner.clone()
    }

    pub fn plot(&mut self, x: usize, y: usize, color: Color32) {
        match self.owner.format {
            PixelFormat::RGBA8888 => {
                let stride = self.owner.width * 4;
                let off = stride * y + x * 4;
                self.data[off + 0] = color[0];
                self.data[off + 1] = color[1];
                self.data[off + 2] = color[2];
                self.data[off + 3] = color[3];
            }
            PixelFormat::RGB888 => {
                let stride = self.owner.width * 3;
                let off = stride * y + x * 3;
                self.data[off + 0] = color[0];
                self.data[off + 1] = color[1];
                self.data[off + 2] = color[2];
            }
            PixelFormat::GREY8 => {
                let off = self.owner.width * y + x;
                self.data[off] = color.to_grey();
            }
        }
    }

    pub fn clear(&mut self, mode: FramebufferClearMode) {
        match self.owner.format {
            PixelFormat::RGBA8888 => match mode {
                FramebufferClearMode::Solid(color) => {
                    for px in self.data.chunks_exact_mut(4) {
                        px[0] = color.r();
                        px[1] = color.g();
                        px[2] = color.b();
                        px[3] = color.a();
                    }
                }
                FramebufferClearMode::Checkerboard(mut a, mut b) => {
                    let bpp = 4;
                    let line_stride = self.owner.width * bpp;
                    let row_stride = line_stride * 16;
                    let col_stride = 16 * bpp;

                    for row in self.data.chunks_mut(row_stride) {
                        for line in row.chunks_exact_mut(line_stride) {
                            let mut la = a;
                            let mut lb = b;
                            for col_span in line.chunks_mut(col_stride) {
                                for px in col_span.chunks_exact_mut(4) {
                                    px[0] = la;
                                    px[1] = la;
                                    px[2] = la;
                                    px[3] = 0xff;
                                }
                                std::mem::swap(&mut la, &mut lb);
                            }
                        }
                        std::mem::swap(&mut a, &mut b);
                    }
                }
            },
            PixelFormat::RGB888 => match mode {
                FramebufferClearMode::Solid(color) => {
                    for px in self.data.chunks_exact_mut(3) {
                        px[0] = color.r();
                        px[1] = color.g();
                        px[2] = color.b();
                    }
                }
                FramebufferClearMode::Checkerboard(mut a, mut b) => {
                    let bpp = 3;
                    let line_stride = self.owner.width * bpp;
                    let row_stride = line_stride * 16;
                    let col_stride = 16 * bpp;

                    for row in self.data.chunks_mut(row_stride) {
                        for line in row.chunks_exact_mut(line_stride) {
                            let mut la = a;
                            let mut lb = b;
                            for col_span in line.chunks_mut(col_stride) {
                                for px in col_span.chunks_exact_mut(3) {
                                    px[0] = la;
                                    px[1] = la;
                                    px[2] = la;
                                }
                                std::mem::swap(&mut la, &mut lb);
                            }
                        }
                        std::mem::swap(&mut a, &mut b);
                    }
                }
            },
            PixelFormat::GREY8 => match mode {
                FramebufferClearMode::Solid(color) => {
                    let grey = color.r();
                    for px in self.data.iter_mut() {
                        *px = grey;
                    }
                }
                FramebufferClearMode::Checkerboard(mut a, mut b) => {
                    let line_stride = self.owner.width;
                    let row_stride = line_stride * 16;
                    let col_stride = 16;

                    for row in self.data.chunks_mut(row_stride) {
                        for line in row.chunks_exact_mut(line_stride) {
                            let mut la = a;
                            let mut lb = b;
                            for col_span in line.chunks_mut(col_stride) {
                                for px in col_span.iter_mut() {
                                    *px = la;
                                }
                                std::mem::swap(&mut la, &mut lb);
                            }
                        }
                        std::mem::swap(&mut a, &mut b);
                    }
                }
            },
        }
    }
}

impl<'a> Framebuffer {
    /// Creates an empty, zero-sized, place-holder framebuffer
    pub fn empty() -> Self {
        Framebuffer::new(0, 0, PixelFormat::RGBA8888)
    }

    /// Allocates a new framebuffer with the given `width`, `height` and pixel `format`
    pub fn new(width: usize, height: usize, format: PixelFormat) -> Self {
        Self {
            width,
            height,
            format,
            inner: Arc::new(Mutex::new(FramebufferInner {
                data: Some(vec![0u8; width * height * format.bpp()]),
            })),
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }
    pub fn format(&self) -> PixelFormat {
        self.format
    }

    pub fn rent_data(&self) -> Option<FramebufferDataRental> {
        let data = {
            let mut guard = self.inner.lock().unwrap();
            guard.data.take()
        };

        if let Some(data) = data {
            Some(FramebufferDataRental {
                owner: self.clone(),
                data,
            })
        } else {
            None
        }
    }

    fn return_rental_data(&mut self, rental_data: Vec<u8>) {
        let mut guard = self.inner.lock().unwrap();
        guard.data = Some(rental_data);
    }
}
