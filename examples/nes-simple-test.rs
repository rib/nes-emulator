use std::str;
use std::fs::File;
use std::io::prelude::*;
use egui_glow;
use glutin::event::VirtualKeyCode;

use rust_nes_emulator::prelude::*;

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

unsafe fn u8_slice_as_color32_slice(u8_data: &[u8]) -> &[egui::Color32] {
    debug_assert!(u8_data.len() % 4 == 0);
    std::slice::from_raw_parts::<egui::Color32>(u8_data.as_ptr() as *const egui::Color32, u8_data.len() / 4)
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
   
    let mut nes = Nes::new(PixelFormat::RGBA8888);

    let cartridge = Cartridge::from_ines_binary(|addr: usize| rom[addr]);
    nes.insert_cartridge(cartridge);

    nes.reset();
    
    // XXX: we only need a single framebuffer considering that egui will synchronously copy
    // the data anyway
    let mut framebuffer = nes.allocate_framebuffer();
    let fb_width = framebuffer.width();
    let fb_height = framebuffer.height();

    let mut textures = vec![];
    let mut current_texture = 0;
    
    {
        let painter = egui.painter_mut();

        for _i in 0..2 {
            let tex = painter.alloc_user_texture();
            textures.push(tex);

            {
                let rental = framebuffer.rent_data().unwrap();
                let pixels = unsafe { u8_slice_as_color32_slice(&rental.data) };
                painter.set_user_texture(tex, (fb_width, fb_height), pixels);
            }
        }
    }

    let mut frame_no = 0;
    event_loop.run(move |event, _, control_flow| {
        let mut redraw = || {
            egui.begin_frame(gl_window.window());

            nes.tick_frame(framebuffer.clone());
            
            let tex = textures[current_texture];
            {
                let rental = framebuffer.rent_data().unwrap();
                let fb_color_slice = unsafe { u8_slice_as_color32_slice(&rental.data) };
                let painter = egui.painter_mut();
                painter.set_user_texture(tex, (fb_width, fb_height), fb_color_slice);
            }

            current_texture += 1;
            if current_texture >= textures.len() {
                current_texture = 0;
            }
            frame_no += 1;

            if frame_no == 50 {
                let rental = framebuffer.rent_data().unwrap();
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
                imgbuf.save(format!("debug-frame-{}.png", frame_no)).unwrap();
            }
            let mut quit = false;

            egui::SidePanel::left("my_side_panel").show(egui.ctx(), |ui| {
                if ui.button("Quit").clicked() {
                    quit = true;
                }
            });
            egui::CentralPanel::default().show(egui.ctx(), |ui| {
                ui.add(egui::Image::new(tex, egui::Vec2::new((fb_width * 2) as f32, (fb_height * 2) as f32)));
            });

            let (_needs_repaint, shapes) = egui.end_frame(gl_window.window());

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
                    glutin::event::WindowEvent::KeyboardInput { input, .. } => {
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
                                let system = nes.system_mut();
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