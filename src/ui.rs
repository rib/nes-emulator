// There's some kind of compiler bug going on, causing a crazy amount of false
// positives atm :(
#![allow(dead_code)]

use std::{collections::{HashSet, HashMap, VecDeque}, time::{Instant, Duration}};

use log::{error, warn, info, debug, trace};
use serde::{de, Deserialize};
use lazy_static::lazy_static;

use tokio::sync::mpsc::UnboundedSender;
use egui::{self, RichText, Color32, Ui};



#[derive(Debug, Clone)]
pub enum Event {
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


#[derive(Default)]
pub struct State {
    notices: VecDeque<Notice>,

}

impl State {

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


    pub fn draw(&mut self, ctx: &egui::Context, ble_tx: &UnboundedSender<ble::BleRequest>) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            self.draw_notices_header(ui);

        });

        egui::CentralPanel::default().show(ctx, |ui| {

        });
    }

    pub fn handle_event(&mut self, event: Event, ble_tx: &UnboundedSender<ble::BleRequest>) {
        match event {

            Event::ShowText(level, text) => {
                self.notices.push_back(Notice { level, text, timestamp: Instant::now() });
            }
            _ => {

            }
        }
    }
}