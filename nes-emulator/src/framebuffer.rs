use std::sync::Arc;
use std::sync::Mutex;
use super::ppu::*;

/// Ref-counted framebuffer that can loan out access to
/// the underlying data (single owner / accessor)
///
/// The intention is that the application allocates a persistent
/// chain of framebuffer that will be cycled through in a fifo
/// order (the number allocated will depend on the lenght of the
/// post-processing pipeline).
///
/// At each stage of the pipeline that stage will 'rent' access
/// to the data and the rental will be returned before the
/// framebuffer moves on to the processor.
#[derive(Clone, Debug)]
pub struct Framebuffer {
    width: usize,
    height: usize,
    format: PixelFormat,
    inner: Arc<Mutex<FramebufferInner>>,
}

#[derive(Debug)]
pub struct FramebufferInner {
    data: Option<Vec<u8>>
}
pub struct FramebufferDataRental {
    owner: Framebuffer,
    pub data: Vec<u8>
}
/// Cloning a `FramebufferDataRental` will result in a new
/// owning framebuffer being created for the rental
impl Clone for FramebufferDataRental {
    fn clone(&self) -> Self {
        let width = self.owner.width;
        let height = self.owner.height;
        let format = self.owner.format;
        let fb_clone = Framebuffer {
            width,
            height,
            format,
            inner: Arc::new(Mutex::new(FramebufferInner {
                data: Some(vec![0u8; width * height * format.bpp()])
            }))
        };
        Self { owner: self.owner.clone(), data: self.data.clone() }
    }
}

impl Drop for FramebufferDataRental {
    fn drop(&mut self) {
        let data = std::mem::take(&mut self.data);
        self.owner.return_rental_data(data);
    }
}

impl FramebufferDataRental {
    /// Gets a reference to the owning framebuffer associated with the data rental
    pub fn owner(&self) -> Framebuffer {
        self.owner.clone()
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
                data: Some(vec![0u8; width * height * 4])
            }))
        }
    }

    pub fn width(&self) -> usize { self.width }
    pub fn height(&self) -> usize { self.height }
    pub fn format(&self) -> PixelFormat { self.format }

    pub fn rent_data(&self) -> Option<FramebufferDataRental> {
        let data = {
            let mut guard = self.inner.lock().unwrap();
            guard.data.take()
        };

        if let Some(data) = data {
            Some(FramebufferDataRental {
                owner: self.clone(),
                data
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