use std::{
    ops::{Index, IndexMut},
    slice::SliceIndex,
};

use bitflags::bitflags;

use crate::apu::channel::frame_sequencer::FrameSequencerStatus;

bitflags! {
    #[derive(Default)]
    pub struct CpuInterruptStatus: u32 {
        const IRQ_DETECTED_PHI2 = 1<<0;
        const NMI_DETECTED_PHI2 = 1<<1;

        const IRQ_DETECTED_PHI1 = 1<<2;
        const NMI_DETECTED_PHI1 = 1<<3;

        const IRQ_POLLED = 1<<4;
        const NMI_POLLED = 1<<5;
        const BRK_POLLED = 1<<6;
    }
}

#[cfg(feature = "trace-events")]
#[derive(Default)]
pub struct TraceBuffer {
    buf: Vec<TraceEvent>,
}
// Have any TraceBuffer arguments and API usage optimize away if "trace-events not enabled"
#[cfg(not(feature = "trace-events"))]
pub struct TraceBuffer;
impl TraceBuffer {
    #[inline(always)]
    pub fn clear(&mut self) {
        #[cfg(feature = "trace-events")]
        self.buf.clear();
    }
    #[inline(always)]
    pub fn push(&mut self, event: TraceEvent) {
        #[cfg(feature = "trace-events")]
        self.buf.push(event);
    }
}
/*
#[cfg(feature="trace-events")]
impl Index<usize> for TraceBuffer {
    type Output = TraceEvent;

    fn index(&self, index: usize) -> &Self::Output {
        #[cfg(feature="unsafe-opt")]
        {
            unsafe { self.buf.get_unchecked(index) }
        }
        #[cfg(not(feature="unsafe-opt"))]
        {
            &self.buf[index]
        }
    }
}*/

#[cfg(feature = "trace-events")]
impl<I: SliceIndex<[TraceEvent]>> Index<I> for TraceBuffer {
    type Output = I::Output;

    #[inline(always)]
    fn index(&self, index: I) -> &Self::Output {
        #[cfg(feature = "unsafe-opt")]
        {
            unsafe { self.buf.get_unchecked(index) }
        }
        #[cfg(not(feature = "unsafe-opt"))]
        {
            &self.buf[index]
        }
    }
}

#[cfg(feature = "trace-events")]
impl IndexMut<usize> for TraceBuffer {
    #[inline(always)]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        #[cfg(feature = "unsafe-opt")]
        {
            unsafe { self.buf.get_unchecked_mut(index) }
        }
        #[cfg(not(feature = "unsafe-opt"))]
        {
            &mut self.buf[index]
        }
    }
}

#[derive(Debug, Clone)]
pub enum TraceEvent {
    /// At the start of each scanline we record the current CPU clock and PPU
    /// clock for aligning CPU + PPU data in debug views
    ///
    /// Other buffers of events (from the APU and mappers) are treated as secondary
    /// to the PPU trace events. Once we know the upper/lower clock bounds for a
    /// line then secondary buffers can be consumed up to the known end-of-line.
    ///
    /// Since these sync events have full 64bit clocks that means that all other
    /// events only need the least-significant 16bits of the cpu clock - or
    /// a 16bit dot.
    PpuCpuLineSync {
        cpu_clk: u64,
        ppu_clk: u64,
        line: u16,
    },

    // For the APU and mappers to track separate buffers of trace events they
    // write a 64bit CPU clock at the start of each line so that they can write
    // events with just the lower 8bits of the cpu clock. The full clock
    // lets us account for any overflow of the lower bits within a single line.
    CpuClockLineSync {
        cpu_clk: u64,
    },

    ApuFrameSeqFrame {
        clk_lower: u8,
        status: FrameSequencerStatus,
    },
    ApuSquareOut {
        clk_lower: u8,
        index: u8,
        envelope: u8,
        sweep_count: u8,
        timer: u8,
        output: u8,
    },
    ApuNoiseOut {
        clk_lower: u8,
        envelope: u8,
        sweep_count: u8,
        timer: u8,
        output: u8,
    },
    ApuMixerOut {
        clk_lower: u8,
        output: f32,
        square1: u8,
        square2: u8,
        triangle: u8,
        noise: u8,
        dmc: u8,
    },

    CpuRead {
        clk_lower: u8,
        addr: u16,
        value: u8,
    },
    CpuWrite {
        clk_lower: u8,
        addr: u16,
        value: u8,
    },
    CpuDmaRead {
        clk_lower: u8,
        addr: u16,
        value: u8,
    },
    CpuDmaWrite {
        clk_lower: u8,
        addr: u16,
        value: u8,
    },

    PpuRead {
        dot: u16,
        addr: u16,
        value: u8,
    },
    PpuWrite {
        dot: u16,
        addr: u16,
        value: u8,
    },
    PpuReadPalette {
        dot: u16,
        off: u8,
        value: u8,
    },
    PpuReadOam {
        dot: u16,
        off: u8,
        value: u8,
    },
    PpuWriteOam {
        dot: u16,
        off: u8,
        value: u8,
    },
    PpuReadSecondaryOam {
        dot: u16,
        off: u8,
        value: u8,
    },
    PpuWriteSecondaryOam {
        dot: u16,
        off: u8,
        value: u8,
    },

    CpuInterruptStatus {
        clk_lower: u8,
        status: CpuInterruptStatus,
    },
    CartridgeIrqRaised {
        dot: u16,
        off: u8,
    },
    ApuIrqRaised {
        clk_lower: u8,
        line: u16,
        dot: u16,
        off: u8,
    },
    PpuNmiRaised {
        dot: u16,
        off: u8,
    },
}
