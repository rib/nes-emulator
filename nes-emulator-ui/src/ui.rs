// There's some kind of compiler bug going on, causing a crazy amount of false
// positives atm :(
#![allow(dead_code)]

use std::{collections::{HashSet, HashMap, VecDeque}, fmt::Debug, time::{Instant, Duration}, path::{Path, PathBuf}, fs::File, io::Read, num::NonZeroUsize, u16::MAX};

use egui::{
    Rect, Pos2, Vec2, pos2, vec2, Shape, Image
};
use egui_extras::Table;
use log::{error, warn, info, debug, trace};
use clap::Parser;

use anyhow::anyhow;
use anyhow::Result;

use winit::event::{Event, WindowEvent, VirtualKeyCode};

use egui::{self, RichText, Color32, Ui, ImageData, TextureHandle, Frame, style::Margin};
use egui::{ColorImage, epaint::ImageDelta};

use cpal::SampleRate;
use cpal::traits::StreamTrait;
use cpal::{traits::{HostTrait, DeviceTrait}, OutputCallbackInfo, SampleFormat, Sample};

use ring_channel::{ring_channel, TryRecvError, RingReceiver, RingSender};

use nes_emulator::prelude::*;


const PPU_EVENTS_FB_WIDTH: usize = 341;
const PPU_EVENTS_FB_HEIGHT: usize = 262;


pub enum Status {
    Ok,
    Quit
}

struct Notice {
    level: log::Level,
    text: String,
    timestamp: Instant
}

const NOTICE_TIMEOUT_SECS: u64 = 7;

fn get_file_as_byte_vec(filename: impl AsRef<Path>) -> Vec<u8> {
    //println!("Loading {}", filename);
    let mut f = File::open(&filename).expect("no file found");
    let metadata = std::fs::metadata(&filename).expect("unable to read metadata");
    let mut buffer = vec![0; metadata.len() as usize];
    f.read(&mut buffer).expect("buffer overflow");

    buffer
}

fn epoch_timestamp() -> u64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(n) => n.as_secs(),
        Err(_) => panic!("SystemTime before UNIX EPOCH!"),
    }
}

unsafe fn u8_slice_as_color32_slice(u8_data: &[u8]) -> &[egui::Color32] {
    debug_assert!(u8_data.len() % 4 == 0);
    std::slice::from_raw_parts::<egui::Color32>(u8_data.as_ptr() as *const egui::Color32, u8_data.len() / 4)
}

fn sample_next(o: &mut SampleRequestOptions) -> f32 {
    o.tick();
    o.tone(440.) * 0.1 + o.tone(880.) * 0.1
    // combination of several tones
}

pub struct SampleRequestOptions {
    pub sample_rate: f32,
    pub sample_clock: f32,
    pub nchannels: usize,
}

impl SampleRequestOptions {
    fn tone(&self, freq: f32) -> f32 {
        (self.sample_clock * freq * 2.0 * std::f32::consts::PI / self.sample_rate).sin()
    }
    fn tick(&mut self) {
        self.sample_clock = (self.sample_clock + 1.0) % self.sample_rate;
    }
}

fn generate_debug_sample(o: &mut SampleRequestOptions) -> f32 {
    o.tick();
    o.tone(440.) * 0.1 + o.tone(880.) * 0.1
    // combination of several tones
}

fn generate_debug_samples<T>(output: &mut [T], request: &mut SampleRequestOptions)
where
    T: cpal::Sample
{
    for frame in output.chunks_mut(request.nchannels) {
        let value: T = cpal::Sample::from::<f32>(&generate_debug_sample(request));
        for sample in frame.iter_mut() {
            *sample = value;
        }
    }
}

const DEBUG_CLOCK_DIV: usize = 1;

fn read_audio_samples<T: Sample + Send + Debug>(rx: &mut RingReceiver<f32>, sampler_state: &mut EmulatorAudioState<T>, nchannels: usize, output: &mut [T], info: &OutputCallbackInfo) {
    for frame in output.chunks_mut(nchannels * DEBUG_CLOCK_DIV as usize) {
        let value: T = match rx.try_recv() {
            Err(TryRecvError::Empty) => {
                //warn!("Audio underflow!");
                sampler_state.last_sample
            },
            Err(err) => {
                error!("Audio stream error: {err}");
                sampler_state.last_sample
            }
            Ok(s) => {
                sampler_state.last_sample = Sample::from::<f32>(&s);
                sampler_state.last_sample
                //if s != 0.0 {
                //    println!("emulator: audio sample = {s}");
                //}
            }
        };
        for sample in frame.iter_mut() {
            //warn!("sample = {value:?}");
            *sample = value;
        }
    }
}

struct EmulatorAudioState<T: Sample + Send + Debug + 'static> {
    last_sample: T
}
fn make_audio_stream<T: Sample + Send + Debug + 'static>(
                        device: &cpal::Device,
                        config: &cpal::StreamConfig,
                        mut rx: RingReceiver<f32>) -> Result<cpal::Stream, anyhow::Error>
{
    let mut debug_options = SampleRequestOptions {
        sample_rate: config.sample_rate.0 as f32,
        sample_clock: 0f32,
        nchannels: config.channels as usize
    };
    let nchannels = config.channels as usize;

    let mut sampler_state = EmulatorAudioState::<T> {
        last_sample: Sample::from::<f32>(&0.0f32)
    };

    Ok(device.build_output_stream(config, move |output, info| {
            //generate_debug_samples::<T>(output, &mut debug_options);
            read_audio_samples::<T>(&mut rx, &mut sampler_state, nchannels, output, info)
        },
        |err| {
            error!("Audio stream failure: {err:?}");
        })?)
}

#[derive(PartialEq, Eq)]
enum AddressSpace {
    System,
    Ppu,
    Oam,
}
impl Debug for AddressSpace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::System => write!(f, "System Bus"),
            Self::Ppu => write!(f, "PPU Bus"),
            Self::Oam => write!(f, "OAM Memory"),
        }
    }
}
pub struct EmulatorUi {
    real_time: bool,

    notices: VecDeque<Notice>,

    audio_device: cpal::Device,
    audio_sample_rate: u32,
    //audio_config: cpal::SupportedStreamConfig
    audio_tx: RingSender<f32>,
    audio_stream: cpal::Stream,

    nes: Nes,

    pub paused: bool,
    single_step: bool,

    fb_width: usize,
    fb_height: usize,
    //framebuffers: [Framebuffer; 2],
    //front_framebuffer: usize,
    //back_framebuffer: usize,
    front_framebuffer: Framebuffer,
    framebuffer_texture: TextureHandle,
    queue_framebuffer_upload: bool,
    last_frame_time: Instant,


    show_nametables: bool,
    nametables_framebuffer: Vec<u8>,
    nametables_texture: TextureHandle,
    // Pixel coordinate within four namespace regions
    nametables_hover_pos: [usize; 2],
    nametables_show_scroll: bool,
    queue_nametable_fb_upload: bool,

    show_memview: bool,
    memview_selected_space: AddressSpace,

    show_ppu_events: bool,
    ppu_events_framebuffer: Vec<u8>,
    ppu_events_texture: TextureHandle,
    // Pixel coordinate within four namespace regions
    ppu_events_hover_pos: [usize; 2],
    queue_ppu_events_fb_upload: bool,

    frame_no: u32, // emulated frames (not drawn frames)

    stats_update_period: Duration,
    last_stats_update_timestamp: Instant,
    last_stats_update_frame_no: u32,
    last_stats_update_cpu_clock: u64,

    profiled_last_clocks_per_second: u32, // Measured from last update()
    profiled_aggregate_clocks_per_second: u32, // Measure over stats update period

    profiled_last_fps: f32, // Extrapolated from last frame duration
    profiled_aggregate_fps: f32, // Measured over stats update period

    tmp_row_values: Vec<u8>,
}

#[derive(Parser, Debug)]
#[clap(version, about, long_about = None)]
pub struct Args {
    rom: Option<String>,

    #[clap(short='t', long="trace", help="Record a trace of CPU instructions executed")]
    trace: Option<String>,

    #[clap(short='r', long="relative-time", help="Step emulator by relative time intervals, not necessarily keeping up with real time")]
    relative_time: Option<bool>,
}

impl EmulatorUi {

    pub fn new(args: &Args, ctx: &egui::Context) -> Result<Self> {
        let audio_host = cpal::default_host();
        let audio_device = audio_host
            .default_output_device()
            .expect("failed to find output device");
        let audio_config = audio_device.default_output_config().unwrap();
        let audio_sample_rate = audio_config.sample_rate().0;
        debug!("Audio sample rate = {}", audio_sample_rate);

        let buffer_time_millis = 5000;
        let ring_size =
            ((audio_sample_rate as u64) *
            (buffer_time_millis as u64)) / 1000;
        let ring_size = ((ring_size * 2) - (ring_size / 2)) as usize;
        debug!("Audio ring buffer size = {ring_size} samples");

        let (audio_tx, rx) = ring_channel::<f32>(NonZeroUsize::new(ring_size).unwrap());
        let audio_stream = match audio_config.sample_format() {
            SampleFormat::F32 => make_audio_stream::<f32>(&audio_device, &audio_config.into(), rx),
            SampleFormat::I16 => make_audio_stream::<i16>(&audio_device, &audio_config.into(), rx),
            SampleFormat::U16 => make_audio_stream::<u16>(&audio_device, &audio_config.into(), rx),
        }.unwrap();

        let mut nes = Nes::new(audio_sample_rate);

        let back_framebuffer = nes.allocate_framebuffer();
        let front_framebuffer = nes.system_ppu().swap_framebuffer(back_framebuffer).unwrap();
        //let framebuffer1 = nes.allocate_framebuffer();
        let fb_width = front_framebuffer.width();
        let fb_height = front_framebuffer.height();

        let framebuffer_texture = {
            //let blank = vec![egui::epaint::Color32::default(); fb_width * fb_height];
            let blank = ColorImage {
                size: [fb_width as _, fb_height as _],
                pixels: vec![Color32::default(); fb_width * fb_height],
            };
            let blank = ImageData::Color(blank);
            let tex = ctx.load_texture("framebuffer", blank, egui::TextureFilter::Nearest);
            tex
        };

        let nametables_fb_bpp = 3;
        let nametables_fb_stride = fb_width * 2 * nametables_fb_bpp;
        let nametables_framebuffer = vec![0u8; nametables_fb_stride * fb_height * 2];
        let nametables_texture = {
            //let blank = vec![egui::epaint::Color32::default(); fb_width * fb_height];
            let blank = ColorImage {
                size: [(fb_width * 2) as _, (fb_height * 2) as _],
                pixels: vec![Color32::default(); fb_width * 2 * fb_height * 2],
            };
            let blank = ImageData::Color(blank);
            let tex = ctx.load_texture("nametables_framebuffer", blank, egui::TextureFilter::Nearest);
            tex
        };

        let ppu_events_fb_bpp = 3;

        let ppu_events_fb_stride = PPU_EVENTS_FB_WIDTH * ppu_events_fb_bpp;
        let ppu_events_framebuffer = vec![0u8; ppu_events_fb_stride * PPU_EVENTS_FB_HEIGHT];
        let ppu_events_texture = {
            //let blank = vec![egui::epaint::Color32::default(); fb_width * fb_height];
            let blank = ColorImage {
                size: [PPU_EVENTS_FB_WIDTH as _, PPU_EVENTS_FB_HEIGHT as _],
                pixels: vec![Color32::default(); PPU_EVENTS_FB_WIDTH * PPU_EVENTS_FB_HEIGHT],
            };
            let blank = ImageData::Color(blank);
            let tex = ctx.load_texture("ppu_events_framebuffer", blank, egui::TextureFilter::Nearest);
            tex
        };
        let now = Instant::now();

        let mut emulator = Self {
            real_time: !args.relative_time.unwrap_or(false),

            notices: Default::default(),
            audio_device,
            audio_sample_rate,
            //audio_config,
            audio_tx,
            audio_stream,

            nes,

            fb_width,
            fb_height,
            //framebuffers: [framebuffer0, framebuffer1],
            //back_framebuffer: 0,
            //front_framebuffer: 1,
            front_framebuffer,
            framebuffer_texture,
            queue_framebuffer_upload: false,
            last_frame_time: now,

            show_nametables: false,
            nametables_framebuffer,
            nametables_texture,
            queue_nametable_fb_upload: false,

            nametables_show_scroll: true,
            nametables_hover_pos: [0, 0],

            show_memview: false,
            memview_selected_space: AddressSpace::Oam,

            paused: false,
            single_step: false,

            frame_no: 0,

            stats_update_period: Duration::from_secs(5),
            last_stats_update_timestamp: now,
            last_stats_update_frame_no: 0,
            last_stats_update_cpu_clock: 0,

            profiled_last_clocks_per_second: 0,
            profiled_aggregate_clocks_per_second: 0,

            profiled_last_fps: 0.0,
            profiled_aggregate_fps: 0.0,

            tmp_row_values: vec![],

            show_ppu_events: false,
            ppu_events_framebuffer,
            ppu_events_texture,
            ppu_events_hover_pos: [0, 0],
            queue_ppu_events_fb_upload: false,
        };

        if let Some(ref rom) = args.rom {
            emulator.open_binary(rom)?;
        }

        Ok(emulator)
    }

    pub fn open_binary(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let rom = get_file_as_byte_vec(path);

        self.nes = Nes::new(self.audio_sample_rate);
        self.nes.open_binary(&rom)?;

        let start_timestamp = std::time::Instant::now();
        self.nes.poweron(start_timestamp);
        self.audio_stream.play()?;

        Ok(())
    }

    fn open_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("nes", &["nes"])
            .add_filter("nsf", &["nsf"])
            .pick_file()
        {
            if let Err(er) = self.open_binary(path) {

            }
        }
    }

    fn save_image(&mut self) {
        let mut front = self.front_buffer();
        if let Some(rental) = front.rent_data() {
            let fb_width = front.width();
            let fb_height = front.height();

            let fb_buf = &rental.data;
            let stride = fb_width * 4;
            let mut imgbuf = image::ImageBuffer::new(fb_width as u32, fb_height as u32);
            for (x, y, pixel) in imgbuf.enumerate_pixels_mut() {
                let x = x as usize;
                let y = y as usize;
                let r = fb_buf[stride * y + x * 4];
                let g = fb_buf[stride * y + x * 4 + 1];
                let b = fb_buf[stride * y + x * 4 + 2];
                let _a = fb_buf[stride * y + x * 4 + 3];
                *pixel = image::Rgb([r, g, b]);
            }
            println!("Saving debug image");
            imgbuf.save(format!("nes-emulator-frame-{}.png", epoch_timestamp())).unwrap();
        }
    }

    pub fn draw_notices_header(&mut self, ui: &mut Ui) {

        while self.notices.len() > 0 {
            let ts = self.notices.front().unwrap().timestamp;
            if Instant::now() - ts > Duration::from_secs(NOTICE_TIMEOUT_SECS) {
                self.notices.pop_front();
            } else {
                break;
            }
        }

        if self.notices.len() > 0 {
            for notice in self.notices.iter() {
                let mut rt = RichText::new(notice.text.clone())
                    .strong();
                let (fg, bg) = match notice.level {
                    log::Level::Warn => (Color32::YELLOW, Color32::DARK_GRAY),
                    log::Level::Error => (Color32::WHITE, Color32::DARK_RED),
                    _ => (Color32::TRANSPARENT, Color32::BLACK)
                };
                rt = rt.color(fg).background_color(bg);
                ui.label(rt);
            }
        }
    }


    fn real_time_emulation_speed(&self) -> f32 {
        self.profiled_last_clocks_per_second as f32 / self.nes.cpu_clock_hz() as f32
    }
    fn aggregated_emulation_speed(&self) -> f32 {
        self.profiled_aggregate_clocks_per_second as f32 / self.nes.cpu_clock_hz() as f32
    }

    pub fn estimated_cpu_clocks_for_duration(&self, duration: Duration) -> u64 {
        if self.profiled_last_clocks_per_second > 0 {
            (self.profiled_last_clocks_per_second as f64 * duration.as_secs_f64()) as u64
        } else {
            (self.nes.cpu_clock_hz() as f64 * duration.as_secs_f64()) as u64
        }
    }
    pub fn estimate_duration_for_cpu_clocks(&self, cpu_clocks: u64) -> Duration {
        if self.profiled_last_clocks_per_second > 0 {
            Duration::from_secs_f64(cpu_clocks as f64 / self.profiled_last_clocks_per_second as f64)
        } else {
            Duration::from_secs_f64(cpu_clocks as f64 / self.nes.cpu_clock_hz() as f64)
        }
    }

    pub fn update_nametable_framebuffer(&mut self) {
        let fb_width = self.front_buffer().width();
        let fb_height = self.front_buffer().height();
        let bpp = 3;
        let stride = fb_width * 2 * bpp;
        for y in 0..(fb_height * 2) {
            for x in 0..(fb_width * 2) {
                let pix = self.nes.debug_sample_nametable(x, y);
                let pos = y * stride + x * bpp;
                self.nametables_framebuffer[pos + 0] = pix[0];
                self.nametables_framebuffer[pos + 1] = pix[1];
                self.nametables_framebuffer[pos + 2] = pix[2];
            }
        }

        self.queue_nametable_fb_upload = true;
    }

    pub fn update(&mut self) {

        if self.paused == false || self.single_step == true {
            let update_limit = Duration::from_micros(1_000_000 / 30); // We want to render at at-least 30fps even if emulation is running slow

            let update_start = Instant::now();
            let update_start_clock = self.nes.cpu_clock();

            let target = if self.real_time {
                let ideal_target = self.nes.cpu_clocks_for_time_since_poweron(update_start);
                if self.estimate_duration_for_cpu_clocks(ideal_target - self.nes.cpu_clock()) < update_limit {
                    // The happy path: we are emulating in real-time and we are keeping up

                    // TODO: if we are consistently vblank synchronized and not missing frames then we
                    // should aim to accurately snap+align update intervals with the vblank interval (even
                    // if that might technically have a small time skew compared to the original hardware
                    // with 60hz vs 59.94hz)
                    ProgressTarget::Clock(self.nes.cpu_clocks_for_time_since_poweron(update_start))
                } else {
                    // We are _trying_ to emulate in real-time but not keeping up, so we limit
                    // how much we try and progress based on the emulation performance we have
                    // observed.
                    let ideal_target = self.nes.cpu_clocks_for_time_since_poweron(update_start);
                    let limit_step_target = self.nes.cpu_clock() + self.estimated_cpu_clocks_for_duration(update_limit);
                    let target = limit_step_target.min(ideal_target);
                    ProgressTarget::Clock(target)
                }
            } else {
                // Non-real-time emulation: we progress the emulator forwards based on
                // the limit duration and based on the emulation performance we have observed
                let limit_step_target = self.nes.cpu_clock() + self.estimated_cpu_clocks_for_duration(update_limit);
                ProgressTarget::Clock(limit_step_target)
            };

            'progress: loop {
                match self.nes.progress(target) {
                    ProgressStatus::FrameReady => {
                        let now = Instant::now();
                        let frame_duration = now - self.last_frame_time;
                        self.profiled_last_fps = (1.0 as f64 / frame_duration.as_secs_f64()) as f32;
                        self.last_frame_time = now;
                        //self.front_framebuffer = self.back_framebuffer;
                        //self.back_framebuffer = (self.back_framebuffer + 1) % self.framebuffers.len();

                        //println!("Frame Ready: swapping in new PPU back buffer");
                        self.front_framebuffer = self.nes.system_ppu().swap_framebuffer(self.front_framebuffer.clone()).expect("Failed to swap in new framebuffer for PPU");

                        self.queue_framebuffer_upload = true;
                        self.update_nametable_framebuffer();
                        self.frame_no += 1;
                    },
                    ProgressStatus::ReachedTarget => {
                        break 'progress;
                    }
                    ProgressStatus::Error => {
                        error!("Internal emulator error");
                        break 'progress;
                    }
                }
                //let delta = std::time::Instant::now() - start;
                //if delta > Duration::from_millis(buffer_time_millis) {
                    for s in self.nes.system_apu().sample_buffer.iter() {
                        let _ = self.audio_tx.send(*s);
                    }
                    self.nes.system_apu().sample_buffer.clear();
                //}
            }

            let cpu_clock = self.nes.cpu_clock();
            let elapsed = Instant::now() - update_start;
            let clocks_elapsed = cpu_clock - update_start_clock;
            // Try to avoid updating last_clocks_per_second for early exit conditions where we didn't actually do any work
            if elapsed > Duration::from_millis(1) || clocks_elapsed > 2000 {
                self.profiled_last_clocks_per_second = (clocks_elapsed as f64 / elapsed.as_secs_f64()) as u32;
            }
            let now = Instant::now();
            let stats_update_duration = now - self.last_stats_update_timestamp;
            if stats_update_duration > self.stats_update_period {
                let n_frames = self.frame_no - self.last_stats_update_frame_no;
                let aggregate_fps = (n_frames as f64 / stats_update_duration.as_secs_f64()) as f32;

                let n_clocks = cpu_clock - self.last_stats_update_cpu_clock;
                let aggregate_cps = (n_clocks as f64 / stats_update_duration.as_secs_f64()) as u32;

                let aggregate_speed = (self.aggregated_emulation_speed() * 100.0) as u32;
                debug!("Aggregate Emulator Stats: Clocks/s: {aggregate_cps:8}, Update FPS: {aggregate_fps:4.2}, Real-time Speed: {aggregate_speed:3}%");

                let last_fps = self.profiled_last_fps;
                let last_cps = self.profiled_last_clocks_per_second;
                let latest_speed = (self.real_time_emulation_speed() * 100.0) as u32;
                debug!("Raw Emulator Stats:       Clocks/s: {last_cps:8}, Update FPS: {last_fps:4.2}, Real-time Speed: {latest_speed:3}%");

                self.last_stats_update_timestamp = now;
                self.last_stats_update_frame_no = self.frame_no;
                self.last_stats_update_cpu_clock = cpu_clock;
                self.profiled_aggregate_fps = aggregate_fps as f32;
                self.profiled_aggregate_clocks_per_second = aggregate_cps;
            }
            self.single_step = false;
        }
    }

    pub fn front_buffer(&mut self) -> Framebuffer {
        self.front_framebuffer.clone()
        //self.framebuffers[self.front_framebuffer].clone()
    }


    pub fn draw_memory_view(&mut self, ctx: &egui::Context) {
        egui::Window::new("Memory View")
            .resizable(true)
            .show(ctx, |ui| {

                 egui::SidePanel::left("memview_options_panel").show_inside(ui, |ui| {
                    egui::ComboBox::from_label("address_space")
                        .selected_text(format!("{:?}", self.memview_selected_space))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.memview_selected_space, AddressSpace::System, "System Bus");
                            ui.selectable_value(&mut self.memview_selected_space, AddressSpace::Ppu, "PPU Bus");
                            ui.selectable_value(&mut self.memview_selected_space, AddressSpace::Ppu, "OAM Memory");
                        }
                    );
                });

                let bytes_per_row = 16;
                let num_rows: usize = (1<<16) / bytes_per_row;
                let n_val_cols = bytes_per_row;


                let addr_col_width = Size::exact(60.0);
                let val_col_width = Size::exact(30.0);
                let char_col_width = Size::exact(10.0);
                let text_view_padding = Size::exact(100.0);
                let row_height_sans_spacing = 30.0;
                //let num_rows = 20;
                use egui_extras::{TableBuilder, Size};
                let mut tb = TableBuilder::new(ui)
                    .column(addr_col_width);

                for _ in 0..n_val_cols {
                    tb = tb.column(val_col_width);
                }

                tb = tb.column(text_view_padding);

                for _ in 0..n_val_cols {
                    tb = tb.column(char_col_width);
                }

                tb
                    .header(30.0, |mut header| {
                        header.col(|ui| {
                            ui.heading("    ");
                        });

                        for i in 0..n_val_cols {
                            header.col(|ui| {
                                ui.heading(format!("{i:02x}"));
                            });
                        }

                        header.col(|ui| {
                            ui.heading("     ");
                        });

                        for _ in 0..n_val_cols {
                            header.col(|ui| {
                                ui.heading(" ");
                            });
                        }
                    })
                    .body(|mut body| {
                        body.rows(row_height_sans_spacing, num_rows, |row_index, mut row| {
                            row.col(|ui| {
                                ui.heading(format!("{:04x}", row_index * bytes_per_row));
                            });

                            self.tmp_row_values.clear();
                            for i in 0..n_val_cols {
                                let addr = row_index * bytes_per_row + i;
                                let val = match self.memview_selected_space{
                                    AddressSpace::System => self.nes.peek_system_bus(addr as u16),
                                    AddressSpace::Ppu => self.nes.peek_ppu_bus(addr as u16),
                                    AddressSpace::Oam => self.nes.system_ppu().peek_oam_data(addr as u8),
                                };
                                self.tmp_row_values.push(val);
                                row.col(|ui| {
                                    ui.label(format!("{:02x}", val));
                                });
                            }
                            row.col(|ui| {
                                ui.label(" ");
                            });
                            for i in 0..n_val_cols {
                                let val = self.tmp_row_values[i];
                                row.col(|ui| {
                                    if val.is_ascii_alphanumeric() {
                                        ui.label(format!("{}", val as char));
                                    } else {
                                        ui.label(".");
                                    }
                                });
                            }
                        });
                    });
         });
    }


    // Really klunky :/
    pub fn draw_nametable_rect(ui: &mut Ui, rect: egui::Rect, scale: f32, offset: egui::Vec2) {
        use std::ops::Mul;
        let scaled = egui::Rect {
            min: rect.min.to_vec2().mul(scale).to_pos2(),
            max: rect.max.to_vec2().mul(scale).to_pos2(),
        };
    }

    pub fn draw_nametables_view(&mut self, ctx: &egui::Context) {
        let width = self.fb_width * 2;
        let height = self.fb_height * 2;
        if self.queue_nametable_fb_upload {
            let copy = ImageDelta::full(ImageData::Color(ColorImage {
                size: [width as _, height as _],
                pixels: self.nametables_framebuffer.chunks_exact(3)
                    .map(|p| Color32::from_rgba_premultiplied(p[0], p[1], p[2], 255))
                    .collect(),
            }), egui::TextureFilter::Nearest);

            ctx.tex_manager().write().set(self.nametables_texture.id(), copy);
            self.queue_nametable_fb_upload = false;
        }

        egui::Window::new("Nametables")
            .default_width(900.0)
            .resizable(true)
            //.resize(|r| r.auto_sized())
            .show(ctx, |ui| {

                let panels_width = ui.fonts().pixels_per_point() * 100.0;

                egui::SidePanel::left("nametables_options_panel")
                    .resizable(false)
                    .min_width(panels_width)
                    .show_inside(ui, |ui| {
                        ui.checkbox(&mut self.nametables_show_scroll, "Show Scroll Position");
                    });
                egui::SidePanel::right("nametables_properties_panel")
                    .resizable(false)
                    .min_width(panels_width)
                    .show_inside(ui, |ui| {
                        //ui.label(format!("Scroll X: {}", self.nes.system_ppu().scroll_x()));
                        //ui.label(format!("Scroll Y: {}", self.nes.system_ppu().scroll_y()));
                });

                egui::TopBottomPanel::bottom("nametables_footer").show_inside(ui, |ui| {
                    ui.label(format!("[{}, {}]", self.nametables_hover_pos[0], self.nametables_hover_pos[1]));
                });

                //let frame = Frame::none().outer_margin(Margin::same(200.0));
                egui::CentralPanel::default()
                    //.frame(frame)
                    .show_inside(ui, |ui| {

                        let (response, painter) =
                            ui.allocate_painter(egui::Vec2::new(width as f32, height as f32), egui::Sense::hover());

                        let img = egui::Image::new(self.nametables_texture.id(), egui::Vec2::new(width as f32, height as f32));
                        //let response = ui.add(egui::Image::new(self.nametables_texture.id(), egui::Vec2::new(width as f32, height as f32)));
                                        // TODO(emilk): builder pattern for Mesh

                        let mut mesh = egui::Mesh::with_texture(self.nametables_texture.id());
                        mesh.add_rect_with_uv(response.rect, egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE,);
                        painter.add(Shape::mesh(mesh));

                        let img_pos = response.rect.left_top();
                        let img_width = response.rect.width();
                        let img_height = response.rect.height();
                        let img_to_nes_px = self.fb_width as f32 * 2.0 / img_width;
                        let nes_px_to_img = 1.0 / img_to_nes_px;

                        if let Some(hover_pos) = response.hover_pos() {
                            let x = ((hover_pos.x - img_pos.x) * img_to_nes_px) as usize;
                            let y = ((hover_pos.y - img_pos.y) * img_to_nes_px) as usize;
                            self.nametables_hover_pos = [x, y];

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

    pub fn draw(&mut self, ctx: &egui::Context) -> Status {
        let mut status = Status::Ok;

        let front = self.front_buffer();

        if self.queue_framebuffer_upload {
            //println!("Uploading frame for render");
            //let rental = self.framebuffers[self.front_framebuffer].rent_data().unwrap();
            let rental = self.front_framebuffer.rent_data().unwrap();

            // hmmm, redundant copy, grumble grumble...
            let copy = ImageDelta::full(ImageData::Color(ColorImage {
                size: [front.width() as _, front.height() as _],
                pixels: rental.data.chunks_exact(4)
                    .map(|p| Color32::from_rgba_premultiplied(p[0], p[1], p[2], 255))
                    .collect(),
            }), egui::TextureFilter::Nearest);

            ctx.tex_manager().write().set(self.framebuffer_texture.id(), copy);

            /*
            // DEBUG clear to red, to be able to see if any framebuffer pixels aren't rendered in the next frame
            for pixel in rental.data.chunks_exact_mut(4) {
                pixel[0] = 0xff;
                pixel[1] = 0x00;
                pixel[2] = 0x00;
                pixel[3] = 0xff;
            }*/

            self.queue_framebuffer_upload = false;
        }

        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            self.draw_notices_header(ui);
            use egui::{menu, Button};

            menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open").clicked() {
                        self.open_dialog();
                    }
                });
            });

        });

        egui::SidePanel::left("side_panel").show(ctx, |ui| {
            if ui.button("Quit").clicked() {
                status = Status::Quit;
            }
            if ui.button("Reset").clicked() {
                self.nes.reset();
            }
            if !self.paused {
                if ui.button("Break").clicked() {
                    self.paused = true;
                }
            } else {
                if ui.button("Step").clicked() {
                    self.single_step = true;
                }
                if ui.button("Continue").clicked() {
                    self.paused = false;
                }

                let ppu = self.nes.system_ppu();
                let debug_val = self.nes.peek_ppu_bus(0x2000);
                //println!("PPU debug = {debug_val:x}");
            }

            ui.checkbox(&mut &mut self.show_memview, "Show Memory");
            ui.checkbox(&mut &mut &mut self.show_nametables, "Show Nametables");
        });

        egui::CentralPanel::default().show(ctx, |ui| {

            ui.add(egui::Image::new(self.framebuffer_texture.id(), egui::Vec2::new((front.width() * 2) as f32, (front.height() * 2) as f32)));
        });

        if self.show_nametables {
            self.draw_nametables_view(ctx);
        }

        if self.show_memview {
            self.draw_memory_view(ctx);
        }

        status
    }



    pub fn handle_window_event(&mut self, event: winit::event::WindowEvent) {
        match event {
            WindowEvent::KeyboardInput { input, .. } => {
                if let Some(keycode) = input.virtual_keycode {
                    let button = match keycode {
                        VirtualKeyCode::Return => { Some(PadButton::Start) }
                        VirtualKeyCode::Space => { Some(PadButton::Select) }
                        VirtualKeyCode::A => { Some(PadButton::Left) }
                        VirtualKeyCode::D => { Some(PadButton::Right) }
                        VirtualKeyCode::W => { Some(PadButton::Up) }
                        VirtualKeyCode::S => { Some(PadButton::Down) }
                        VirtualKeyCode::Right => { Some(PadButton::A) }
                        VirtualKeyCode::Left => { Some(PadButton::B) }
                        _ => None
                    };
                    if let Some(button) = button {
                        let system = self.nes.system_mut();
                        if input.state == winit::event::ElementState::Pressed {
                            system.pad1.press_button(button);
                        } else {
                            system.pad1.release_button(button);
                        }
                    }
                }
            }
            _ => {

            }
        }
    }
}
