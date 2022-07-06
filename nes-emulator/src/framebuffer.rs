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

impl Drop for FramebufferDataRental {
    fn drop(&mut self) {
        let data = std::mem::take(&mut self.data);
        self.owner.return_rental_data(data);
    }
}

impl<'a> Framebuffer {
    pub fn new(width: usize, height: usize, format: PixelFormat) -> Framebuffer {
        Framebuffer {
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

    pub fn rent_data(&mut self) -> Option<FramebufferDataRental> {
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