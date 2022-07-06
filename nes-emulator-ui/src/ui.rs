// There's some kind of compiler bug going on, causing a crazy amount of false
// positives atm :(
#![allow(dead_code)]

use std::{collections::{HashSet, HashMap, VecDeque}, fmt::Debug, time::{Instant, Duration}, path::{Path, PathBuf}, fs::File, io::Read, num::NonZeroUsize};

use log::{error, warn, info, debug, trace};
use clap::Parser;

use anyhow::anyhow;
use anyhow::Result;

use winit::event::{Event, WindowEvent, VirtualKeyCode};

use egui::{self, RichText, Color32, Ui, ImageData, TextureHandle};
use egui::{ColorImage, epaint::ImageDelta};

use cpal::SampleRate;
use cpal::traits::StreamTrait;
use cpal::{traits::{HostTrait, DeviceTrait}, OutputCallbackInfo, SampleFormat, Sample};

use ring_channel::{ring_channel, TryRecvError, RingReceiver, RingSender};

use nes_emulator::prelude::*;



#[derive(Debug, Clone)]
pub enum UserEvent {
    RequestRedraw,
    // Show a notice to the user...
    ShowText(log::Level, String),
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

pub struct EmulatorUi {
    notices: VecDeque<Notice>,

    audio_device: cpal::Device,
    audio_sample_rate: u32,
    //audio_config: cpal::SupportedStreamConfig

    nes: Nes,
    framebuffers: [Framebuffer; 2],
    front_framebuffer: usize,
    back_framebuffer: usize,
    framebuffer_texture: TextureHandle,
    audio_tx: RingSender<f32>,
    audio_stream: cpal::Stream,
    paused: bool,
    single_step: bool,

    frame_no: u32,

    queue_framebuffer_upload: bool,
}

#[derive(Parser, Debug)]
#[clap(version, about, long_about = None)]
pub struct Args {
    rom: Option<String>,

    #[clap(short='t', long="trace", help="Record a trace of CPU instructions executed")]
    trace: Option<String>,
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

        let nes = Nes::new(PixelFormat::RGBA8888, audio_sample_rate);

        let framebuffer0 = nes.allocate_framebuffer();
        let framebuffer1 = nes.allocate_framebuffer();
        let fb_width = framebuffer0.width();
        let fb_height = framebuffer0.height();

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

        let mut emulator = Self {
            notices: Default::default(),
            audio_device,
            audio_sample_rate,
            //audio_config,
            nes,
            framebuffers: [framebuffer0, framebuffer1],
            back_framebuffer: 0,
            front_framebuffer: 1,
            framebuffer_texture,
            audio_tx,
            audio_stream,
            paused: false,
            single_step: false,

            frame_no: 0,
            queue_framebuffer_upload: false,
        };

        if let Some(ref rom) = args.rom {
            emulator.open_binary(rom)?;
        }

        Ok(emulator)
    }

    pub fn open_binary(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let rom = get_file_as_byte_vec(path);

        self.nes = Nes::new(PixelFormat::RGBA8888, self.audio_sample_rate);
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

    pub fn update(&mut self) {

        if self.paused == false || self.single_step == true {
            let target_timestamp = Instant::now();
            'progress: loop {

                match self.nes.progress(target_timestamp, self.framebuffers[self.back_framebuffer].clone()) {
                    ProgressStatus::FrameReady => {
                        self.front_framebuffer = self.back_framebuffer;
                        self.back_framebuffer = (self.back_framebuffer + 1) % self.framebuffers.len();
                        self.queue_framebuffer_upload = true;
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

            self.single_step = false;
        }
    }

    pub fn front_buffer(&mut self) -> Framebuffer {
        self.framebuffers[self.front_framebuffer].clone()
    }

    pub fn draw(&mut self, ctx: &egui::Context) {
        let front = self.front_buffer();

        if self.queue_framebuffer_upload {
            let rental = self.framebuffers[self.front_framebuffer].rent_data().unwrap();

            // hmmm, redundant copy, grumble grumble...
            let copy = ImageDelta::full(ImageData::Color(ColorImage {
                size: [front.width() as _, front.height() as _],
                pixels: rental.data.chunks_exact(4)
                    .map(|p| Color32::from_rgba_premultiplied(p[0], p[1], p[2], 255))
                    .collect(),
            }), egui::TextureFilter::Nearest);

            ctx.tex_manager().write().set(self.framebuffer_texture.id(), copy);
            self.queue_framebuffer_upload = false;
        }

        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            self.draw_notices_header(ui);

        });

        egui::CentralPanel::default().show(ctx, |ui| {

            ui.add(egui::Image::new(self.framebuffer_texture.id(), egui::Vec2::new((front.width() * 2) as f32, (front.height() * 2) as f32)));
        });
    }

    pub fn handle_event(&mut self, event: Event<UserEvent>) {
        match event {
            Event::WindowEvent { event, .. } => {
                match event {
                    WindowEvent::KeyboardInput { input, .. } => {
                        if let Some(keycode) = input.virtual_keycode {
                            let button = match keycode {
                                VirtualKeyCode::Return => {
                                    println!("{:?} Start Button", input.state);
                                    Some(PadButton::Start) }
                                VirtualKeyCode::Space => { Some(PadButton::Select) }
                                VirtualKeyCode::A => { Some(PadButton::Left) }
                                VirtualKeyCode::D => { Some(PadButton::Right) }
                                VirtualKeyCode::W => { Some(PadButton::Up) }
                                VirtualKeyCode::S => { Some(PadButton::Down) }
                                VirtualKeyCode::Right => {
                                    println!("{:?} Button A", input.state);
                                     Some(PadButton::A) }
                                VirtualKeyCode::Left => {
                                    println!("{:?} Button B", input.state);
                                    Some(PadButton::B) }
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
            Event::UserEvent(UserEvent::ShowText(level, text)) => {
                self.notices.push_back(Notice { level, text, timestamp: Instant::now() });
            }
            _ => {

            }
        }
    }
}