use std::time::{Instant, Duration};
use nes_emulator::nes::Nes;

pub struct BenchmarkState {
    nes_cpu_clock_hz: u64,
    stats_update_period: Duration,

    update_start: Instant,
    update_start_clock: u64,

    last_stats_update_timestamp: Instant,
    last_stats_update_frame_no: u32,
    last_stats_update_cpu_clock: u64,
    last_frame_time: Instant,

    /// Measured from last update()
    profiled_last_clocks_per_second: u32,
    /// Measure over stats update period
    profiled_aggregate_clocks_per_second: u32,

    /// Extrapolated from last frame duration
    profiled_last_fps: f32,
    /// Measured over stats update period
    profiled_aggregate_fps: f32,

    /// emulated frames (not drawn frames)
    pub frame_count: u32,
}

impl BenchmarkState {
    pub fn new(nes: &Nes, stats_update_period: Duration) -> Self {
        let now = Instant::now();
        Self {
            nes_cpu_clock_hz: nes.cpu_clock_hz(),
            stats_update_period,

            update_start: now,
            update_start_clock: 0,

            last_stats_update_timestamp: now,
            last_stats_update_frame_no: 0,
            last_stats_update_cpu_clock: 0,
            last_frame_time: now,

            profiled_last_clocks_per_second: 0,
            profiled_aggregate_clocks_per_second: 0,

            profiled_last_fps: 0.0,
            profiled_aggregate_fps: 0.0,

            frame_count: 0
        }
    }

    fn real_time_emulation_speed(&self) -> f32 {
        self.profiled_last_clocks_per_second as f32 / self.nes_cpu_clock_hz as f32
    }

    fn aggregated_emulation_speed(&self) -> f32 {
        self.profiled_aggregate_clocks_per_second as f32 / self.nes_cpu_clock_hz as f32
    }

    pub fn estimated_cpu_clocks_for_duration(&self, duration: Duration) -> u64 {
        if self.profiled_last_clocks_per_second > 0 {
            (self.profiled_last_clocks_per_second as f64 * duration.as_secs_f64()) as u64
        } else {
            (self.nes_cpu_clock_hz as f64 * duration.as_secs_f64()) as u64
        }
    }

    pub fn estimate_duration_for_cpu_clocks(&self, cpu_clocks: u64) -> Duration {
        if self.profiled_last_clocks_per_second > 0 {
            Duration::from_secs_f64(cpu_clocks as f64 / self.profiled_last_clocks_per_second as f64)
        } else {
            Duration::from_secs_f64(cpu_clocks as f64 / self.nes_cpu_clock_hz as f64)
        }
    }

    pub fn start_update(&mut self, nes: &Nes, now: Instant) {
        self.update_start = now;
        self.update_start_clock = nes.cpu_clock();
    }

    pub fn end_update(&mut self, nes: &Nes) {
        let cpu_clock = nes.cpu_clock();
        if cpu_clock < self.last_stats_update_cpu_clock {
            log::warn!("Resetting benchmark stats after emulator clock went backwards");
            *self = BenchmarkState::new(nes, self.stats_update_period);
        }
        let elapsed = Instant::now() - self.update_start;
        let clocks_elapsed = cpu_clock - self.update_start_clock;
        // Try to avoid updating last_clocks_per_second for early exit conditions where we didn't actually do any work
        if elapsed > Duration::from_millis(1) || clocks_elapsed > 2000 {
            self.profiled_last_clocks_per_second = (clocks_elapsed as f64 / elapsed.as_secs_f64()) as u32;
        }
        let now = Instant::now();
        let stats_update_duration = now - self.last_stats_update_timestamp;
        if stats_update_duration > self.stats_update_period {
            let n_frames = self.frame_count - self.last_stats_update_frame_no;
            let aggregate_fps = (n_frames as f64 / stats_update_duration.as_secs_f64()) as f32;

            let n_clocks = cpu_clock - self.last_stats_update_cpu_clock;
            let aggregate_cps = (n_clocks as f64 / stats_update_duration.as_secs_f64()) as u32;

            let aggregate_speed = (self.aggregated_emulation_speed() * 100.0) as u32;
            log::debug!("Aggregate Emulator Stats: Clocks/s: {aggregate_cps:8}, Update FPS: {aggregate_fps:4.2}, Real-time Speed: {aggregate_speed:3}%");

            let last_fps = self.profiled_last_fps;
            let last_cps = self.profiled_last_clocks_per_second;
            let latest_speed = (self.real_time_emulation_speed() * 100.0) as u32;
            log::debug!("Raw Emulator Stats:       Clocks/s: {last_cps:8}, Update FPS: {last_fps:4.2}, Real-time Speed: {latest_speed:3}%");

            self.last_stats_update_timestamp = now;
            self.last_stats_update_frame_no = self.frame_count;
            self.last_stats_update_cpu_clock = cpu_clock;
            self.profiled_aggregate_fps = aggregate_fps as f32;
            self.profiled_aggregate_clocks_per_second = aggregate_cps;
        }
    }

    pub fn end_frame(&mut self) {
        let now = Instant::now();
        let frame_duration = now - self.last_frame_time;
        self.profiled_last_fps = (1.0 as f64 / frame_duration.as_secs_f64()) as f32;
        self.last_frame_time = now;
        self.frame_count += 1;
    }
}