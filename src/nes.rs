use crate::constants::*;
use crate::prelude::*;
use crate::system::*;
use crate::cpu::*;
use crate::ppu::*;
use log::{warn};

pub struct Nes {
    pixel_format: PixelFormat,
    system: System,
    cpu: Cpu,
    ppu: Ppu,
    framebuffers: Vec<Vec<u8>>,
    current_fb: usize,
}

impl Nes {
    pub fn new(pixel_format: PixelFormat) -> Nes {
        let mut system = System::default();
        let mut cpu = Cpu::default();
        let mut ppu = Ppu::default();
        ppu.draw_option.fb_width = FRAMEBUFFER_WIDTH as u32;
        ppu.draw_option.fb_height = FRAMEBUFFER_HEIGHT as u32;
        ppu.draw_option.offset_x = 0;
        ppu.draw_option.offset_y = 0;
        ppu.draw_option.scale = 1;
        ppu.draw_option.pixel_format = pixel_format;
        
        let mut framebuffers = vec![vec![0u8; FRAMEBUFFER_WIDTH * FRAMEBUFFER_HEIGHT * 4]; 2];

        Nes {
            pixel_format, system, cpu, ppu, framebuffers, current_fb: 0
        }
    }

    pub fn insert_cartridge(&mut self, cartridge: Option<Cartridge>) {
       self.system.cartridge = cartridge; 
    }

    pub fn reset(&mut self) {
        self.cpu.reset();
        self.system.reset();
        self.ppu.reset();
        self.cpu.interrupt(&mut self.system, Interrupt::RESET);
    }

    pub fn system_mut(&mut self) -> &mut System {
        &mut self.system
    }
    pub fn system_cpu(&mut self) -> &mut Cpu {
        &mut self.cpu
    }
    pub fn system_ppu(&mut self) -> &mut Ppu {
        &mut self.ppu
    }

    pub fn allocate_framebuffer(&self) -> Framebuffer {
        Framebuffer::new(FRAMEBUFFER_WIDTH, FRAMEBUFFER_HEIGHT, self.pixel_format)
    }
    
    pub fn tick_frame(&mut self, mut framebuffer: Framebuffer) {
        let rental = framebuffer.rent_data();
        if let Some(mut fb_data) = rental {
            let mut i = 0;
            while i < CYCLE_PER_DRAW_FRAME {
                let cyc = self.cpu.step(&mut self.system);
                i += cyc as usize;

                let irq = self.ppu.step(cyc.into(), &mut self.system, fb_data.data.as_mut_ptr());
                
                if let Some(irq) = irq {
                    self.cpu.interrupt(&mut self.system, irq);
                }
            }
        } else {
            warn!("Can't tick with framebuffer that's still in use!");
        }
    }
}