use crate::trace::{TraceBuffer, TraceEvent};


#[derive(Clone, Default)]
pub struct Mixer {
    pub square1_muted: bool,
    pub square2_muted: bool,
    pub triangle_muted: bool,
    pub noise_muted: bool,
    pub dmc_muted: bool,
}

impl Mixer {
    pub fn new() -> Self {
        Self {
            ..Default::default()
            /*
            square1_muted: false,
            square2_muted: false,
            triangle_muted: false,
            noise_muted: false,
            dmc_muted: false,
            */
        }
    }

    pub fn power_cycle(&mut self) {
        *self = Self::new();
    }

    pub fn mix(
        &self,
        square1_channel: u8,
        square2_channel: u8,
        triangle_channel: u8,
        noise_channel: u8,
        dmc_channel: u8,
        clock: u64,
        trace: &mut TraceBuffer
    ) -> f32 {
        debug_assert!(square1_channel < 16);
        debug_assert!(square2_channel < 16);
        debug_assert!(triangle_channel < 16);
        debug_assert!(noise_channel < 16);
        debug_assert!(dmc_channel < 128);

        //if square1_channel != 0 {
        //    println!("Mixer: square 1 input = {square1_channel}");
        //    println!("Mixer: square 2 input = {square2_channel}");
        //}

        let square1_channel = if !self.square1_muted { square1_channel } else { 0 };
        let square2_channel = if !self.square2_muted { square2_channel } else { 0 };
        let triangle_channel = if !self.triangle_muted { triangle_channel } else { 0 };
        let noise_channel = if !self.noise_muted { noise_channel } else { 0 };
        let dmc_channel = if !self.dmc_muted { dmc_channel } else { 0 };

        // DAC Output formula from https://www.nesdev.com/apu_ref.txt

        let square_denominator = (square1_channel + square2_channel) as f64;
        let square_out = if square_denominator == 0.0f64 {
            0f64
        } else {
            95.88f64 / ((8128.0f64 / (square_denominator)) + 100.0f64)
        };
        //if square_out != 0.0 {
        //    println!("Square output {square_out}");
        //}

        let tnd_denominator = (triangle_channel as f64 / 8227.0f64) + (noise_channel as f64 / 12241.0f64) + (dmc_channel as f64 / 22638.0f64);
        let tnd_out = if tnd_denominator == 0.0f64 {
            0.0f64
        } else {
            159.79f64 / ((1.0f64 / tnd_denominator) + 100.0f64)
        };
        //if tnd_out != 0.0 {
        //    println!("TND channels output {tnd_out}");
        //}
        //let sample = ((square_out + tnd_out) - 0.5) * 2.0;
        let sample = square_out + tnd_out;

        #[cfg(feature="trace-events")]
        {
            trace.push(TraceEvent::ApuMixerOut {
                clk_lower: (clock & 0xff) as u8,
                output: sample as f32,
                square1: square1_channel,
                square2: square2_channel,
                triangle: triangle_channel,
                noise: noise_channel,
                dmc: dmc_channel
            })
        }

        //if sample != 0.0 {
        //    println!("Mixer output {sample}");
        //}
        sample as f32

        //(sample * (i16::MAX as f64)) as i16
    }

    pub fn set_square1_muted(&mut self, enabled: bool) {
        self.square1_muted = enabled;
    }
    pub fn set_square2_muted(&mut self, enabled: bool) {
        self.square2_muted = enabled;
    }
    pub fn set_triangle_muted(&mut self, enabled: bool) {
        self.triangle_muted = enabled;
    }
    pub fn set_noise_muted(&mut self, enabled: bool) {
        self.noise_muted = enabled;
    }
    pub fn set_dmc_muted(&mut self, enabled: bool) {
        self.dmc_muted = enabled;
    }
}

