//#![allow(unused)]
// There's some kind of compiler bug going on, causing a crazy amount of false
// positives atm :(
#![allow(dead_code)]

use std::thread;
use std::time::{Duration, Instant};

use log::{error, warn, info, debug, trace};
use clap::Parser;

use anyhow::Result;

mod ui;

use winit::event_loop::{EventLoopWindowTarget, EventLoop};

use winit::{
    event_loop::{ControlFlow},
};

use egui_wgpu::winit::Painter;
use egui_winit::State;
//use egui_winit_platform::{Platform, PlatformDescriptor};
use winit::event::Event::*;


const INITIAL_WIDTH: u32 = 1920;
const INITIAL_HEIGHT: u32 = 1080;

/// Enable egui to request redraws via a custom Winit event...
#[derive(Clone)]
struct RepaintSignal(std::sync::Arc<std::sync::Mutex<winit::event_loop::EventLoopProxy<Event>>>);


fn create_window<T>(event_loop: &EventLoopWindowTarget<T>, state: &mut State, painter: &mut Painter) -> winit::window::Window {
    let window = winit::window::WindowBuilder::new()
        .with_decorations(true)
        .with_resizable(true)
        .with_transparent(false)
        .with_title("NES Emulator")
        .with_inner_size(winit::dpi::PhysicalSize {
            width: INITIAL_WIDTH,
            height: INITIAL_HEIGHT,
        })
            .build(&event_loop)
            .unwrap();

    unsafe { painter.set_window(Some(&window)) };

    // NB: calling set_window will lazily initialize render state which
    // means we will be able to query the maximum supported texture
    // dimensions
    if let Some(max_size) = painter.max_texture_side() {
        state.set_max_texture_side(max_size);
    }

    let pixels_per_point = window.scale_factor() as f32;
    state.set_pixels_per_point(pixels_per_point);

    window.request_redraw();

    window
}

pub enum Event {
    RequestRedraw
}


fn _main() -> Result<()> {
    let args = ui::Args::parse();

    let event_loop: winit::event_loop::EventLoop<Event> = EventLoop::with_user_event();

    let ctx = egui::Context::default();
    let repaint_signal = RepaintSignal(std::sync::Arc::new(std::sync::Mutex::new(
        event_loop.create_proxy()
    )));
    ctx.set_request_repaint_callback(move || {
        repaint_signal.0.lock().unwrap().send_event(Event::RequestRedraw).ok();
    });

    let mut winit_state = egui_winit::State::new(&event_loop);
    let mut painter = egui_wgpu::winit::Painter::new(
        wgpu::Backends::all(),
        wgpu::PowerPreference::LowPower,
        wgpu::DeviceDescriptor {
            label: None,
            features: wgpu::Features::default(),
            limits: wgpu::Limits::default()
        },
        wgpu::PresentMode::Fifo,
        1);
    let mut window: Option<winit::window::Window> = None;
    let mut emulator_ui = ui::EmulatorUi::new(&args, &ctx)?;

    window = Some(create_window(&event_loop, &mut winit_state, &mut painter));

    event_loop.run(move |event, event_loop, control_flow| {

        match event {
            RedrawRequested(..) => {
                if let Some(window) = window.as_ref() {
                    let mut raw_input = winit_state.take_egui_input(window);
                    emulator_ui.update();

                    let full_output = ctx.run(raw_input, |ctx| {
                        match emulator_ui.draw(ctx) {
                            ui::Status::Ok => {}
                            ui::Status::Quit => {
                                *control_flow = winit::event_loop::ControlFlow::Exit;
                            }
                        }
                    });
                    winit_state.handle_platform_output(window, &ctx, full_output.platform_output);

                    painter.paint_and_update_textures(winit_state.pixels_per_point(),
                        egui::Rgba::default(),
                        &ctx.tessellate(full_output.shapes),
                        &full_output.textures_delta);

                    // This seems like some pretty funky API design :/
                    //
                    // Winit should probably have some kind of timers API to remove the need for
                    // WaitUntil and there should maybe be an explicit .quit() API for the event
                    // loop that would avoid this awkward control_flow precedence issue.
                    //
                    // It also seems pretty odd for Egui to specify a wait _duration_ when it
                    // doesn't know when we will start waiting - surely they should specify
                    // an optional deadline instant instead.
                    if *control_flow != winit::event_loop::ControlFlow::Exit {
                        *control_flow = if full_output.repaint_after.is_zero() || emulator_ui.paused == false  {
                            window.request_redraw();
                            winit::event_loop::ControlFlow::Poll
                        } else if let Some(repaint_after_instant) =
                            std::time::Instant::now().checked_add(full_output.repaint_after)
                        {
                            // if repaint_after is something huge and can't be added to Instant,
                            // we will use `ControlFlow::Wait` instead.
                            // technically, this might lead to some weird corner cases where the user *WANTS*
                            // winit to use `WaitUntil(MAX_INSTANT)` explicitly. they can roll their own
                            // egui backend impl i guess.
                            winit::event_loop::ControlFlow::WaitUntil(repaint_after_instant)
                        } else {
                            winit::event_loop::ControlFlow::Wait
                        };
                    }
                }
            }
            MainEventsCleared | UserEvent(Event::RequestRedraw) => {
                if let Some(window) = window.as_ref() {
                    window.request_redraw();
                }
            }
            UserEvent(ref user_event) => {
            }
            WindowEvent { event, .. } => {
                if winit_state.on_event(&ctx, &event) == false {
                    match event {
                        winit::event::WindowEvent::Resized(size) => {
                            painter.on_window_resized(size.width, size.height);
                        }
                        winit::event::WindowEvent::CloseRequested => {
                            *control_flow = ControlFlow::Exit;
                        }
                        event => { emulator_ui.handle_window_event(event); }
                    }
                }
            },
            _ => (),
        }
    });
}


fn main() -> Result<()> {
    env_logger::builder().filter_level(log::LevelFilter::Trace) // Default Log Level
        .filter(Some("naga"), log::LevelFilter::Warn)
        .filter(Some("wgpu"), log::LevelFilter::Warn)
        .parse_default_env()
        .init();

    _main()
}