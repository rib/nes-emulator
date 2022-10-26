use bitflags::bitflags;
use std::{cell::RefCell, rc::Rc};

use egui::{epaint, pos2, vec2, Painter, Rect, TextureHandle, Ui};
use nes_emulator::{
    apu::channel::frame_sequencer::FrameSequencerStatus,
    constants::*,
    framebuffer::{Framebuffer, FramebufferClearMode, FramebufferDataRental, PixelFormat},
    hook::HookHandle,
    nes::Nes,
    trace::{CpuInterruptStatus, TraceEvent},
};

use crate::ui::{blank_texture_for_framebuffer, full_framebuffer_image_delta};

const TRACE_EVENTS_DOT_WIDTH: usize = 341;
const TRACE_EVENTS_DOT_HEIGHT: usize = 262;

struct SpritesHookState {
    screen_framebuffer_front: FramebufferDataRental,
}

#[derive(Clone)]
enum IoOp {
    Read(u16, u8),
    Write(u16, u8),
}

bitflags! {
    #[derive(Default)]
    struct DotViewFlags: u32 {
        const APU_IRQ_RAISED = 1<<0;
        const MAPPER_IRQ_RAISED = 1<<1;
        const NMI_RAISED = 1<<2;

        const IRQ_DETECT_PHI2 = 1<<3;
        const NMI_DETECT_PHI2 = 1<<4;

        const IRQ_DETECT_PHI1 = 1<<5;
        const NMI_DETECT_PHI1 = 1<<6;

        const IRQ_POLL = 1<<7;
        const NMI_POLL = 1<<8;

        const INTERRUPT_DISPATCH = 1<<9;

        const DMA_READ = 1<<10;
        const DMA_WRITE = 1<<11;

        const APU_HALF_FRAME = 1<<12;
        const APU_QUARTER_FRAME = 1<<13;
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum ApuOutput {
    #[default]
    None,
    Mixer,
    Pulse1,
    Pulse2,
    Noise,
    Triangle,
    Dmc,
}

#[derive(Default, Clone)]
struct DotView {
    visible: bool,
    cpu_clock: u64,
    //line: u16,
    //dot: u16,
    system_bus_io: Option<IoOp>,
    ppu_bus_io: Option<IoOp>,
    apu_output: f32,
    flags: DotViewFlags,
    //interrupts: InterruptEvents
}

pub struct TraceEventsView {
    pub visible: bool,

    paused: bool,

    zoom: f32,
    line_gap_height: f32,
    line_height: f32,

    show_irq_interrupts: bool,
    show_nmi_interrupts: bool,
    show_dma_io: bool,
    show_ppu_register_io: bool,
    show_apu_register_io: bool,
    show_port_register_io: bool,
    show_mapper_register_io: bool,
    show_apu_sequencer: bool,
    show_apu_output: ApuOutput,

    screen_texture: TextureHandle,
    queue_screen_fb_upload: bool,

    mux_hook_handle: Option<HookHandle>,
    dot_hook_handle: Option<HookHandle>,
    hook_state: Rc<RefCell<SpritesHookState>>,

    //line_dot_to_cpu: Vec<usize>,

    // at the start of each scanline we get notified of the cpu clock count.
    // Note: since there a ~3 PPU clocks per CPU clock, we don't know if the start
    // of the line corresponds to the start/middle/end of this CPU cycle so can't
    // linearly interpolate a mapping from CPU -> PPU dot based on this, but we
    // can use it as a reference point and then use `nes.cpu_to_ppu_clock()` to
    // map future cycles to dots for the rest of the line
    //line_cpu_start: u64,
    // Since we know the number of CPU cycles per scanline (rounded up to an integer)
    // we can create a lookup table mapping for the current line from CPU cycles to
    // PPU dots based on the nes.cpu_to_ppu_clock() function which will account for
    // the clock [mis]alignment in the same way that the emulator does internally
    line_cpu_to_dot: Vec<u16>,

    line_dot_to_cpu: Vec<u64>,

    // We have vectors for building up the view data for two scanlines because
    // the CPU clock at the end of a line may cross over onto the next line
    // These vectors are swapped at the start of a new line
    scanline_view: Vec<DotView>,
    next_scanline_view: Vec<DotView>,

    hover_pos: [usize; 2],
}

impl TraceEventsView {
    pub fn new(ctx: &egui::Context) -> Self {
        let screen_framebuffer_front =
            Framebuffer::new(FRAME_WIDTH, FRAME_HEIGHT, PixelFormat::RGB888);
        let screen_framebuffer_front = screen_framebuffer_front.rent_data().unwrap();
        let screen_texture = blank_texture_for_framebuffer(
            ctx,
            &screen_framebuffer_front,
            "trace_events_screen_framebuffer",
        );

        Self {
            visible: false,
            paused: false,

            zoom: 4.0f32,
            line_height: 1.0f32,
            line_gap_height: 0.5f32,

            screen_texture,
            queue_screen_fb_upload: false,

            mux_hook_handle: None,
            dot_hook_handle: None,
            hook_state: Rc::new(RefCell::new(SpritesHookState {
                screen_framebuffer_front,
            })),

            //line_cpu_start: 0,
            line_cpu_to_dot: vec![],
            // Note: we sometimes create mapping beyond a single scanline to account for the CPU
            // running ahead of the PPU which can mean there are CPU events that belong to the next
            // scanline - and we need the dot_to_cpu mapping to be able to determine the correct
            // cpu_to_dot mapping for those overflow events.
            line_dot_to_cpu: vec![],

            scanline_view: vec![DotView::default(); TRACE_EVENTS_DOT_WIDTH],
            next_scanline_view: vec![DotView::default(); TRACE_EVENTS_DOT_WIDTH],
            hover_pos: [0, 0],

            show_irq_interrupts: false,
            show_nmi_interrupts: false,
            show_dma_io: false,
            show_ppu_register_io: false,
            show_apu_register_io: false,
            show_port_register_io: false,
            show_mapper_register_io: false,
            show_apu_sequencer: false,
            show_apu_output: ApuOutput::None,
        }
    }

    pub fn set_paused(&mut self, paused: bool, _nes: &mut Nes) {
        self.paused = paused;
    }

    pub fn set_visible(&mut self, nes: &mut Nes, visible: bool) {
        self.visible = visible;
        if visible {
            let shared = self.hook_state.clone();

            // By giving the closure ownership of the back buffer then this per-pixel hook
            // avoids needing to poke into the Rc<RefCell<>> every pixel and only needs
            // to swap the back buffer into the shared state at the end of the frame
            let screen_framebuffer_back =
                Framebuffer::new(FRAME_WIDTH, FRAME_HEIGHT, PixelFormat::RGB888);
            let mut screen_framebuffer_back = screen_framebuffer_back.rent_data().unwrap();
            self.mux_hook_handle = Some(nes.ppu_mut().add_mux_hook(Box::new(
                move |_ppu, _cartridge, state| {
                    //println!("screen x = {}, y = {}, enabled = {}", state.screen_x, state.screen_y, state.rendering_enabled);
                    if state.screen_x == 0 && state.screen_y == 0 {
                        screen_framebuffer_back
                            .clear(FramebufferClearMode::Checkerboard(0x80, 0xa0));
                    }
                    let color = nes_emulator::ppu_palette::rgb_lut(state.palette_value);
                    screen_framebuffer_back.plot(
                        state.screen_x as usize,
                        state.screen_y as usize,
                        color,
                    );

                    if state.screen_x == 255 && state.screen_y == 239 {
                        std::mem::swap(
                            &mut screen_framebuffer_back,
                            &mut shared.borrow_mut().screen_framebuffer_front,
                        );
                        //println!("Finished frame");
                    }
                },
            )));

            self.dot_hook_handle = Some(nes.ppu_mut().add_dot_hook(
                240,
                0,
                Box::new(move |_ppu, _cartridge| {
                    //self.queue_screen_fb_upload = true;
                }),
            ));
        } else {
            if let Some(handle) = self.mux_hook_handle {
                nes.ppu_mut().remove_mux_hook(handle);
                self.mux_hook_handle = None;
            }
            if let Some(handle) = self.dot_hook_handle {
                nes.ppu_mut().remove_dot_hook(240, 0, handle);
                self.dot_hook_handle = None;
            }
        }
    }

    pub fn update(&mut self, _nes: &mut Nes) {
        /*
        let bpp = 3;
        let stride = FRAME_WIDTH * bpp;

        for y in 0..FRAME_HEIGHT {
            for x in 0..FRAME_WIDTH {
                let pix = nes.debug_sample_sprites(x, y);
                let pos = y * stride + x * bpp;
                self.screen_framebuffer[pos + 0] = pix[0];
                self.screen_framebuffer[pos + 1] = pix[1];
                self.screen_framebuffer[pos + 2] = pix[2];
            }
        }

        */

        self.queue_screen_fb_upload = true;
    }

    pub fn zoom_in(&mut self) {
        self.zoom += 1.0f32;
        self.line_height -= 0.1f32;
        if self.line_height < 0.5 {
            self.line_height = 0.5;
        }
        self.line_gap_height += 0.1f32;
        if self.line_gap_height > 2.0 {
            self.line_gap_height = 2.0;
        }
    }
    pub fn zoom_out(&mut self) {
        self.zoom -= 1.0f32;
        if self.zoom < 1.0f32 {
            self.zoom = 1.0f32;
        }
        self.line_height += 0.1f32;
        if self.line_height > 1.0 {
            self.line_height = 1.0;
        }
        self.line_gap_height -= 0.1f32;
        if self.line_gap_height < 0.5 {
            self.line_gap_height = 0.5;
        }
    }

    fn is_address_filtered(&self, addr: u16, read: bool) -> bool {
        match addr {
            0x0000..=0x1fff => { // RAM
            }
            0x2000..=0x3fff => {
                // PPU I/O
                if self.show_ppu_register_io {
                    return true;
                }
            }
            0x4000..=0x401f => {
                let index = usize::from(addr - 0x4000);
                match index {
                    0x14 => { // OAMDMA
                    }
                    0x16 => {
                        // READ = controller port 1, WRITE = APU + port 1/2
                        if read {
                            if self.show_port_register_io {
                                return true;
                            }
                        } else {
                            if self.show_port_register_io {
                                return true;
                            }
                            if self.show_apu_register_io {
                                return true;
                            }
                        }
                    }
                    0x17 => {
                        // READ = controller port 2, WRITE = APU
                        if read {
                            if self.show_port_register_io {
                                return true;
                            }
                        } else {
                            if self.show_apu_register_io {
                                return true;
                            }
                        }
                    }
                    _ => {
                        // APU I/O
                        if self.show_apu_register_io {
                            return true;
                        }
                    }
                }
            }
            _ => { // Cartridge
            }
        }
        false
    }
    /// Processes System, PPU and CPU trace events
    ///
    /// Returns: (next_line, modified-scanline, modified-next-scanline)
    #[inline(always)]
    fn process_main_events_line<F: Fn(u64) -> u64, C: Fn(usize) -> bool>(
        &mut self,
        nes: &mut Nes,
        cull_line: bool,
        dot_horizontal_cull: C,
        show_current_events: bool,
        ppu_to_cpu_mapper: F,
        line_start_index: usize, // in-out
        line_start_cpu_clk: u64,
        scanline_view: &mut Vec<DotView>,
        next_scanline_view: &mut Vec<DotView>,
    ) -> (Option<(u64, u16, usize)>, bool, bool) {
        let mut found_next_line = None;
        let mut modified_scanline = false;
        let mut modified_next_scanline = false;

        let events = if show_current_events {
            &nes.ppu_mut().debug.trace_events_current
        } else {
            &nes.ppu_mut().debug.trace_events_prev
        };
        let slice = &events[line_start_index..];
        //println!("Scanning {} events for line", slice.len());
        for (i, event) in slice.iter().enumerate() {
            if i > 0 {
                if let TraceEvent::PpuCpuLineSync {
                    cpu_clk,
                    ppu_clk: _,
                    line,
                } = event
                {
                    found_next_line = Some((*cpu_clk, *line, line_start_index + i));
                    //println!("Next line sync found: line = {next_line}, index = {}", line_start_index);
                    break;
                }
            }

            if cull_line {
                // Just scan for the next line sync event
                continue;
            }

            let dot = match event {
                TraceEvent::PpuCpuLineSync {
                    cpu_clk,
                    ppu_clk,
                    line: _,
                } => {
                    //debug_assert_eq!(current_line, *line);
                    //debug_assert_eq!(*cpu_clk, line_start_clk);

                    let expected_dot0_clk = ppu_to_cpu_mapper(*ppu_clk);

                    // Allow for a discrepancy between the actual CPU clock and expected, e.g.
                    // considering PPU breakpoints that may pause the PPU in the middle of a
                    // CPU cycle. The clock will re-sync to the expected value at some point
                    // and our visualization will represent the non-debug clock alignment.
                    let clk_fixup = *cpu_clk as i64 - expected_dot0_clk as i64;

                    // For events that just contain a CPU clock we need to be able to map them
                    // to a scanline dot.
                    //
                    // We want to map CPU clocks to the first PPU dot that overlaps with that CPU
                    // clock, which is not what we would get from the nes.cpu_to_ppu_clock function
                    // - which effectively maps elapsed CPU cycle counts into elapsed PPU cycles
                    // (which is more like pointing a CPU cycle to the last corresponding PPU
                    // cycle instead of the first)
                    //
                    // The other consideration here is that dot0 might be the tail end of a
                    // CPU cycle but any events for that CPU cycle need to map to dot0 even
                    // if the start of the CPU cycle was on the previous scanline. (It's a given
                    // that all events up until the next line sync are for this line)
                    //
                    // The simplest approach for now is to fill in the cpu->ppu mapping as we
                    // fill out the dot->cpu mapping

                    //let mut debug = vec![];

                    //println!("Line start cpu clk = {}, ppu clk = {}, cpu clock fixup = {}", *cpu_clk, *ppu_clk, clk_fixup);
                    self.line_cpu_to_dot.clear();
                    self.line_dot_to_cpu.clear();
                    self.line_cpu_to_dot.push(0);
                    let mut prev_clk = *cpu_clk;
                    for dot in 0..TRACE_EVENTS_DOT_WIDTH {
                        let clk =
                            (ppu_to_cpu_mapper(*ppu_clk + dot as u64) as i64 + clk_fixup) as u64;
                        self.line_dot_to_cpu.push(clk);
                        if clk != prev_clk {
                            debug_assert_eq!(
                                clk - line_start_cpu_clk,
                                self.line_cpu_to_dot.len() as u64
                            );
                            self.line_cpu_to_dot.push(dot as u16);
                            prev_clk = clk;
                        }
                        //debug.push((dot, clk)); // DEBUG
                    }

                    // Considering that execution of a CPU cycle starts ahead of any PPU cycle, and also
                    // that the PPU may become halted by a breakpoint then it's possible for the CPU to
                    // briefly get ahead of the PPU and we may get some traced CPU events that really
                    // belong to the _next_ scanline.
                    //
                    // E.g. a negative clk_fixup means we know the CPU was ahead of where it was expected
                    // to be at the start of the scanline. As a heuristic we try to anticipate some overflow
                    // according to clk_fixup + 1 cycle - and then cpu cycles any further out of range will
                    // simply generate a warning and the events will be discarded since they can't be mapped to
                    // a dot.
                    let mut overflow_cyc = if clk_fixup < 0 {
                        -clk_fixup as usize + 1
                    } else {
                        1
                    };
                    //println!("Allowing for {} cpu clocks of overflow", overflow_cyc);
                    let mut overflow_dot = 341u16; // Dots >= 341 will be recognised later as overflow for the next line
                    while overflow_cyc > 0 {
                        let clk = (ppu_to_cpu_mapper(*ppu_clk + overflow_dot as u64) as i64
                            + clk_fixup) as u64;
                        if clk != prev_clk {
                            debug_assert_eq!(
                                clk - line_start_cpu_clk,
                                self.line_cpu_to_dot.len() as u64
                            );
                            self.line_cpu_to_dot.push(overflow_dot);
                            prev_clk = clk;
                            overflow_cyc -= 1;
                        }
                        //debug.push((overflow_dot as usize, clk)); // DEBUG
                        overflow_dot += 1;
                    }
                    //println!("line dot to cpu map: {:?}", self.line_dot_to_cpu);
                    //println!("line cpu to dot map: {:?}", self.line_cpu_to_dot);
                    //println!("line debug map: {:?}", debug);
                    0u16
                }
                TraceEvent::CpuRead { clk_lower, .. }
                | TraceEvent::CpuWrite { clk_lower, .. }
                | TraceEvent::CpuDmaRead { clk_lower, .. }
                | TraceEvent::CpuDmaWrite { clk_lower, .. }
                | TraceEvent::CpuInterruptStatus { clk_lower, .. } => {
                    let mut full_clk = line_start_cpu_clk & 0xffffffff_ffffff00 | *clk_lower as u64;
                    if full_clk < line_start_cpu_clk {
                        // check if there was an overflow in the lower 8 bits
                        full_clk += 256
                    }

                    let clk_line_delta = full_clk - line_start_cpu_clk;
                    if clk_line_delta >= self.line_cpu_to_dot.len() as u64 {
                        log::warn!("CPU clock {} ran too far into the future: can't map trace events to scanline dots (would be clock {} for the line)", full_clk, clk_line_delta);
                        continue;
                    }
                    self.line_cpu_to_dot[clk_line_delta as usize]
                }
                _ => {
                    continue;
                }
            } as usize;

            let (dot, dot_view, is_overflow) = if dot >= 341 {
                let dot = dot % 341;
                (dot, &mut next_scanline_view[dot], true)
            } else {
                (dot, &mut scanline_view[dot], false)
            };

            if dot_horizontal_cull(dot) {
                continue;
            }

            dot_view.visible = true;
            if is_overflow {
                modified_next_scanline = true;
            } else {
                modified_scanline = true;
            }

            dot_view.cpu_clock = self.line_dot_to_cpu[dot];
            //dot_view.line = line;
            //dot_view.dot = dot as u16;
            match event {
                TraceEvent::CpuRead { addr, value, .. } => {
                    //println!("CPU Read Event line = {current_line}, dot = {dot}");
                    if self.is_address_filtered(*addr, true) {
                        dot_view.system_bus_io = Some(IoOp::Read(*addr, *value));
                    }
                }
                TraceEvent::CpuWrite { addr, value, .. } => {
                    if self.is_address_filtered(*addr, false) {
                        dot_view.system_bus_io = Some(IoOp::Write(*addr, *value));
                    }
                }
                TraceEvent::CpuDmaRead { addr, value, .. } => {
                    if self.show_dma_io {
                        dot_view.system_bus_io = Some(IoOp::Read(*addr, *value));
                        dot_view.flags.set(DotViewFlags::DMA_READ, true);
                    }
                }
                TraceEvent::CpuDmaWrite { addr, value, .. } => {
                    if self.show_dma_io {
                        dot_view.system_bus_io = Some(IoOp::Write(*addr, *value));
                        dot_view.flags.set(DotViewFlags::DMA_WRITE, true);
                    }
                }
                TraceEvent::CpuInterruptStatus { status, .. } => {
                    if status.contains(CpuInterruptStatus::IRQ_DETECTED_PHI2) {
                        dot_view.flags.set(DotViewFlags::IRQ_DETECT_PHI2, true);
                    }
                    if status.contains(CpuInterruptStatus::NMI_DETECTED_PHI2) {
                        dot_view.flags.set(DotViewFlags::NMI_DETECT_PHI2, true);
                    }
                    if status.contains(CpuInterruptStatus::IRQ_DETECTED_PHI1) {
                        dot_view.flags.set(DotViewFlags::IRQ_DETECT_PHI1, true);
                    }
                    if status.contains(CpuInterruptStatus::NMI_DETECTED_PHI1) {
                        dot_view.flags.set(DotViewFlags::NMI_DETECT_PHI1, true);
                    }
                    if status.contains(CpuInterruptStatus::IRQ_POLLED) {
                        dot_view.flags.set(DotViewFlags::IRQ_POLL, true);
                    }
                    if status.contains(CpuInterruptStatus::NMI_POLLED) {
                        dot_view.flags.set(DotViewFlags::NMI_POLL, true);
                    }
                }
                _ => {}
            }
        }

        (found_next_line, modified_scanline, modified_next_scanline)
    }

    /// Returns: (found-next-line, modified-scanline, modified-next-scanline)
    #[inline(always)]
    fn process_secondary_apu_events_line<C: Fn(usize) -> bool>(
        &mut self,
        nes: &mut Nes,
        cull_line: bool,
        //line: u16,
        dot_horizontal_cull: C,
        show_current_events: bool,
        line_start_index: usize,
        line_start_cpu_clk: u64,
        scanline_view: &mut Vec<DotView>,
        next_scanline_view: &mut Vec<DotView>,
    ) -> (Option<(u64, usize)>, bool, bool) {
        let mut found_next_line = None;
        let mut modified_scanline = false;
        let mut modified_next_scanline = false;

        let events = if show_current_events {
            &nes.apu_mut().debug.trace_events_current
        } else {
            &nes.apu_mut().debug.trace_events_prev
        };
        let slice = &events[line_start_index..];
        //println!("Scanning {} events for line", slice.len());
        for (i, event) in slice.iter().enumerate() {
            if i > 0 {
                if let TraceEvent::CpuClockLineSync { cpu_clk } = event {
                    found_next_line = Some((*cpu_clk, line_start_index + i));
                    //println!("Next line sync found: line = {next_line}, index = {}", line_start_index);
                    break;
                }
            }

            if cull_line {
                // Just scan for the next line sync event
                continue;
            }

            let dot = match event {
                TraceEvent::ApuFrameSeqFrame { clk_lower, .. }
                | TraceEvent::ApuMixerOut { clk_lower, .. }
                | TraceEvent::ApuIrqRaised { clk_lower, .. } => {
                    let mut full_clk = line_start_cpu_clk & 0xffffffff_ffffff00 | *clk_lower as u64;
                    if full_clk < line_start_cpu_clk {
                        // check if there was an overflow in the lower 8 bits
                        full_clk += 256
                    }

                    let clk_line_delta = full_clk - line_start_cpu_clk;
                    if clk_line_delta >= self.line_cpu_to_dot.len() as u64 {
                        log::warn!("CPU clock {} run too far into the future: can't map trace events to scanline dots (would be clock {} for the line)", full_clk, clk_line_delta);
                        continue;
                    }
                    self.line_cpu_to_dot[clk_line_delta as usize]
                }
                _ => {
                    continue;
                }
            } as usize;

            // The same as for PPU events: we need to consider overflow onto the next line
            let (dot, dot_view, is_overflow) = if dot >= 341 {
                let dot = dot % 341;
                (dot, &mut next_scanline_view[dot], true)
            } else {
                (dot, &mut scanline_view[dot], false)
            };

            if dot_horizontal_cull(dot) {
                continue;
            }

            dot_view.visible = true;
            if is_overflow {
                modified_next_scanline = true;
            } else {
                modified_scanline = true;
            }

            dot_view.cpu_clock = self.line_dot_to_cpu[dot];
            //dot_view.line = line;
            //dot_view.dot = dot as u16;
            match event {
                TraceEvent::ApuFrameSeqFrame { status, .. } => {
                    if self.show_apu_sequencer {
                        if status.contains(FrameSequencerStatus::QUARTER_FRAME) {
                            dot_view.flags.set(DotViewFlags::APU_QUARTER_FRAME, true);
                        }
                        if status.contains(FrameSequencerStatus::HALF_FRAME) {
                            dot_view.flags.set(DotViewFlags::APU_HALF_FRAME, true);
                        }
                    }
                }
                TraceEvent::ApuMixerOut {
                    output,
                    square1,
                    square2,
                    triangle,
                    noise,
                    dmc,
                    ..
                } => match self.show_apu_output {
                    ApuOutput::Mixer => {
                        dot_view.apu_output = *output;
                    }
                    ApuOutput::Pulse1 => {
                        dot_view.apu_output = *square1 as f32 / 16.0f32;
                    }
                    ApuOutput::Pulse2 => {
                        dot_view.apu_output = *square2 as f32 / 16.0f32;
                    }
                    ApuOutput::Noise => {
                        dot_view.apu_output = *noise as f32 / 16.0f32;
                    }
                    ApuOutput::Triangle => {
                        dot_view.apu_output = *triangle as f32 / 16.0f32;
                    }
                    ApuOutput::Dmc => {
                        dot_view.apu_output = *dmc as f32 / 128.0f32;
                    }
                    ApuOutput::None => {}
                },
                TraceEvent::ApuIrqRaised { .. } => {
                    dot_view.flags.set(DotViewFlags::APU_IRQ_RAISED, true);
                }
                _ => {}
            }
        }

        (found_next_line, modified_scanline, modified_next_scanline)
    }

    fn draw_dot_view(
        &mut self,
        _nes: &mut Nes,
        _ui: &mut Ui,
        painter: &Painter,
        dot_view: &DotView,
        _line: u16,
        _dot: u16,
        rect: Rect,
    ) {
        let mut _highlight = false;
        if dot_view.system_bus_io.is_some() {
            _highlight = true;
        }
        if self.show_dma_io
            && (dot_view.flags.contains(DotViewFlags::DMA_READ)
                || dot_view.flags.contains(DotViewFlags::DMA_WRITE))
        {
            _highlight = true;
        }

        if self.show_nmi_interrupts
            && (dot_view.flags.contains(DotViewFlags::NMI_RAISED)
                || dot_view.flags.contains(DotViewFlags::NMI_DETECT_PHI2)
                || dot_view.flags.contains(DotViewFlags::NMI_DETECT_PHI1)
                || dot_view.flags.contains(DotViewFlags::NMI_POLL))
        {
            _highlight = true;
        }

        if self.show_irq_interrupts
            && (dot_view.flags.contains(DotViewFlags::APU_IRQ_RAISED)
                || dot_view.flags.contains(DotViewFlags::MAPPER_IRQ_RAISED)
                || dot_view.flags.contains(DotViewFlags::IRQ_DETECT_PHI2)
                || dot_view.flags.contains(DotViewFlags::IRQ_DETECT_PHI1)
                || dot_view.flags.contains(DotViewFlags::IRQ_POLL))
        {
            _highlight = true;
        }
        {
            //let pad_x = rect.width() / 5.0f32;
            //let pad_y = rect.height() / 5.0f32;
            //let inset =
            //    Rect::from_min_max(rect.min + vec2(pad_x, pad_y), rect.max - vec2(pad_x, pad_y));
            //painter.add(egui::Shape::Rect(epaint::RectShape::filled(inset, epaint::Rounding::none(), egui::Color32::LIGHT_GREEN)));
        }
        /*
        if highlight {
            painter.add(egui::Shape::Rect(epaint::RectShape::filled(rect, epaint::Rounding::none(), egui::Color32::YELLOW)));
        }
        */
        //if self.show_apu_output != ApuOutput::None {
        {
            let half_width = rect.width() / 2.0f32;
            let full_output_rect = Rect::from_min_max(rect.min + vec2(half_width, 0.0), rect.max);
            let full_height = rect.height();
            let output_height = full_height * dot_view.apu_output;
            let output_rect = Rect::from_min_max(
                rect.min + vec2(half_width, full_height - output_height),
                rect.max,
            );

            painter.add(egui::Shape::Rect(epaint::RectShape::filled(
                full_output_rect,
                epaint::Rounding::none(),
                egui::Color32::DARK_GRAY,
            )));
            painter.add(egui::Shape::Rect(epaint::RectShape::filled(
                output_rect,
                epaint::Rounding::none(),
                egui::Color32::LIGHT_GREEN,
            )));
        }
    }

    fn draw_viewport(&mut self, nes: &mut Nes, ui: &mut Ui, viewport_rect: Rect) {
        let n_lines = TRACE_EVENTS_DOT_HEIGHT as f32;
        let n_dots_per_line = TRACE_EVENTS_DOT_WIDTH as f32;
        //let line_gap_height = 0.5f32;
        let dot_gap_px = 0.0f32;
        let logical_width = n_dots_per_line + (n_dots_per_line - 1.0f32) * dot_gap_px;
        let req_width = logical_width * self.zoom;
        let logical_height = n_lines * self.line_height + n_lines * self.line_gap_height;
        let req_height = logical_height * self.zoom;
        let (response, painter) = ui.allocate_painter(
            egui::Vec2::new(req_width, req_height),
            egui::Sense::click_and_drag(),
        );

        if ui.rect_contains_pointer(response.rect) {
            let scroll_delta = ui.ctx().input().zoom_delta();
            if scroll_delta > 1.0 {
                self.zoom_in();
            } else if scroll_delta < 1.0 {
                self.zoom_out();
            }
        }

        let _img = egui::Image::new(
            self.screen_texture.id(),
            egui::Vec2::new(FRAME_WIDTH as f32, FRAME_HEIGHT as f32),
        );
        //let response = ui.add(egui::Image::new(self.nametables_texture.id(), egui::Vec2::new(width as f32, height as f32)));
        // TODO(emilk): builder pattern for Mesh

        let allocation_pos = response.rect.left_top();
        let allocation_width = response.rect.width();
        let allocation_height = response.rect.height();
        let alloc_scale_x = allocation_width / logical_width as f32;
        let alloc_scale_y = allocation_height / logical_height as f32;

        let allocation_x_to_nes_px = TRACE_EVENTS_DOT_WIDTH as f32 / allocation_width;
        let allocation_y_to_nes_px = TRACE_EVENTS_DOT_HEIGHT as f32 / allocation_height;
        //let _nes_px_to_allocation = 1.0 / allocation_to_nes_px;

        let screen_width = (FRAME_WIDTH as f32 / TRACE_EVENTS_DOT_WIDTH as f32) * allocation_width;
        let _screen_height =
            (FRAME_HEIGHT as f32 / TRACE_EVENTS_DOT_HEIGHT as f32) * allocation_height;

        let px_width = alloc_scale_x;
        let line_height = self.line_height * alloc_scale_y;
        let line_gap_height = self.line_gap_height * alloc_scale_y;

        //println!("allocation scale y = {alloc_scale_y}, req_height = {req_height} line_height = {line_height}");
        let mut mesh = egui::Mesh::with_texture(self.screen_texture.id());

        let current_ppu_line = nes.ppu_mut().line;
        let _next_ppu_dot = nes.ppu_mut().dot;

        // Draw each scanline of the framebuffer with a gap for room to show debug/event labels
        for line in 0..262 {
            let line_y = line as f32 * (line_height + line_gap_height);

            let rect = egui::Rect::from_min_size(
                allocation_pos + vec2(0.0, line_y),
                vec2(allocation_width, line_height),
            );
            painter.add(egui::Shape::Rect(epaint::RectShape::filled(
                rect,
                epaint::Rounding::none(),
                egui::Color32::GRAY,
            )));

            if let 0..=239 = line {
                let rect = egui::Rect::from_min_size(
                    allocation_pos + vec2(0.0, line_y),
                    vec2(screen_width, line_height),
                );

                let uv_line_top = line as f32 / 240.0f32;
                let uv_line_bottom = (line as f32 + 1.0f32) / 240.0f32;
                let uv_rect =
                    egui::Rect::from_min_max(pos2(0.0, uv_line_top), pos2(1.0, uv_line_bottom));

                if line < current_ppu_line {
                    mesh.add_rect_with_uv(rect, uv_rect, egui::Color32::WHITE);
                } else if line == current_ppu_line {
                    mesh.add_rect_with_uv(rect, uv_rect, egui::Color32::WHITE);
                } else {
                    mesh.add_rect_with_uv(rect, uv_rect, egui::Color32::LIGHT_GRAY);
                }
            }
        }
        painter.add(egui::Shape::mesh(mesh));

        let show_current_events = if self.paused { true } else { false };

        // Returns true if the dot isn't within the horizontal span of the viewport
        let dot_horizontal_cull = |dot: usize| -> bool {
            let dot_x_gap_min = px_width * dot as f32;
            if dot_x_gap_min > viewport_rect.max.x {
                return true;
            }
            let dot_x_gap_max = px_width * (dot + 1) as f32;
            if dot_x_gap_max < viewport_rect.min.x {
                return true;
            }
            false
        };

        //let mut dot_render_count = 0;

        //let cpu_to_ppu_mapper = nes.cpu_to_ppu_clock_mapper();
        let ppu_to_cpu_mapper = nes.ppu_to_cpu_clock_mapper();
        let mut next_line = None;

        //let mut line_start_cpu_clk = 0;
        let mut ppu_line_start_index = 0;
        let mut apu_line_start_index = 0;

        // Find the first line sync events (when the emulator starts then the line sync might not be the first event)
        {
            let ppu_events = if show_current_events {
                &nes.ppu_mut().debug.trace_events_current
            } else {
                &nes.ppu_mut().debug.trace_events_prev
            };
            //println!("Event trace buffer len = {}", events.len());

            for (i, event) in ppu_events[0..].iter().enumerate() {
                if let TraceEvent::PpuCpuLineSync {
                    cpu_clk,
                    ppu_clk: _,
                    line,
                } = event
                {
                    next_line = Some((*line, *cpu_clk));
                    ppu_line_start_index = i;
                    break;
                }
            }

            if let Some((_, first_line_start_clk)) = next_line {
                let mut found = false;
                let apu_events = if show_current_events {
                    &nes.apu_mut().debug.trace_events_current
                } else {
                    &nes.apu_mut().debug.trace_events_prev
                };
                for (i, event) in apu_events[0..].iter().enumerate() {
                    if let TraceEvent::CpuClockLineSync { cpu_clk } = event {
                        if *cpu_clk == first_line_start_clk {
                            found = true;
                            apu_line_start_index = i;
                            break;
                        }
                    }
                }
                if !found {
                    log::error!("Can't handle inconsistent APU trace events");
                    return;
                }
            }
        }
        //println!("Initial scan for line sync found line {next_line} at index = {}", line_start_index);

        // We're careful to avoid redundant clears of these scanline view buffers
        let mut scanline_view_is_clear = false;
        let mut next_scanline_view_is_clear = false;
        // No need to clear anything if we haven't found a single line of trace events
        if next_line.is_some() {
            self.next_scanline_view = vec![DotView::default(); TRACE_EVENTS_DOT_WIDTH];
            next_scanline_view_is_clear = true;
        }

        // We draw one line at a time so we can gather all the per-dot data, with culling before
        // drawing each dot.
        //
        // One of the thorny details to keep in mind that each scanline may have some number of
        // 'overflow' events for CPU clocks that actually relate to the next scanline (which is
        // why we manage two scanline_view buffers). The overflow events can happen because the
        // CPU can get briefly ahead of the CPU clock, but line boundaries are only determined
        // after the completion of a PPU clock cycle.
        //
        while next_line.is_some() {
            // Inherit the state of next_scanline_view from the previous line (which may contain overflow
            // state from the previous line) and then make sure next_scanline_view is clear for any
            // further overflow state
            std::mem::swap(&mut self.scanline_view, &mut self.next_scanline_view);
            std::mem::swap(
                &mut scanline_view_is_clear,
                &mut next_scanline_view_is_clear,
            );
            if !next_scanline_view_is_clear {
                self.next_scanline_view = vec![DotView::default(); TRACE_EVENTS_DOT_WIDTH];
                next_scanline_view_is_clear = true;
            }

            let (current_line, line_start_cpu_clk) = next_line.unwrap();
            let line_y_gap_min =
                current_line as f32 * (line_height + line_gap_height) + line_height;
            let line_y_gap_max = line_y_gap_min + line_gap_height;

            // Determine if this line is going to be culled, though we don't jump ahead
            // immediately since we still have to find the next line start even if we
            // aren't rendering this line
            let cull_line =
                if line_y_gap_max < viewport_rect.min.y || line_y_gap_min > viewport_rect.max.y {
                    //println!("Culling line {current_line} line");
                    true
                } else {
                    false
                };

            // Temporarily pluck the scanline_view and next_scanline_view vectors out of self
            // so we can access self state while also updating view state
            {
                let mut borrowed_scanline_view = vec![];
                let mut borrowed_next_scanline_view = vec![];
                std::mem::swap(&mut borrowed_scanline_view, &mut self.scanline_view);
                std::mem::swap(
                    &mut borrowed_next_scanline_view,
                    &mut self.next_scanline_view,
                );

                let (found_next_line, modified_scanline, modified_next_scanline) = self
                    .process_main_events_line(
                        nes,
                        cull_line,
                        &dot_horizontal_cull,
                        show_current_events,
                        &ppu_to_cpu_mapper,
                        ppu_line_start_index,
                        line_start_cpu_clk,
                        &mut borrowed_scanline_view,
                        &mut borrowed_next_scanline_view,
                    );
                if let Some((next_line_clk_start, line, next_line_start_index)) = found_next_line {
                    next_line = Some((line, next_line_clk_start));
                    ppu_line_start_index = next_line_start_index;
                } else {
                    next_line = None;
                }

                // Avoid redundant clears of the scanline view buffers
                if modified_scanline {
                    scanline_view_is_clear = false;
                }
                if modified_next_scanline {
                    next_scanline_view_is_clear = false;
                }

                // Next scan through APU events up until the next line sync (and check it's consistent with the main trace events)
                // For secondary events we can re-use the line_cpu_to_dot mapping we set up above
                let (found_next_apu_line, modified_scanline, modified_next_scanline) = self
                    .process_secondary_apu_events_line(
                        nes,
                        cull_line,
                        &dot_horizontal_cull,
                        show_current_events,
                        apu_line_start_index,
                        line_start_cpu_clk,
                        &mut borrowed_scanline_view,
                        &mut borrowed_next_scanline_view,
                    );
                if let Some((next_line_clk_start, next_line_start_index)) = found_next_apu_line {
                    #[cfg(debug_assertions)]
                    {
                        if let Some((_, clk)) = next_line {
                            debug_assert_eq!(clk, next_line_clk_start);
                        }
                    }
                    apu_line_start_index = next_line_start_index;
                } else {
                    debug_assert!(next_line.is_none());
                }

                // Avoid redundant clears of the scanline view buffers
                if modified_scanline {
                    scanline_view_is_clear = false;
                }
                if modified_next_scanline {
                    next_scanline_view_is_clear = false;
                }

                std::mem::swap(&mut borrowed_scanline_view, &mut self.scanline_view);
                std::mem::swap(
                    &mut borrowed_next_scanline_view,
                    &mut self.next_scanline_view,
                );
            }

            if cull_line {
                // The process_ methods shouldn't touch the scanline_view buffers if the line is being culled
                // so we at least expect that the next_scanline_view is still clear.
                //
                // NB: The current scanline_view may contain overflow state from the previous line still so may not
                // be clear even though the process_ methods shouldn't have updated it.
                debug_assert_eq!(next_scanline_view_is_clear, true);

                // HACK: disabled culling
                //continue; // Skip the rendering
            }

            //println!("Rendering line {current_line}");

            let mut borrowed_view = vec![];
            std::mem::swap(&mut borrowed_view, &mut self.scanline_view);
            for (i, dot) in borrowed_view.iter().enumerate() {
                if !dot.visible {
                    // HACK: disabled culling
                    //continue;
                }
                //dot_render_count += 1;

                let dot_x_gap_min = px_width * i as f32;
                let dot_x_gap_max = px_width * (i + 1) as f32;
                let rect = Rect::from_min_max(
                    allocation_pos + vec2(dot_x_gap_min, line_y_gap_min),
                    allocation_pos + vec2(dot_x_gap_max, line_y_gap_max),
                );
                self.draw_dot_view(nes, ui, &painter, dot, current_line, i as u16, rect);
            }
            std::mem::swap(&mut borrowed_view, &mut self.scanline_view);
        }

        //println!("Rendered {dot_render_count} dots");

        // Draw a line to represent the line that the PPU is currently processing
        let ppu_line = nes.ppu_mut().line;
        let ppu_line_y = ppu_line as f32 * (line_height + line_gap_height);
        let ppu_dot_x =
            allocation_width * (nes.ppu_mut().dot as f32 / TRACE_EVENTS_DOT_WIDTH as f32);
        painter.line_segment(
            [
                allocation_pos + vec2(0.0f32, ppu_line_y),
                allocation_pos + vec2(ppu_dot_x, ppu_line_y),
            ],
            (1.0f32, egui::Color32::YELLOW),
        );

        if let Some(hover_pos) = response.hover_pos() {
            let x = ((hover_pos.x - allocation_pos.x) * allocation_x_to_nes_px) as usize;
            let y = ((hover_pos.y - allocation_pos.y) * allocation_y_to_nes_px) as usize;
            self.hover_pos = [x, y];

            /*
            let tile_x =
            painter.rect_stroke(
                egui::Rect::from_min_size(response.rect.min + vec2(x_off as f32 * nes_px_to_img, y_off as f32 * nes_px_to_img),
                                            vec2(self.fb_width as f32 * nes_px_to_img, self.fb_height as f32 * nes_px_to_img)),
                egui::Rounding::none(),
                egui::Stroke::new(1.0, Color32::RED));
                */
            //painter.rect_filled(egui::Rect::from_min_size(response.rect.min, vec2(200.0, 200.0)),
            //    egui::Rounding::none(), Color32::RED);
        }
    }

    pub fn draw_left_sidebar(&mut self, _nes: &mut Nes, ui: &mut Ui) {
        // show_irq_interrupts: bool,
        // show_nmi_interrupts: bool,
        // show_dma_io: bool,
        // show_ppu_register_io: bool,
        // show_apu_register_io: bool,
        // show_mapper_register_io: bool,

        ui.toggle_value(&mut self.show_irq_interrupts, "IRQs");
        ui.toggle_value(&mut self.show_nmi_interrupts, "NMIs");
        ui.toggle_value(&mut self.show_ppu_register_io, "PPU Register IO");
        ui.toggle_value(&mut self.show_apu_register_io, "APU Register IO");
        //ui.toggle_value(&mut self.show_dma_io, "Mapper Register IO");
        ui.toggle_value(&mut self.show_dma_io, "DMAs");
        ui.toggle_value(&mut self.show_apu_sequencer, "APU Sequencer");
        //ui.toggle_value(&mut self.show_apu_output, "APU Output");
        //if self.show_apu_output {
        egui::ComboBox::from_label("APU Output")
            .selected_text(format!("{:?}", self.show_apu_output))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.show_apu_output, ApuOutput::None, "None");
                ui.selectable_value(&mut self.show_apu_output, ApuOutput::Mixer, "Mixer");
                ui.selectable_value(&mut self.show_apu_output, ApuOutput::Pulse1, "Pulse 1");
                ui.selectable_value(&mut self.show_apu_output, ApuOutput::Pulse2, "Pulse 2");
                ui.selectable_value(&mut self.show_apu_output, ApuOutput::Noise, "Noise");
                ui.selectable_value(&mut self.show_apu_output, ApuOutput::Triangle, "Triangle");
                ui.selectable_value(&mut self.show_apu_output, ApuOutput::Dmc, "Dmc");
            });
        //}
    }

    pub fn draw_right_sidebar(&mut self, _nes: &mut Nes, _ui: &mut Ui) {}

    pub fn draw(&mut self, nes: &mut Nes, ctx: &egui::Context) {
        if self.queue_screen_fb_upload {
            let _hook_state = self.hook_state.borrow();
            let copy =
                full_framebuffer_image_delta(&self.hook_state.borrow().screen_framebuffer_front);
            ctx.tex_manager()
                .write()
                .set(self.screen_texture.id(), copy);
            self.queue_screen_fb_upload = false;
        }
        egui::Window::new("Trace Events")
            .default_width(900.0)
            .resizable(true)
            //.resize(|r| r.auto_sized())
            .show(ctx, |ui| {
                let panels_width = ui.fonts().pixels_per_point() * 100.0;

                egui::SidePanel::left("trace_events_options_panel")
                    .resizable(false)
                    .min_width(panels_width)
                    .show_inside(ui, |ui| {
                        self.draw_left_sidebar(nes, ui);
                        //ui.checkbox(&mut view.show_scroll, "Show Scroll Position");
                    });
                egui::SidePanel::right("trace_events_properties_panel")
                    .resizable(false)
                    .min_width(panels_width)
                    .show_inside(ui, |ui| {
                        //ui.label(format!("Scroll X: {}", self.nes.system_ppu().scroll_x()));
                        //ui.label(format!("Scroll Y: {}", self.nes.system_ppu().scroll_y()));
                        self.draw_right_sidebar(nes, ui);
                    });

                egui::TopBottomPanel::bottom("trace_events_footer").show_inside(ui, |ui| {
                    ui.label(format!("[{}, {}]", self.hover_pos[0], self.hover_pos[1]));
                });

                //let frame = Frame::none().outer_margin(Margin::same(200.0));
                egui::CentralPanel::default()
                    //.frame(frame)
                    .show_inside(ui, |ui| {
                        egui::ScrollArea::both().show_viewport(ui, |ui, viewport_rect| {
                            self.draw_viewport(nes, ui, viewport_rect);
                        });
                        /*
                        let x_off = self.nes.system_ppu().scroll_x();
                        let y_off = self.nes.system_ppu().scroll_y();
                        painter.rect_stroke(
                            egui::Rect::from_min_size(response.rect.min + vec2(x_off as f32 * nes_px_to_img, y_off as f32 * nes_px_to_img),
                                                        vec2(self.fb_width as f32 * nes_px_to_img, self.fb_height as f32 * nes_px_to_img)),
                            egui::Rounding::none(),
                            egui::Stroke::new(2.0, Color32::YELLOW));
                            */
                    });
            });
    }
}
