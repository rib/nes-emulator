use crate::constants::*;
use crate::prelude::*;
use crate::system::*;
use crate::cpu::*;
use crate::ppu::*;
use log::{warn};

pub struct Nes {
    pixel_format: PixelFormat,
    cpu: Cpu,
    apu: Apu,
    system: System,
    framebuffers: Vec<Vec<u8>>,
    current_fb: usize,
}

impl Nes {
    pub fn new(pixel_format: PixelFormat) -> Nes {
        let cpu = Cpu::default();
        let mut ppu = Ppu::default();
        let mut apu = Apu::default();
        ppu.draw_option.fb_width = FRAMEBUFFER_WIDTH as u32;
        ppu.draw_option.fb_height = FRAMEBUFFER_HEIGHT as u32;
        ppu.draw_option.offset_x = 0;
        ppu.draw_option.offset_y = 0;
        ppu.draw_option.scale = 1;
        ppu.draw_option.pixel_format = pixel_format;

        let mut framebuffers = vec![vec![0u8; FRAMEBUFFER_WIDTH * FRAMEBUFFER_HEIGHT * 4]; 2];

        let system = System::new(ppu, Cartridge::none());
        Nes {
            pixel_format, cpu, apu, system, framebuffers, current_fb: 0
        }
    }

    pub fn insert_cartridge(&mut self, cartridge: Option<Cartridge>) {
        if let Some(cartridge) = cartridge {
            self.system.cartridge = cartridge;
        } else {
            self.system.cartridge = Cartridge::none();
        }
    }

    pub fn poweron(&mut self) {
        self.cpu.poweron();
        self.system.poweron();
        self.cpu.interrupt(&mut self.system, Interrupt::RESET);
    }

    pub fn reset(&mut self) {
        self.cpu.p |= Flags::INTERRUPT;
        self.cpu.sp = self.cpu.sp.wrapping_sub(3);
        self.cpu.interrupt(&mut self.system, Interrupt::RESET);
    }

    pub fn system_mut(&mut self) -> &mut System {
        &mut self.system
    }
    pub fn system_cpu(&mut self) -> &mut Cpu {
        &mut self.cpu
    }
    pub fn system_ppu(&mut self) -> &mut Ppu {
        &mut self.system.ppu
    }

    pub fn allocate_framebuffer(&self) -> Framebuffer {
        Framebuffer::new(FRAMEBUFFER_WIDTH, FRAMEBUFFER_HEIGHT, self.pixel_format)
    }

    // Aiming for Meson compatible trace format which can be used for cross referencing
    #[cfg(feature="trace")]
    fn display_trace(&self) {
        let trace = &self.cpu.trace;
        let pc = trace.instruction_pc;
        let op = trace.instruction_op_code;
        let operand_len = trace.instruction.len() - 1;
        let bytecode_str = if operand_len == 2 {
            let lsb = trace.instruction_operand & 0xff;
            let msb = (trace.instruction_operand & 0xff00) >> 8;
            format!("${op:02X} ${lsb:02X} ${msb:02X}")
        } else if operand_len == 1{
            format!("${op:02X} ${:02X}", trace.instruction_operand)
        } else {
            format!("${op:02X}")
        };
        let disassembly = trace.instruction.disassemble(trace.instruction_operand, trace.effective_address, trace.loaded_mem_value, trace.stored_mem_value);
        let a = trace.saved_a;
        let x = trace.saved_x;
        let y = trace.saved_y;
        let sp = trace.saved_sp & 0xff;
        let p = trace.saved_p.to_flags_string();
        let cpu_cycles = trace.saved_cyc;
        println!("{pc:0X} {bytecode_str:11} {disassembly:23} A:{a:02X} X:{x:02X} Y:{y:02X} P:{p} SP:{sp:X} CPU Cycle:{cpu_cycles}");

    }

    pub fn tick_frame(&mut self, mut framebuffer: Framebuffer) {
        let rental = framebuffer.rent_data();
        if let Some(mut fb_data) = rental {
            let mut i = 0;
            while i < CYCLE_PER_DRAW_FRAME {
                let cyc = self.cpu.step(&mut self.system);

                #[cfg(feature="trace")]
                self.display_trace();

                i += cyc as usize;


                let irq = self.system.step(cyc.into(), &mut self.apu, fb_data.data.as_mut_ptr());

                if let Some(irq) = irq {
                    self.cpu.interrupt(&mut self.system, irq);
                }
            }
        } else {
            warn!("Can't tick with framebuffer that's still in use!");
        }
    }
}