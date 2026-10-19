//! No input interception or frame state on macOS.

use egui::{Context, Id, Response, ScrollArea, Ui};

#[derive(Default)]
pub struct Autoscroll {
    _private: (),
}

#[derive(Default)]
pub struct Outcome {
    pub scrolling: bool,
    pub stop_following_lyrics: bool,
}

impl Autoscroll {
    pub fn active(&self) -> bool {
        false
    }
    pub fn cancel_if_unfocused(&mut self, _ctx: &Context) -> bool {
        false
    }
    pub fn begin(&mut self, _ctx: &Context, _enabled: bool) {}
    pub fn finish(&mut self, _ctx: &Context, _enabled: bool) -> Outcome {
        Outcome::default()
    }
}

pub fn row(_ui: &Ui, _response: &Response) {}

pub fn show<R>(
    ui: &mut Ui,
    area: ScrollArea,
    _axes: egui::Vec2b,
    contents: impl FnOnce(&mut Ui) -> R,
) -> egui::scroll_area::ScrollAreaOutput<R> {
    area.show(ui, contents)
}

pub fn lyrics(_ui: &Ui, _id: Id) {}
