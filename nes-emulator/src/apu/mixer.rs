
pub struct Mixer {
    square1_enabled: bool,
    square2_enabled: bool,
    triangle_enabled: bool,
    noise_enabled: bool,
    dmc_enabled: bool,
}

impl Mixer {
    pub fn new() -> Self {

        Mixer {
            square1_enabled: false,
            square2_enabled: false,
            triangle_enabled: false,
            noise_enabled: false,
            dmc_enabled: false,
        }
    }

    pub fn mix(
        &self,
        square1_channel: u8,
        square2_channel: u8,
        triangle_channel: u8,
        noise_channel: u8,
        dmc_channel: u8
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

        //if sample != 0.0 {
        //    println!("Mixer output {sample}");
        //}
        sample as f32

        //(sample * (i16::MAX as f64)) as i16
    }

    fn set_square1_enabled(&mut self, enabled: bool) {
        self.square1_enabled = enabled;
    }
    fn set_square2_enabled(&mut self, enabled: bool) {
        self.square2_enabled = enabled;
    }
    fn set_triangle_enabled(&mut self, enabled: bool) {
        self.triangle_enabled = enabled;
    }
    fn set_noise_enabled(&mut self, enabled: bool) {
        self.noise_enabled = enabled;
    }
    fn set_dmc_enabled(&mut self, enabled: bool) {
        self.dmc_enabled = enabled;
    }
}

