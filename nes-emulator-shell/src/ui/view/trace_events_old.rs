
use egui::{TextureHandle, ColorImage, ImageData, epaint::ImageDelta, pos2};

const TRACE_EVENTS_FB_WIDTH: usize = 341;
const TRACE_EVENTS_FB_HEIGHT: usize = 262;
pub struct TraceEventsView {
    pub visible: bool,
    framebuffer: Vec<u8>,
    texture: TextureHandle,
    // Pixel coordinate within four namespace regions
    hover_pos: [usize; 2],
    queue_fb_upload: bool,
}
impl TraceEventsView {
    pub fn new(ctx: &egui::Context) -> Self {

        let screen_framebuffer_front = Framebuffer::new(FRAME_WIDTH, FRAME_HEIGHT, PixelFormat::RGB888);
        let screen_framebuffer_front = screen_framebuffer_front.rent_data().unwrap();
        let screen_texture = blank_texture_for_framebuffer(ctx, &screen_framebuffer_front, "sprites_screen_framebuffer");

        let fb_bpp = 3;
        let fb_stride = TRACE_EVENTS_FB_WIDTH * fb_bpp;
        let framebuffer = vec![0u8; fb_stride * TRACE_EVENTS_FB_HEIGHT];
        let texture = {
            //let blank = vec![egui::epaint::Color32::default(); fb_width * fb_height];
            let blank = ColorImage {
                size: [TRACE_EVENTS_FB_WIDTH as _, TRACE_EVENTS_FB_HEIGHT as _],
                pixels: vec![egui::Color32::default(); TRACE_EVENTS_FB_WIDTH * TRACE_EVENTS_FB_HEIGHT],
            };
            let blank = ImageData::Color(blank);
            let tex = ctx.load_texture("trace_events_framebuffer", blank, egui::TextureFilter::Nearest);
            tex
        };

        Self {
            visible: false,
            framebuffer,
            texture,
            hover_pos: [0, 0],
            queue_fb_upload: false,
        }
    }

    pub fn draw(&mut self, ctx: &egui::Context) {
        if self.queue_fb_upload {
            let copy = ImageDelta::full(ImageData::Color(ColorImage {
                size: [TRACE_EVENTS_FB_WIDTH, TRACE_EVENTS_FB_HEIGHT],
                pixels: self.framebuffer.chunks_exact(3)
                    .map(|p| egui::Color32::from_rgba_premultiplied(p[0], p[1], p[2], 255))
                    .collect(),
            }), egui::TextureFilter::Nearest);

            ctx.tex_manager().write().set(self.texture.id(), copy);
            self.queue_fb_upload = false;
        }

        egui::Window::new("Trace Events")
            .default_width(900.0)
            .resizable(true)
            //.resize(|r| r.auto_sized())
            .show(ctx, |ui| {

                let panels_width = ui.fonts().pixels_per_point() * 100.0;

                egui::SidePanel::left("trace_events_options_panel")
                    .resizable(false)
                    .min_width(panels_width)
                    .show_inside(ui, |_ui| {
                        //ui.checkbox(&mut view.show_scroll, "Show Scroll Position");
                    });
                egui::SidePanel::right("trace_events_properties_panel")
                    .resizable(false)
                    .min_width(panels_width)
                    .show_inside(ui, |_ui| {
                        //ui.label(format!("Scroll X: {}", self.nes.system_ppu().scroll_x()));
                        //ui.label(format!("Scroll Y: {}", self.nes.system_ppu().scroll_y()));
                });

                egui::TopBottomPanel::bottom("trace_events_footer").show_inside(ui, |ui| {
                    ui.label(format!("[{}, {}]", self.hover_pos[0], self.hover_pos[1]));
                });

                //let frame = Frame::none().outer_margin(Margin::same(200.0));
                egui::CentralPanel::default()
                    //.frame(frame)
                    .show_inside(ui, |ui| {

                        let (response, painter) =
                            ui.allocate_painter(egui::Vec2::new(TRACE_EVENTS_FB_WIDTH as f32, TRACE_EVENTS_FB_HEIGHT as f32), egui::Sense::hover());

                        let _img = egui::Image::new(self.texture.id(), egui::Vec2::new(TRACE_EVENTS_FB_WIDTH as f32, TRACE_EVENTS_FB_HEIGHT as f32));
                        //let response = ui.add(egui::Image::new(self.nametables_texture.id(), egui::Vec2::new(width as f32, height as f32)));
                                        // TODO(emilk): builder pattern for Mesh

                        let mut mesh = egui::Mesh::with_texture(self.texture.id());
                        mesh.add_rect_with_uv(response.rect, egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), egui::Color32::WHITE,);
                        painter.add(egui::Shape::mesh(mesh));

                        let img_pos = response.rect.left_top();
                        let img_width = response.rect.width();
                        let _img_height = response.rect.height();
                        let img_to_nes_px = TRACE_EVENTS_FB_WIDTH as f32 / img_width;
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