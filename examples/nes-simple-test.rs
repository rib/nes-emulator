use std::slice;
use std::str;
use rust_nes_emulator::prelude::*;
use std::fs::File;
use std::io::prelude::*;
use egui_glow;
use glutin::event::VirtualKeyCode;

pub const EMBEDDED_EMULATOR_VISIBLE_SCREEN_WIDTH: usize = 256;
pub const EMBEDDED_EMULATOR_VISIBLE_SCREEN_HEIGHT: usize = 240;

pub const EMBEDDED_EMULATOR_PLAYER_0: u32 = 0;
pub const EMBEDDED_EMULATOR_PLAYER_1: u32 = 1;

#[derive(PartialEq, Eq)]
#[repr(u8)]
pub enum CpuInterrupt {
    NMI,
    RESET,
    IRQ,
    BRK,
    NONE,
}

fn get_file_as_byte_vec(filename: &str) -> Vec<u8> {
    println!("Loading {}", filename);
    let mut f = File::open(&filename).expect("no file found");
    let metadata = std::fs::metadata(&filename).expect("unable to read metadata");
    let mut buffer = vec![0; metadata.len() as usize];
    f.read(&mut buffer).expect("buffer overflow");

    buffer
}


fn create_display(
    event_loop: &glutin::event_loop::EventLoop<()>,
) -> (
    glutin::WindowedContext<glutin::PossiblyCurrent>,
    glow::Context,
) {
    let window_builder = glutin::window::WindowBuilder::new()
        .with_resizable(true)
        .with_inner_size(glutin::dpi::LogicalSize {
            width: 800.0,
            height: 600.0,
        })
        .with_title("egui_glow example");

    let gl_window = unsafe {
        glutin::ContextBuilder::new()
            .with_depth_buffer(0)
            .with_srgb(true)
            .with_stencil_buffer(0)
            .with_vsync(true)
            .build_windowed(window_builder, event_loop)
            .unwrap()
            .make_current()
            .unwrap()
    };

    let gl = unsafe { glow::Context::from_loader_function(|s| gl_window.get_proc_address(s)) };

    unsafe {
        use glow::HasContext as _;
        gl.enable(glow::FRAMEBUFFER_SRGB);
    }

    (gl_window, gl)
}

fn main() {
    env_logger::builder().filter_level(log::LevelFilter::Warn) // Default Log Level
                         .parse_default_env()
                         .format(pretty_env_logger::formatter)
                         .init();


    let event_loop = glutin::event_loop::EventLoop::with_user_event();
    let (gl_window, gl) = create_display(&event_loop);

    let mut egui = egui_glow::EguiGlow::new(&gl_window, &gl);

    let rom = get_file_as_byte_vec(&std::env::args().nth(1).expect("Expected path to .nes ROM"));
    
    let scale = 2;
    let screen_width = EMBEDDED_EMULATOR_VISIBLE_SCREEN_WIDTH;
    let screen_height = EMBEDDED_EMULATOR_VISIBLE_SCREEN_HEIGHT;
    let fb_size = screen_width * screen_height * 4;
    println!("FB size = {}", fb_size);

    let mut fb_buf = vec![0u8; fb_size];
    let mut cpu = Cpu::default();
    let mut system = System::default();
    let mut ppu = Ppu::default();
    ppu.draw_option.fb_width = screen_width as u32;
    ppu.draw_option.fb_height = screen_height as u32;
    ppu.draw_option.offset_x = 0;
    ppu.draw_option.offset_y = 0;
    ppu.draw_option.scale = 1;
    ppu.draw_option.pixel_format = PixelFormat::RGBA8888;

    let cartridge = Cartridge::from_ines_binary(|addr: usize| rom[addr]);
    system.cartridge = cartridge;

    cpu.reset();
    system.reset();
    ppu.reset();
    cpu.interrupt(&mut system, Interrupt::RESET);

    let mut frame_no = 0;
    let mut buffers = vec![];
    
    {
        let painter = egui.painter_mut();

        for _i in 0..2 {
            let tex = painter.alloc_user_texture();
            buffers.push(tex);
            println!("uploading tex, w={}, h = {}", screen_width, screen_height);
            let red_buf: Vec<egui::Color32> = vec![egui::Color32::RED; screen_width * screen_height];
            painter.set_user_texture(tex, (screen_width, screen_height), &red_buf);
        }
    }
    let mut buffer_pos = 0;

    event_loop.run(move |event, _, control_flow| {
        let mut redraw = || {
            egui.begin_frame(gl_window.window());

            let cyc_per_frame = CYCLE_PER_DRAW_FRAME;

            println!("Frame {}", frame_no);
            let mut i = 0;
            while i < cyc_per_frame {
                let cyc = cpu.step(&mut system);
                i += cyc as usize;

                let irq = ppu.step(cyc.into(), &mut system, fb_buf.as_mut_ptr());
                
                if let Some(irq) = irq {
                    cpu.interrupt(&mut system, irq);
                }
            }
            let tex = buffers[buffer_pos];
            {
                let fb_as_color_slice= unsafe { std::slice::from_raw_parts::<egui::Color32>(fb_buf.as_ptr() as *const egui::Color32, fb_buf.len() / 4) };
                let painter = egui.painter_mut();
                painter.set_user_texture(tex, (screen_width, screen_height), fb_as_color_slice);
            }

            buffer_pos += 1;
            if buffer_pos >= buffers.len() {
                buffer_pos = 0;
            }
            frame_no += 1;

            if frame_no == 50 {
                let stride = screen_width * 4;
                let mut imgbuf = image::ImageBuffer::new(screen_width as u32, screen_height as u32);
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
                imgbuf.save(format!("debug-frame-{}.png", frame_no)).unwrap();
            }
            let mut quit = false;

            egui::SidePanel::left("my_side_panel").show(egui.ctx(), |ui| {
                if ui.button("Quit").clicked() {
                    quit = true;
                }
            });
            egui::CentralPanel::default().show(egui.ctx(), |ui| {
                ui.add(egui::Image::new(tex, egui::Vec2::new((screen_width * 2) as f32, (screen_height * 2) as f32)));
            });

            let (needs_repaint, shapes) = egui.end_frame(gl_window.window());

            *control_flow = if quit {
                glutin::event_loop::ControlFlow::Exit
            } else {
                gl_window.window().request_redraw();
                glutin::event_loop::ControlFlow::Poll
            };
            /*else if needs_repaint {
                gl_window.window().request_redraw();
                glutin::event_loop::ControlFlow::Poll
            } else {
                glutin::event_loop::ControlFlow::Wait
            };*/


            {
                let color = egui::Rgba::from_rgb(0.1, 0.3, 0.2);
                unsafe {
                    use glow::HasContext as _;
                    gl.clear_color(color[0], color[1], color[2], color[3]);
                    gl.clear(glow::COLOR_BUFFER_BIT);
                }

                // draw things behind egui here

                egui.paint(&gl_window, &gl, shapes);

                // draw things on top of egui here

                gl_window.swap_buffers().unwrap();
            }
        };

        match event {
            // Platform-dependent event handlers to workaround a winit bug
            // See: https://github.com/rust-windowing/winit/issues/987
            // See: https://github.com/rust-windowing/winit/issues/1619
            glutin::event::Event::RedrawEventsCleared if cfg!(windows) => redraw(),
            glutin::event::Event::RedrawRequested(_) if !cfg!(windows) => redraw(),

            glutin::event::Event::WindowEvent { event, .. } => {
                match event {
                    glutin::event::WindowEvent::KeyboardInput { device_id, input, .. } => {
                        if let Some(keycode) = input.virtual_keycode {
                            let button = match keycode {
                                VirtualKeyCode::Return => { Some(PadButton::Start) }
                                VirtualKeyCode::Space => { Some(PadButton::Select) }
                                VirtualKeyCode::A => { Some(PadButton::Left) }
                                VirtualKeyCode::D => { Some(PadButton::Right) }
                                VirtualKeyCode::W => { Some(PadButton::Up) }
                                VirtualKeyCode::S => { Some(PadButton::Down) }
                                VirtualKeyCode::Left => { Some(PadButton::A) }
                                VirtualKeyCode::Right => { Some(PadButton::B) }
                                _ => None
                            };
                            if let Some(button) = button {
                                if input.state == glutin::event::ElementState::Pressed {
                                    system.pad1.push_button(button);
                                } else {
                                    system.pad1.release_button(button);
                                }
                            }
                        }
                    }
                    _ => {}
                }
                if egui.is_quit_event(&event) {
                    *control_flow = glutin::event_loop::ControlFlow::Exit;
                }

                if let glutin::event::WindowEvent::Resized(physical_size) = event {
                    gl_window.resize(physical_size);
                }

                egui.on_event(&event);

                gl_window.window().request_redraw(); // TODO: ask egui if the events warrants a repaint instead
            }
            glutin::event::Event::LoopDestroyed => {
                egui.destroy(&gl);
            }
            _ => (),
        }
    });
}