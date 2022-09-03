use std::{collections::{VecDeque}, fmt::Debug, time::{Instant, Duration}, path::{Path, PathBuf}, fs::File, io::{BufWriter}, num::NonZeroUsize, rc::Rc, cell::RefCell, sync::mpsc};
use std::io::Write;

use log::{error, debug};

use anyhow::Result;

use winit::{event::{WindowEvent, VirtualKeyCode, ModifiersState}, event_loop::EventLoopProxy};

use egui::{self, RichText, Color32, Ui, ImageData, TextureHandle};
use egui::{ColorImage, epaint::ImageDelta};

use cpal::traits::StreamTrait;
use cpal::{traits::{HostTrait, DeviceTrait}, OutputCallbackInfo, SampleFormat, Sample};

use ring_channel::{ring_channel, TryRecvError, RingReceiver, RingSender};

use nes_emulator::{nes::*, system::Model, port::ControllerButton, hook::HookHandle, cpu::cpu::BreakpointHandle};
use nes_emulator::framebuffer::*;

use crate::{Args, utils, benchmark::BenchmarkState, macros::{Macro, MacroPlayer, self}, view::{macro_builder::MacroBuilderView, memory::MemView, nametable::NametablesView, trace_events::TraceEventsView, sprites::SpritesView, debugger::DebuggerView, apu::ApuView}};

const BENCHMARK_STATS_PERIOD_SECS: u8 = 3;

pub enum Status {
    Ok,
    Quit
}

const NOTICE_TIMEOUT_SECS: u8 = 7;
struct Notice {
    level: log::Level,
    text: String,
    timestamp: Instant
}


/// Each debug view / tool is fairly self-contained and although they can directly inspect
/// and modify the Nes they don't have arbitrary control over the rest of the EmulatorUi
/// and instead need to send the top-level UI requests
#[derive(Debug)]
pub enum ViewRequest {
    ShowUserNotice(log::Level, String),
    RunMacro(Macro),
    LoadRom(String),

    InstructionStepOver,
    InstructionStepIn,
    InstructionStepOut,
}

#[derive(Clone)]
pub struct ViewRequestSender {
    tx: mpsc::Sender<ViewRequest>,
    proxy: EventLoopProxy<crate::ui_winit::Event>
}

impl ViewRequestSender {
    pub fn send(&self, req: ViewRequest) {
        let _ = self.tx.send(req);
        let _ = self.proxy.send_event(crate::ui_winit::Event::RequestRedraw);
    }
}

fn load_nes(path: Option<impl AsRef<Path>>, rom_dirs: &Vec<PathBuf>, audio_sample_rate: u32, start_timestamp: Instant, notices: &mut VecDeque<Notice>) -> (Nes, Option<PathBuf>) {
    if let Some(path) = path {
        if let Some(ref path) = utils::find_rom(path, rom_dirs) {
            match utils::create_nes_from_binary(path, audio_sample_rate, start_timestamp) {
                Ok(nes) => return (nes, Some(path.clone())),
                Err(err) => {
                    notices.push_back(Notice { level: log::Level::Error, text: format!("{}", err), timestamp: Instant::now() });
                }
            }
        }
    }
    (Nes::new(Model::Ntsc, audio_sample_rate, start_timestamp), None)
}

pub fn blank_texture_for_framebuffer(ctx: &egui::Context, info: &impl FramebufferInfo, name: impl Into<String>) -> TextureHandle {
    let blank = ColorImage {
        size: [info.width(), info.height()],
        pixels: vec![Color32::default(); info.width() * info.height()],
    };
    let blank = ImageData::Color(blank);
    ctx.load_texture(name, blank, egui::TextureFilter::Nearest)
}

pub fn full_framebuffer_image_delta(fb: &FramebufferDataRental) -> ImageDelta {
    let owner = &fb.owner();
    let width = owner.width();
    let height = owner.height();

    match owner.format() {
        PixelFormat::RGBA8888 => {
            ImageDelta::full(ImageData::Color(ColorImage {
                size: [width, height],
                pixels: fb.data.chunks_exact(4)
                    .map(|p| Color32::from_rgba_premultiplied(p[0], p[1], p[2], p[3]))
                    .collect(),
            }), egui::TextureFilter::Nearest)
        }
        PixelFormat::RGB888 => {
            ImageDelta::full(ImageData::Color(ColorImage {
                size: [width, height],
                pixels: fb.data.chunks_exact(3)
                    .map(|p| Color32::from_rgba_premultiplied(p[0], p[1], p[2], 0xff))
                    .collect(),
            }), egui::TextureFilter::Nearest)
        }
        PixelFormat::GREY8 => {
            ImageDelta::full(ImageData::Color(ColorImage {
                size: [width, height],
                pixels: fb.data.iter()
                    .map(|p| Color32::from_rgba_premultiplied(*p, *p, *p, 0xff))
                    .collect(),
            }), egui::TextureFilter::Nearest)
        }
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

fn read_audio_samples<T: Sample + Send + Debug>(rx: &mut RingReceiver<f32>, sampler_state: &mut EmulatorAudioState<T>, nchannels: usize, output: &mut [T], _info: &OutputCallbackInfo) {
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
    let mut _debug_options = SampleRequestOptions {
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
    real_time: bool,

    modifiers: ModifiersState,

    notices: VecDeque<Notice>,

    audio_device: cpal::Device,
    audio_sample_rate: u32,
    //audio_config: cpal::SupportedStreamConfig
    audio_tx: RingSender<f32>,
    audio_stream: cpal::Stream,

    player: Option<MacroPlayer>,
    shared_crc32: Rc<RefCell<u32>>,
    crc_hook_handle: Option<HookHandle>,

    rom_dirs: Vec<PathBuf>,

    nes: Nes,
    loaded_rom: Option<PathBuf>,

    trace_writer: Option<Rc<RefCell<BufWriter<File>>>>,

    pub paused: bool,

    /// The address of any temporary debugger breakpoint (for handling things
    /// like 'step over' or 'step out') which should be removed whenever
    /// the debugger next stops
    temp_debug_breakpoint: Option<BreakpointHandle>,

    fb_width: usize,
    fb_height: usize,
    front_framebuffer: Framebuffer,
    framebuffer_texture: TextureHandle,
    queue_framebuffer_upload: bool,
    last_frame_time: Instant,

    #[cfg(feature="cpu-debugger")]
    debugger_view: DebuggerView,

    #[cfg(feature="macro-builder")]
    macro_builder_view: MacroBuilderView,

    nametables_view: NametablesView,

    apu_view: ApuView,

    #[cfg(feature="sprite-view")]
    sprites_view: SpritesView,

    mem_view: MemView,
    trace_events_view: TraceEventsView,

    view_requests_rx: mpsc::Receiver<ViewRequest>,
    view_request_sender: ViewRequestSender,

    stats: BenchmarkState,

}

impl EmulatorUi {

    pub fn new(args: &Args, ctx: &egui::Context, event_loop_proxy: EventLoopProxy<crate::ui_winit::Event>) -> Result<Self> {

        let mut notices = VecDeque::new();

        let audio_host = cpal::default_host();
        let audio_device = audio_host
            .default_output_device()
            .expect("failed to find audio output device");
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

        let rom_dirs = utils::canonicalize_rom_dirs(&args.rom_dir);
        let rom_path = match &args.rom {
            Some(rom) => utils::find_rom(rom, &rom_dirs),
            None => None
        };
        let (mut nes, loaded_rom) = load_nes(rom_path.as_ref(), &rom_dirs, audio_sample_rate, Instant::now(), &mut notices);

        let back_framebuffer = nes.allocate_framebuffer();
        let front_framebuffer = nes.ppu_mut().swap_framebuffer(back_framebuffer).unwrap();
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

        let stats = BenchmarkState::new(&nes, Duration::from_secs(BENCHMARK_STATS_PERIOD_SECS as u64));

        let now = Instant::now();

        let (tx, rx) = mpsc::channel();
        let view_request_sender = ViewRequestSender {
            tx,
            proxy: event_loop_proxy
        };

        let paused = false;

        let mut emulator = Self {
            real_time: !args.relative_time,
            modifiers: Default::default(),
            notices,
            audio_device,
            audio_sample_rate,
            //audio_config,
            audio_tx,
            audio_stream,

            nes,

            player: None,
            crc_hook_handle: None,
            shared_crc32: Rc::new(RefCell::new(0)),

            trace_writer: None,

            paused,
            temp_debug_breakpoint: None,

            fb_width,
            fb_height,
            //framebuffers: [framebuffer0, framebuffer1],
            //back_framebuffer: 0,
            //front_framebuffer: 1,
            front_framebuffer,
            framebuffer_texture,
            queue_framebuffer_upload: false,
            last_frame_time: now,

            #[cfg(feature="cpu-debugger")]
            debugger_view: DebuggerView::new(view_request_sender.clone(), paused),

            #[cfg(feature="macro-builder")]
            macro_builder_view: MacroBuilderView::new(ctx, args, rom_dirs.clone(), loaded_rom.clone(), view_request_sender.clone(), paused),
            nametables_view: NametablesView::new(ctx),

            apu_view: ApuView::new(),

            #[cfg(feature="sprite-view")]
            sprites_view: SpritesView::new(ctx),

            trace_events_view: TraceEventsView::new(ctx),

            mem_view: MemView::new(),

            view_request_sender,
            view_requests_rx: rx,

            rom_dirs,
            loaded_rom,

            stats,
        };

        if let Some(trace) = &args.trace {
            if trace == "-" {
                emulator.nes.add_cpu_instruction_trace_hook(Box::new(move |_nes, trace_state| {
                    println!("{trace_state}");
                }));
            } else {
                let f = File::create(trace)?;
                let writer = Rc::new(RefCell::new(BufWriter::new(f)));
                emulator.trace_writer = Some(writer.clone());
                emulator.nes.add_cpu_instruction_trace_hook(Box::new(move |_nes, trace_state| {
                    if let Err(err) = writeln!(*writer.borrow_mut(), "{trace_state}") {
                        log::error!("Failed to write to CPU trace: {err}");
                    }
                }));
            }
        }

        //emulator.recreate_test_builder_on_load(ctx);
        emulator.power_on_new_nes();

        Ok(emulator)
    }

    fn power_on_new_nes(&mut self) {
        self.stats = BenchmarkState::new(&self.nes, Duration::from_secs(BENCHMARK_STATS_PERIOD_SECS as u64));

        let start_timestamp = std::time::Instant::now();
        self.nes.power_cycle(start_timestamp);
        if let Err(err) = self.audio_stream.play() {
            self.notices.push_back(Notice { level: log::Level::Error, text: format!("Couldn't start audio stream: {:#?}", err), timestamp: Instant::now() });
        }

        #[cfg(feature="macro-builder")]
        self.macro_builder_view.power_on_new_nes_hook(&mut self.nes, self.loaded_rom.as_ref());
    }

    /*
    fn recreate_test_builder_on_load(&mut self, ctx: &egui::Context) {
        #[cfg(feature="macro-builder")]
        {
            if let Some(loaded_rom) = &self.loaded_rom {
                self.macro_builder_view = Some(MacroBuilderView::new(ctx, loaded_rom.to_string(), self.view_request_sender.clone(), self.paused))
            }
        }
    }
    */

    pub fn open_binary(&mut self, path: impl AsRef<Path>) {
        self.disconnect_nes();
        let (nes, loaded_rom) = load_nes(Some(path.as_ref().clone()), &self.rom_dirs, self.audio_sample_rate, Instant::now(), &mut self.notices);
        self.nes = nes;
        self.loaded_rom = loaded_rom;

        self.power_on_new_nes();
    }

    pub fn disconnect_nes(&mut self) {
        if let Some(handle) = self.crc_hook_handle {
            self.nes.ppu_mut().remove_mux_hook(handle);
            self.crc_hook_handle = None;
        }

        #[cfg(feature="macro-builder")]
        self.macro_builder_view.disconnect_nes(&mut self.nes);
    }

    pub(crate) fn pick_rom_dialog() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .add_filter("nes", &["nes"])
            .add_filter("nsf", &["nsf"])
            .pick_file()
    }

    fn open_dialog(&mut self, _ctx: &egui::Context) {
        if let Some(path) = EmulatorUi::pick_rom_dialog() {
            self.open_binary(path);
        }
    }

    fn save_image(&mut self) {
        let front = self.front_buffer();
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
            imgbuf.save(format!("nes-emulator-frame-{}.png", utils::epoch_timestamp())).unwrap();
        }
    }

    pub fn draw_notices_header(&mut self, ui: &mut Ui) {

        while self.notices.len() > 0 {
            let ts = self.notices.front().unwrap().timestamp;
            if Instant::now() - ts > Duration::from_secs(NOTICE_TIMEOUT_SECS as u64) {
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

        match self.view_requests_rx.try_recv() {
            Ok(req) => {
                println!("Got view request: {req:?}");
                match req {
                    ViewRequest::ShowUserNotice(level, text) => {
                        self.notices.push_back(Notice { level, text, timestamp: Instant::now() });
                    }
                    ViewRequest::RunMacro(recording) => {
                        if let Some(rom) = utils::find_rom(&recording.rom, &self.rom_dirs) {
                            self.open_binary(rom);

                            if self.crc_hook_handle.is_none() {
                                self.crc_hook_handle = Some(macros::register_frame_crc_hasher(&mut self.nes, self.shared_crc32.clone()));
                            }
                            self.player = Some(MacroPlayer::new(recording, &mut self.nes, self.shared_crc32.clone()));

                            self.set_paused(false);
                            if let Some(player) = &mut self.player {

                                // TODO: have a generic way of notifying all views
                                #[cfg(feature="macro-builder")]
                                self.macro_builder_view.started_playback(&mut self.nes, player);

                                player.update(&mut self.nes);

                                // TODO: have a generic way of notifying all views
                                #[cfg(feature="macro-builder")]
                                self.macro_builder_view.playback_update(&mut self.nes, &player);
                            }
                        } else {
                            self.notices.push_back(Notice { level: log::Level::Error, text: "Failed to find ROM for macro".to_string(), timestamp: Instant::now() });
                        }
                    }
                    ViewRequest::LoadRom(path) => {
                        if let Some(rom) = utils::find_rom(path, &self.rom_dirs) {
                            self.open_binary(rom);
                            self.set_paused(false);

                            // TODO: have a generic way of notifying all views
                            #[cfg(feature="macro-builder")]
                            self.macro_builder_view.load_rom_request_finished(true);
                        } else {
                            self.notices.push_back(Notice { level: log::Level::Error, text: "Failed to find ROM for macro".to_string(), timestamp: Instant::now() });

                            // TODO: have a generic way of notifying all views
                            #[cfg(feature="macro-builder")]
                            self.macro_builder_view.load_rom_request_finished(false);
                        }
                    }
                    ViewRequest::InstructionStepIn => {
                        self.step_instruction_in()
                    }
                    ViewRequest::InstructionStepOut => {
                        self.step_instruction_out()
                    }
                    ViewRequest::InstructionStepOver => {
                        self.step_instruction_over()
                    }
                }
            },
            Err(_) => {}
        }

        if self.paused == false {
            let update_limit = Duration::from_micros(1_000_000 / 30); // We want to render at at-least 30fps even if emulation is running slow

            let update_start = Instant::now();
            self.stats.start_update(&self.nes, update_start);

            let target = if self.real_time {
                let ideal_target = self.nes.cpu_clocks_for_time_since_power_cycle(update_start);
                if self.stats.estimate_duration_for_cpu_clocks(ideal_target - self.nes.cpu_clock()) < update_limit {
                    // The happy path: we are emulating in real-time and we are keeping up

                    // TODO: if we are consistently vblank synchronized and not missing frames then we
                    // should aim to accurately snap+align update intervals with the vblank interval (even
                    // if that might technically have a small time skew compared to the original hardware
                    // with 60hz vs 59.94hz)
                    ProgressTarget::Clock(self.nes.cpu_clocks_for_time_since_power_cycle(update_start))
                } else {
                    // We are _trying_ to emulate in real-time but not keeping up, so we limit
                    // how much we try and progress based on the emulation performance we have
                    // observed.
                    let ideal_target = self.nes.cpu_clocks_for_time_since_power_cycle(update_start);
                    let limit_step_target = self.nes.cpu_clock() + self.stats.estimated_cpu_clocks_for_duration(update_limit);
                    let target = limit_step_target.min(ideal_target);
                    ProgressTarget::Clock(target)
                }
            } else {
                // Non-real-time emulation: we progress the emulator forwards based on
                // the limit duration and based on the emulation performance we have observed
                let limit_step_target = self.nes.cpu_clock() + self.stats.estimated_cpu_clocks_for_duration(update_limit);
                ProgressTarget::Clock(limit_step_target)
            };

            'progress: loop {
                match self.nes.progress(target) {
                    ProgressStatus::FrameReady => {
                        self.stats.end_frame();
                        //self.front_framebuffer = self.back_framebuffer;
                        //self.back_framebuffer = (self.back_framebuffer + 1) % self.framebuffers.len();

                        //println!("Frame Ready: swapping in new PPU back buffer");
                        self.front_framebuffer = self.nes.ppu_mut().swap_framebuffer(self.front_framebuffer.clone()).expect("Failed to swap in new framebuffer for PPU");

                        self.queue_framebuffer_upload = true;
                        if self.nametables_view.visible {
                            self.nametables_view.update(&mut self.nes);
                        }
                        #[cfg(feature="sprite-view")]
                        if self.sprites_view.visible {
                            self.sprites_view.update(&mut self.nes);
                        }
                        if self.trace_events_view.visible {
                            self.trace_events_view.update(&mut self.nes);
                        }
                    },
                    ProgressStatus::ReachedTarget => {
                        break 'progress;
                    }
                    ProgressStatus::Breakpoint => {
                        println!("Hit breakpoint");

                        // See if the macro player was expecting this breakpoint before deciding whether
                        // to pause the emulator
                        if let Some(player) = &mut self.player {
                            if !player.check_breakpoint(&mut self.nes) {
                                self.set_paused(true);
                            } else {
                                println!("Breakpoint was handled my macro player");
                            }
                        } else {
                            self.set_paused(true);
                        }

                        // If we had set a temporary breakpoint before continuing running the emulator
                        // we need to remove that now, regardless of where we stopped (it's possible
                        // we hit a different breakpoint and so our temporary one may not have been
                        // automatically removed)
                        if let Some(handle) = self.temp_debug_breakpoint {
                            self.nes.cpu_mut().remove_breakpoint(handle);
                            self.temp_debug_breakpoint = None;
                        }
                        break 'progress;
                    }
                    //ProgressStatus::Error => {
                    //    error!("Internal emulator error");
                    //    break 'progress;
                    //}
                }
                //let delta = std::time::Instant::now() - start;
                //if delta > Duration::from_millis(buffer_time_millis) {
                    for s in self.nes.apu_mut().sample_buffer.iter() {
                        let _ = self.audio_tx.send(*s);
                    }
                    self.nes.apu_mut().sample_buffer.clear();
                //}
            }

            if let Some(player) = &mut self.player {
                player.update(&mut self.nes);

                #[cfg(feature="macro-builder")]
                self.macro_builder_view.playback_update(&mut self.nes, &player);
            }

            self.stats.end_update(&self.nes);
        }
    }

    pub fn front_buffer(&mut self) -> Framebuffer {
        self.front_framebuffer.clone()
        //self.framebuffers[self.front_framebuffer].clone()
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

            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open").clicked() {
                        ui.close_menu();
                        self.open_dialog(ui.ctx());
                    }
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        status = Status::Quit;
                        ui.close_menu();
                    }
                });

                ui.menu_button("Nes", |ui| {
                    egui::Grid::new("some_unique_id").show(ui, |ui| {

                        if ui.button(if self.paused == false { "Pause" } else { "Resume" }).clicked() {
                            ui.close_menu();
                            self.set_paused(!self.paused);
                        }
                        ui.label("Esc");
                        ui.end_row();

                        if ui.button("Reset").clicked() {
                            ui.close_menu();
                            self.nes.reset();
                        }
                        ui.label("Ctrl-R");
                        ui.end_row();

                        if ui.button("Power Cycle").clicked() {
                            ui.close_menu();
                            self.nes.power_cycle(Instant::now());
                        }
                        ui.label("Ctrl-T");
                        ui.end_row();
                    });
                });
            });
        });

        egui::SidePanel::left("side_panel").show(ctx, |ui| {

            ui.spacing();
            ui.label("Tools");
            ui.group(|ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                    ui.toggle_value(&mut self.debugger_view.visible, "Debugger");
                    ui.toggle_value(&mut self.mem_view.visible, "Memory");
                    ui.toggle_value(&mut self.nametables_view.visible, "Nametables");

                    ui.toggle_value(&mut self.apu_view.visible, "APU");

                    ui.add_enabled_ui(cfg!(feature="sprite-view"), |ui| {
                        let resp = ui.toggle_value(&mut self.sprites_view.visible, "Show Sprites")
                            .on_disabled_hover_text("\"sprite-view\" feature not enabled");
                        #[cfg(feature="sprite-view")]
                        {
                            if resp.changed {
                                self.sprites_view.set_visible(&mut self.nes, self.sprites_view.visible);
                            }
                        }
                    });

                    ui.add_enabled_ui(cfg!(feature="macro-builder"), |ui| {
                        let mut visible = {
                            #[cfg(feature="macro-builder")]
                            {self.macro_builder_view.visible}
                            #[cfg(not(feature="macro-builder"))]
                            {false}
                        };
                        let resp = ui.toggle_value(&mut visible, "Record Macros")
                            .on_disabled_hover_text("\"macro-builder\" feature not enabled");
                        #[cfg(feature="macro-builder")]
                        {
                            if resp.changed() {
                                self.macro_builder_view.set_visible(&mut self.nes, visible);
                            }
                        }
                    });
                    let mut visible = self.trace_events_view.visible;
                    let resp = ui.toggle_value(&mut visible, "Show Events");
                    if resp.changed() {
                        self.trace_events_view.set_visible(&mut self.nes, visible);
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add(egui::Image::new(self.framebuffer_texture.id(), egui::Vec2::new((front.width() * 2) as f32, (front.height() * 2) as f32)));
        });

        #[cfg(feature="cpu-debugger")]
        {
            if self.debugger_view.visible {
                self.debugger_view.draw(&mut self.nes, ctx);
            }
        }

        if self.nametables_view.visible {
            self.nametables_view.draw(ctx);
        }

        if self.apu_view.visible {
            self.apu_view.draw(&mut self.nes, ctx);
        }

        #[cfg(feature="sprite-view")]
        if self.sprites_view.visible {
            self.sprites_view.draw(&mut self.nes, ctx);
        }

        #[cfg(feature="macro-builder")]
        {
            if self.macro_builder_view.visible {
                self.macro_builder_view.draw(&mut self.nes, ctx);
            }
        }
        if self.trace_events_view.visible {
            self.trace_events_view.draw(&mut self.nes, ctx);
        }
        if self.mem_view.visible {
            self.mem_view.draw(&mut self.nes, ctx);
        }

        status
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;

        if paused {
            if let Some(writer) = &self.trace_writer {
                let _ = writer.borrow_mut().flush();
            }
        }

        #[cfg(feature="cpu-debugger")]
        {
            self.debugger_view.set_paused(paused, &mut self.nes);
        }
        #[cfg(feature="macro-builder")]
        {
            self.macro_builder_view.set_paused(paused, &mut self.nes);
        }
        #[cfg(feature="trace-events")]
        {
            self.trace_events_view.set_paused(paused, &mut self.nes);
        }

        if paused == false {
            // Stop the emulator from trying to catch up for lost time
            self.nes.set_progress_time(Instant::now());
        }
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    #[cfg(feature="cpu-debugger")]
    pub fn step_instruction_in(&mut self) {
        self.nes.step_instruction_in();
    }

    #[cfg(feature="cpu-debugger")]
    pub fn step_instruction_over(&mut self) {
        self.temp_debug_breakpoint = Some(self.nes.add_tmp_step_over_breakpoint());
        self.set_paused(false);
    }

    #[cfg(feature="cpu-debugger")]
    pub fn step_instruction_out(&mut self) {
        self.temp_debug_breakpoint = self.nes.add_tmp_step_out_breakpoint();
        self.set_paused(false);
    }

    pub fn handle_window_event(&mut self, event: winit::event::WindowEvent) {
        match event {
            WindowEvent::ModifiersChanged(modifiers) => { self.modifiers = modifiers; },
            WindowEvent::KeyboardInput { input, .. } => {
                if let Some(keycode) = input.virtual_keycode {
                    if input.state == winit::event::ElementState::Released {
                        match keycode {
                            VirtualKeyCode::Escape => {
                                self.set_paused(!self.paused());
                            }
                            VirtualKeyCode::R if self.modifiers.contains(ModifiersState::CTRL) => {
                                self.nes.reset();
                            }
                            VirtualKeyCode::T if self.modifiers.contains(ModifiersState::CTRL) => {
                                self.nes.power_cycle(Instant::now());
                            }
                            VirtualKeyCode::S if self.modifiers.contains(ModifiersState::CTRL) => {

                                #[cfg(feature="macro-builder")]
                                self.macro_builder_view.save();
                            }
                            _ => {}
                        }
                    }

                    let button = match keycode {
                        VirtualKeyCode::Return => { Some(ControllerButton::Start) }
                        VirtualKeyCode::Space => { Some(ControllerButton::Select) }
                        VirtualKeyCode::A => { Some(ControllerButton::Left) }
                        VirtualKeyCode::D => { Some(ControllerButton::Right) }
                        VirtualKeyCode::W => { Some(ControllerButton::Up) }
                        VirtualKeyCode::S => { Some(ControllerButton::Down) }
                        VirtualKeyCode::Right => { Some(ControllerButton::A) }
                        VirtualKeyCode::Left => { Some(ControllerButton::B) }
                        _ => None
                    };
                    if let Some(button) = button {
                        if input.state == winit::event::ElementState::Pressed {
                            // run the macro builder hook first so it can see if the input is redundant
                            #[cfg(feature="macro-builder")]
                            self.macro_builder_view.controller_input_hook(&mut self.nes, button, true);
                            self.nes.system_mut().port1.press_button(button);
                        } else {
                            // run the macro builder hook first so it can see if the input is redundant
                            #[cfg(feature="macro-builder")]
                            self.macro_builder_view.controller_input_hook(&mut self.nes, button, false);
                            self.nes.system_mut().port1.release_button(button);
                        }
                    }
                }
            }
            _ => {

            }
        }
    }
}
