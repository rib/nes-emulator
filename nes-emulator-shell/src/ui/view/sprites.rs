use std::{cell::RefCell, rc::Rc};

use egui::{pos2, TextureHandle};
use nes_emulator::{
    constants::*,
    framebuffer::{Framebuffer, FramebufferClearMode, FramebufferDataRental, PixelFormat},
    hook::HookHandle,
    nes::Nes,
};

use crate::ui::{blank_texture_for_framebuffer, full_framebuffer_image_delta};

struct SpritesHookState {
    screen_framebuffer_front: FramebufferDataRental,
}

pub struct SpritesView {
    pub visible: bool,
    screen_texture: TextureHandle,
    queue_screen_fb_upload: bool,

    mux_hook_handle: Option<HookHandle>,
    dot_hook_handle: Option<HookHandle>,
    hook_state: Rc<RefCell<SpritesHookState>>,

    hover_pos: [usize; 2],
}

impl SpritesView {
    pub fn new(ctx: &egui::Context) -> Self {
        let screen_framebuffer_front =
            Framebuffer::new(FRAME_WIDTH, FRAME_HEIGHT, PixelFormat::RGB888);
        let screen_framebuffer_front = screen_framebuffer_front.rent_data().unwrap();
        let screen_texture = blank_texture_for_framebuffer(
            ctx,
            &screen_framebuffer_front,
            "sprites_screen_framebuffer",
        );

        Self {
            visible: false,

            screen_texture,
            queue_screen_fb_upload: false,

            mux_hook_handle: None,
            dot_hook_handle: None,
            hook_state: Rc::new(RefCell::new(SpritesHookState {
                screen_framebuffer_front,
            })),

            hover_pos: [0, 0],
        }
    }

    pub fn set_visible(&mut self, nes: &mut Nes, visible: bool) {
        self.visible = visible;
        if visible {
            let shared = self.hook_state.clone();

            // By giving the closure ownership of the back buffer then this per-pixel hook
            // avoids needing to poke into the Rc<RefCell<>> every pixel and only needs
            // to swap the back buffer into the shared state at the end of the frame
            let screen_framebuffer_back =
                Framebuffer::new(FRAME_WIDTH, FRAME_HEIGHT, PixelFormat::RGB888);
            let mut screen_framebuffer_back = screen_framebuffer_back.rent_data().unwrap();
            self.mux_hook_handle = Some(nes.ppu_mut().add_mux_hook(Box::new(
                move |_ppu, _cartridge, state| {
                    //println!("screen x = {}, y = {}, enabled = {}", state.screen_x, state.screen_y, state.rendering_enabled);
                    if state.screen_x == 0 && state.screen_y == 0 {
                        screen_framebuffer_back
                            .clear(FramebufferClearMode::Checkerboard(0x80, 0xa0));
                    }
                    if state.sprite_pattern != 0 {
                        let color = nes_emulator::ppu_palette::rgb_lut(state.sprite_palette_value);
                        screen_framebuffer_back.plot(
                            state.screen_x as usize,
                            state.screen_y as usize,
                            color,
                        );
                    }
                    if state.screen_x == 255 && state.screen_y == 239 {
                        std::mem::swap(
                            &mut screen_framebuffer_back,
                            &mut shared.borrow_mut().screen_framebuffer_front,
                        );
                        //println!("Finished frame");
                    }
                },
            )));

            self.dot_hook_handle = Some(nes.ppu_mut().add_dot_hook(
                240,
                0,
                Box::new(move |_ppu, _cartridge| {
                    //self.queue_screen_fb_upload = true;
                }),
            ));
        } else {
            if let Some(handle) = self.mux_hook_handle {
                nes.ppu_mut().remove_mux_hook(handle);
                self.mux_hook_handle = None;
            }
            if let Some(handle) = self.dot_hook_handle {
                nes.ppu_mut().remove_dot_hook(240, 0, handle);
                self.dot_hook_handle = None;
            }
        }
    }

    pub fn update(&mut self, _nes: &mut Nes) {
        /*
        let bpp = 3;
        let stride = FRAME_WIDTH * bpp;

        for y in 0..FRAME_HEIGHT {
            for x in 0..FRAME_WIDTH {
                let pix = nes.debug_sample_sprites(x, y);
                let pos = y * stride + x * bpp;
                self.screen_framebuffer[pos + 0] = pix[0];
                self.screen_framebuffer[pos + 1] = pix[1];
                self.screen_framebuffer[pos + 2] = pix[2];
            }
        }

        */

        self.queue_screen_fb_upload = true;
    }

    pub fn draw(&mut self, _nes: &mut Nes, ctx: &egui::Context) {
        if self.queue_screen_fb_upload {
            let _hook_state = self.hook_state.borrow();
            let copy =
                full_framebuffer_image_delta(&self.hook_state.borrow().screen_framebuffer_front);
            ctx.tex_manager()
                .write()
                .set(self.screen_texture.id(), copy);
            self.queue_screen_fb_upload = false;
        }
        egui::Window::new("Sprites")
            .default_width(900.0)
            .resizable(true)
            //.resize(|r| r.auto_sized())
            .show(ctx, |ui| {
                let panels_width = ui.fonts(|f| f.pixels_per_point() * 100.0);

                egui::SidePanel::left("sprites_options_panel")
                    .resizable(false)
                    .min_width(panels_width)
                    .show_inside(ui, |_ui| {
                        //ui.checkbox(&mut view.show_scroll, "Show Scroll Position");
                    });
                egui::SidePanel::right("sprites_properties_panel")
                    .resizable(false)
                    .min_width(panels_width)
                    .show_inside(ui, |_ui| {
                        //ui.label(format!("Scroll X: {}", self.nes.system_ppu().scroll_x()));
                        //ui.label(format!("Scroll Y: {}", self.nes.system_ppu().scroll_y()));
                    });

                egui::TopBottomPanel::bottom("sprites_footer").show_inside(ui, |ui| {
                    ui.label(format!("[{}, {}]", self.hover_pos[0], self.hover_pos[1]));
                });

                //let frame = Frame::none().outer_margin(Margin::same(200.0));
                egui::CentralPanel::default()
                    //.frame(frame)
                    .show_inside(ui, |ui| {
                        let (response, painter) = ui.allocate_painter(
                            egui::Vec2::new(FRAME_WIDTH as f32, FRAME_HEIGHT as f32),
                            egui::Sense::hover(),
                        );

                        let _img = egui::Image::new(
                            self.screen_texture.id(),
                            egui::Vec2::new(FRAME_WIDTH as f32, FRAME_HEIGHT as f32),
                        );
                        //let response = ui.add(egui::Image::new(self.nametables_texture.id(), egui::Vec2::new(width as f32, height as f32)));
                        // TODO(emilk): builder pattern for Mesh

                        let mut mesh = egui::Mesh::with_texture(self.screen_texture.id());
                        mesh.add_rect_with_uv(
                            response.rect,
                            egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                        painter.add(egui::Shape::mesh(mesh));

                        let img_pos = response.rect.left_top();
                        let img_width = response.rect.width();
                        let _img_height = response.rect.height();
                        let img_to_nes_px = FRAME_WIDTH as f32 / img_width;
                        let _nes_px_to_img = 1.0 / img_to_nes_px;

                        if let Some(hover_pos) = response.hover_pos() {
                            let x = ((hover_pos.x - img_pos.x) * img_to_nes_px) as usize;
                            let y = ((hover_pos.y - img_pos.y) * img_to_nes_px) as usize;
                            self.hover_pos = [x, y];

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
}
