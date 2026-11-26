// Copyright (C) 2026 Lemon Studio contributors
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Lemon Pitch — original implementation (see params.rs note).

//! The plugin editor: knob drags, segment block drags (retune a segment), the sample file dialog and the
//! parameter menus, following the other Lemon instruments' editor pattern. Edits show and sound at once.

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

use nih_plug::prelude::*;
use os_plugkit::MenuDef;
use os_plugkit::live::LiveEdits;
use os_plugkit::vals::Vals;
use os_ui::kit::menu::{MenuItem, MenuStyle};

use crate::dsp::Shared;
use crate::params::PitchParams;
use crate::view::{self, Hit, Target, ViewState};

pub struct PitchEditor {
    pub params: Arc<PitchParams>,
    pub shared: Arc<Shared>,
    /// DPI scale as f32 bits.
    pub scale: AtomicU32,
    #[cfg_attr(not(windows), allow(dead_code))]
    hwnd: Arc<AtomicIsize>,
    #[cfg(any(target_os = "linux", target_os = "macos", feature = "embed"))]
    slot: os_plugkit::gui::Slot,
    live: Arc<LiveEdits>,
}

impl PitchEditor {
    pub fn new(params: Arc<PitchParams>, shared: Arc<Shared>) -> Self {
        PitchEditor {
            params,
            shared,
            scale: AtomicU32::new(1f32.to_bits()),
            hwnd: Arc::new(AtomicIsize::new(0)),
            #[cfg(any(target_os = "linux", target_os = "macos", feature = "embed"))]
            slot: Default::default(),
            live: Arc::new(LiveEdits::default()),
        }
    }

    fn notify(&self) {
        #[cfg(any(target_os = "linux", target_os = "macos", feature = "embed"))]
        os_plugkit::gui::notify(&self.slot);
    }
}

impl Editor for PitchEditor {
    fn spawn(
        &self,
        parent: ParentWindowHandle,
        context: Arc<dyn GuiContext>,
    ) -> Box<dyn Any + Send> {
        let scale = f32::from_bits(self.scale.load(Ordering::Relaxed));
        match parent {
            #[cfg(any(target_os = "linux", all(feature = "embed", not(target_os = "macos"))))]
            ParentWindowHandle::X11Window(window) => Box::new(gui::open(
                window,
                scale,
                self.params.clone(),
                self.shared.clone(),
                self.live.clone(),
                context,
                &self.slot,
            )),
            #[cfg(target_os = "macos")]
            ParentWindowHandle::AppKitNsView(view) => {
                let scale = os_plugkit::gui::backing_scale(view as usize);
                Box::new(gui::open(
                    view as usize,
                    scale,
                    self.params.clone(),
                    self.shared.clone(),
                    self.live.clone(),
                    context,
                    &self.slot,
                ))
            }
            _ => {
                let _ = (context, scale);
                Box::new(())
            }
        }
    }

    fn size(&self) -> (u32, u32) {
        (view::EDITOR_W, view::EDITOR_H)
    }

    fn set_scale_factor(&self, factor: f32) -> bool {
        self.scale.store(factor.to_bits(), Ordering::Relaxed);
        true
    }

    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {
        self.notify();
    }
    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {
        self.notify();
    }
    fn param_values_changed(&self) {
        self.notify();
    }
}

/// Pixels of drag for a knob's whole range.
pub const KNOB_PX: f32 = 200.0;
const FINE: f32 = 0.25;

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    None,
    Reset(String),
    CopyValue(String),
    PasteValue(String),
    TypeIn(String),
    Automate(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuReq {
    Param(String),
}

/// Something the window has to do after an event.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Repaint,
    /// Hide the cursor and hold it in place (knob drag).
    Lock,
    Menu(MenuReq, f32, f32),
    TypeIn(String, f32, f32),
    /// Ask the host for an automation clip of parameter `index` (host link).
    Automate(usize),
    /// Open the file dialog for the sample to correct.
    OpenDialog,
    /// Render and export the corrected audio to the sample library.
    Export,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Btn {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Debug)]
enum Drag {
    Knob {
        id: String,
        v: f32,
    },
    /// A segment block: the pointer's start y and the target it started from.
    Block {
        seg: usize,
        y0: f32,
        start: f32,
    },
    /// Panning the grid: the pointer start, the window's start frame and top note at grab time.
    Pan {
        x0: f32,
        y0: f32,
        start0: f32,
        hi0: f32,
    },
}

pub struct Logic {
    pub vals: Vals,
    pub params: Arc<PitchParams>,
    pub shared: Arc<Shared>,
    pub view: ViewState,
    pub hits: Vec<Hit>,
    drag: Option<Drag>,
    pub hint: String,
    copied: Option<f32>,
    pub hosted: bool,
}

impl Logic {
    pub fn new(
        params: Arc<PitchParams>,
        shared: Arc<Shared>,
        live: Arc<LiveEdits>,
        ctx: Option<Arc<dyn GuiContext>>,
    ) -> Self {
        let vals = Vals::new(&*params, live, ctx);
        let mut l = Logic {
            vals,
            params,
            shared,
            view: ViewState::default(),
            hits: Vec::new(),
            drag: None,
            hint: String::new(),
            copied: None,
            hosted: false,
        };
        l.sync_data();
        l
    }

    /// Takes the shared sample/analysis into the view when they changed.
    pub fn sync_data(&mut self) {
        let v = self.shared.version();
        if v != self.view.data.version {
            self.view.data = view::DataView {
                sample: self.shared.sample(),
                analysis: self.shared.analysis(),
                path: self.shared.loaded_path(),
                version: v,
            };
            // Frame the detected notes on the grid, at full horizontal zoom.
            if let Some(a) = &self.view.data.analysis {
                let mut lo = f32::MAX;
                let mut hi = f32::MIN;
                for s in &a.segments {
                    lo = lo.min(s.median);
                    hi = hi.max(s.median);
                }
                if hi >= lo {
                    self.view.note_hi = (hi + 8.0).clamp(45.0, 110.0);
                }
                self.view.zoom_x = 1.0;
                self.view.scroll = 0.0;
            }
        }
    }

    /// Loads `path` as the sample to correct (the file dialog's pick); the hint says how it went.
    pub fn load(&mut self, path: &str) {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path).to_string();
        self.hint = match self.shared.load(path) {
            Ok(()) => {
                if let Ok(mut p) = self.params.sample_path.write() {
                    *p = path.to_string();
                }
                format!("Loaded {name}")
            }
            Err(_) => format!("{name} could not be read"),
        };
        self.sync_data();
    }

    /// Renders the corrected audio and exports it to the sample library (the drag-out equivalent);
    /// the hint reports where it went.
    pub fn export(&mut self) {
        self.hint = match crate::dsp::export_corrected(&self.shared, &self.params) {
            Ok(name) => format!("Exported {name} — find it in the sample library"),
            Err(e) => format!("Export failed: {e}"),
        };
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    fn hit(&self, x: f32, y: f32) -> Option<Target> {
        self.hits
            .iter()
            .rev()
            .find(|h| h.r.contains(x, y))
            .map(|h| h.t.clone())
    }

    fn hint_for(&self, t: Option<&Target>) -> String {
        match t {
            Some(t) => view::hint(t, &self.vals, &self.view),
            None => String::new(),
        }
    }

    fn reset(&mut self, id: &str) {
        let d = self.vals.default_norm(id);
        self.vals.edit(id, d);
    }

    fn total(&self) -> usize {
        self.view.data.sample.as_ref().map_or(0, |s| s.frames())
    }

    /// Horizontal zoom by `factor`, keeping the frame under `at_x` (or the window centre) in place.
    pub fn zoom_h(&mut self, factor: f32, at_x: Option<f32>) {
        let total = self.total();
        if total == 0 {
            return;
        }
        let g = view::grid_rect();
        let x = at_x.unwrap_or(g.cx());
        let f0 = view::frame_at_x(&self.view, x, total);
        self.view.zoom_x = (self.view.zoom_x * factor).clamp(1.0, view::MAX_ZOOM_X);
        let (_, win) = self.view.window(total);
        let start = f0 - (x - g.x) / g.w * win;
        let range = (total as f32 - win).max(1.0);
        self.view.scroll = (start / range).clamp(0.0, 1.0);
    }

    /// Vertical zoom by `factor` (more = fewer, larger notes), keeping the note under `at_y` in place.
    pub fn zoom_v(&mut self, factor: f32, at_y: Option<f32>) {
        let g = view::grid_rect();
        let y = at_y.unwrap_or(g.cy());
        let n0 = view::note_at_y(self.view.note_hi, self.view.visible, y);
        self.view.visible =
            (self.view.visible / factor).clamp(view::MIN_VISIBLE, view::MAX_VISIBLE);
        let rh = view::row_h(self.view.visible);
        self.view.note_hi = (n0 + (y - g.y) / rh).clamp(20.0, 127.0);
    }

    /// Zoom full: the whole sample and the default pitch range.
    pub fn zoom_full(&mut self) {
        self.view.zoom_x = 1.0;
        self.view.scroll = 0.0;
        self.view.visible = 30.0;
    }

    pub fn down(&mut self, x: f32, y: f32, btn: Btn, m: Mods, _dbl: bool) -> Vec<Act> {
        self.view.mouse = Some((x, y));
        let Some(t) = self.hit(x, y) else {
            return vec![];
        };
        let mut acts = vec![Act::Repaint];
        match (&t, btn) {
            (Target::Knob(id), Btn::Left) if m.alt => self.reset(id),
            (Target::Knob(id), Btn::Middle) => self.reset(id),
            (Target::Knob(id), Btn::Left) => {
                self.vals.begin(id);
                self.drag = Some(Drag::Knob {
                    id: id.clone(),
                    v: self.vals.norm(id),
                });
                self.view.pressed = Some(t.clone());
                acts.push(Act::Lock);
            }
            (Target::Knob(id), Btn::Right) => {
                acts.push(Act::Menu(MenuReq::Param(id.clone()), x, y))
            }
            (Target::Toggle(id), Btn::Left) => {
                let on = self.vals.plain(id) > 0.5;
                self.vals.edit_plain(id, if on { 0.0 } else { 1.0 });
            }
            (Target::Set(id, v), Btn::Left) => {
                self.vals.edit_plain(id, *v);
            }
            (Target::ZoomH(f), Btn::Left) => self.zoom_h(*f, None),
            (Target::ZoomV(f), Btn::Left) => self.zoom_v(*f, None),
            (Target::ZoomFull, Btn::Left) => self.zoom_full(),
            (Target::LoadSample, Btn::Left) => acts.push(Act::OpenDialog),
            (Target::Export, Btn::Left) => acts.push(Act::Export),
            (Target::Grid, Btn::Middle) => {
                let (start0, _) = self.view.window(self.total());
                self.drag = Some(Drag::Pan {
                    x0: x,
                    y0: y,
                    start0,
                    hi0: self.view.note_hi,
                });
                self.view.pressed = Some(t.clone());
            }
            (Target::Grid, Btn::Left) if m.ctrl && m.alt => {
                let (start0, _) = self.view.window(self.total());
                self.drag = Some(Drag::Pan {
                    x0: x,
                    y0: y,
                    start0,
                    hi0: self.view.note_hi,
                });
                self.view.pressed = Some(t.clone());
            }
            (Target::Block(seg), Btn::Left) => {
                let start = self
                    .view
                    .data
                    .analysis
                    .as_ref()
                    .map_or(0.0, |a| a.segments.get(*seg).map_or(0.0, |s| s.target));
                self.drag = Some(Drag::Block {
                    seg: *seg,
                    y0: y,
                    start,
                });
                self.view.pressed = Some(t.clone());
            }
            _ => {}
        }
        self.hint = self.hint_for(Some(&t));
        acts
    }

    /// Mouse movement during a locked drag (logical px since the last event).
    pub fn delta(&mut self, _dx: f32, dy: f32, m: Mods) -> Vec<Act> {
        let Some(Drag::Knob { id, v }) = self.drag.clone() else {
            return vec![];
        };
        let f = if m.ctrl { FINE } else { 1.0 };
        let v = (v - dy * f / KNOB_PX).clamp(0.0, 1.0);
        self.vals.set(&id, v);
        self.drag = Some(Drag::Knob { id, v });
        self.hint = self.hint_for(self.view.pressed.as_ref());
        vec![Act::Repaint]
    }

    pub fn up(&mut self, _btn: Btn) -> Vec<Act> {
        self.drag.take();
        self.view.pressed = None;
        vec![Act::Repaint]
    }

    pub fn moved(&mut self, x: f32, y: f32, _m: Mods) -> Vec<Act> {
        self.view.mouse = Some((x, y));
        match self.drag.clone() {
            Some(Drag::Block { seg, y0, start }) => {
                let mut note = (start + (y0 - y) / view::row_h(self.view.visible))
                    .round()
                    .clamp(12.0, 120.0);
                // Snap-to-scale: the block only lands on notes of the chosen scale.
                if self.vals.plain("snap") > 0.5 {
                    let scale = self.vals.plain("scale").round() as i32;
                    let root = self.vals.plain("root").round() as i32;
                    note = crate::dsp::snap_note(note, scale, root);
                }
                let cur = self
                    .view
                    .data
                    .analysis
                    .as_ref()
                    .and_then(|a| a.segments.get(seg))
                    .map(|s| s.target);
                if cur != Some(note) {
                    self.shared.set_target(seg, note);
                    self.sync_data();
                }
                self.hint = self.hint_for(self.view.pressed.as_ref());
                vec![Act::Repaint]
            }
            Some(Drag::Pan {
                x0,
                y0,
                start0,
                hi0,
            }) => {
                let total = self.total();
                if total > 0 {
                    let g = view::grid_rect();
                    let (_, win) = self.view.window(total);
                    let start = start0 - (x - x0) / g.w * win;
                    let range = (total as f32 - win).max(1.0);
                    self.view.scroll = (start / range).clamp(0.0, 1.0);
                }
                self.view.note_hi =
                    (hi0 + (y - y0) / view::row_h(self.view.visible)).clamp(20.0, 127.0);
                vec![Act::Repaint]
            }
            Some(Drag::Knob { .. }) => vec![Act::Repaint],
            None => {
                let t = self.hit(x, y);
                self.hint = self.hint_for(t.as_ref());
                vec![Act::Repaint]
            }
        }
    }

    pub fn leave(&mut self) -> Vec<Act> {
        self.view.mouse = None;
        self.hint.clear();
        vec![Act::Repaint]
    }

    /// Wheel, following Newtone's editor: plain = scroll vertically, Shift = scroll horizontally,
    /// Ctrl = zoom horizontally (time), Alt = zoom vertically (pitch). Knobs step by 1/32 of range.
    pub fn wheel(&mut self, x: f32, y: f32, notches: f32, m: Mods) -> Vec<Act> {
        let Some(t) = self.hit(x, y) else {
            return vec![];
        };
        match &t {
            Target::Knob(id) => {
                let v = (self.vals.norm(id) + notches.signum() / 32.0).clamp(0.0, 1.0);
                self.vals.edit(id, v);
            }
            Target::Grid | Target::Block(_) => {
                let s = notches.signum();
                if m.ctrl {
                    self.zoom_h(if s > 0.0 { 1.3 } else { 1.0 / 1.3 }, Some(x));
                } else if m.alt {
                    self.zoom_v(if s > 0.0 { 1.3 } else { 1.0 / 1.3 }, Some(y));
                } else if m.shift {
                    self.view.scroll = (self.view.scroll - s * 0.12).clamp(0.0, 1.0);
                } else {
                    self.view.note_hi = (self.view.note_hi + s * 3.0).clamp(20.0, 127.0);
                }
            }
            _ => return vec![],
        }
        self.hint = self.hint_for(Some(&t));
        vec![Act::Repaint]
    }

    pub fn menu(&self, req: &MenuReq) -> MenuDef<Cmd> {
        match req {
            MenuReq::Param(id) => {
                let mut m = MenuDef::new(MenuStyle::Context);
                m.head(&view::pretty(id), Cmd::None);
                m.add(MenuItem::item("Reset"), Cmd::Reset(id.clone()));
                m.sep(Cmd::None);
                m.add(MenuItem::item("Copy value"), Cmd::CopyValue(id.clone()));
                m.add(
                    MenuItem::item("Paste value").enabled(self.copied.is_some()),
                    Cmd::PasteValue(id.clone()),
                );
                m.add(MenuItem::item("Enter value..."), Cmd::TypeIn(id.clone()));
                m.sep(Cmd::None);
                m.add(
                    MenuItem::item("Create automation clip")
                        .enabled(self.hosted || cfg!(feature = "embed")),
                    Cmd::Automate(id.clone()),
                );
                m
            }
        }
    }

    pub fn pick(&mut self, _req: &MenuReq, cmd: Cmd, x: f32, y: f32) -> Vec<Act> {
        match cmd {
            Cmd::None => {}
            Cmd::Reset(id) => self.reset(&id),
            Cmd::CopyValue(id) => self.copied = Some(self.vals.norm(&id)),
            Cmd::PasteValue(id) => {
                if let Some(v) = self.copied {
                    self.vals.edit(&id, v);
                }
            }
            Cmd::TypeIn(id) => return vec![Act::TypeIn(id, x, y)],
            Cmd::Automate(id) => {
                return self
                    .param_index(&id)
                    .map(Act::Automate)
                    .into_iter()
                    .collect();
            }
        }
        vec![Act::Repaint]
    }

    /// Position of `id` in the parameter list (the host's automation index).
    pub fn param_index(&self, id: &str) -> Option<usize> {
        self.params
            .param_map()
            .iter()
            .position(|(pid, _, _)| pid == id)
    }

    /// The value text a type-in box starts with.
    pub fn value_text(&self, id: &str) -> String {
        self.vals.text(id)
    }

    /// Applies a typed value.
    pub fn typed(&mut self, id: &str, text: &str) {
        if text.trim() == self.value_text(id).trim() {
            return;
        }
        if let Some(n) = self.vals.parse(id, text) {
            if n != self.vals.norm(id) {
                self.vals.edit(id, n);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The window (Linux / macOS / embed): the os-plugkit GUI thread painting into the host's window.

#[cfg(any(target_os = "linux", target_os = "macos", feature = "embed"))]
mod gui {
    use std::sync::Arc;

    use nih_plug::prelude::GuiContext;
    use os_hostlink::{Cmd, PageIcon, Peer};
    use os_plugkit::gui::{self as px, Button, Input, Window};
    use os_plugkit::live::LiveEdits;
    use os_theme::Palette;
    use os_ui::Canvas;

    use super::{Act, Btn, Logic, Mods};
    use crate::dsp::Shared;
    use crate::params::PitchParams;
    use crate::view::{self, ViewState};

    /// The editor on its GUI thread.
    struct Ed {
        win: Window,
        canvas: Canvas,
        buf: Vec<u32>,
        crop: Vec<u32>,
        scale: f32,
        size: (u32, u32),
        params: Arc<PitchParams>,
        logic: Logic,
        palette: Palette,
        last_sig: u64,
        last_view: ViewState,
        last_hint: String,
        dirty: bool,
        link: Arc<Peer>,
        last_page: usize,
    }

    pub fn open(
        parent: px::Parent,
        scale: f32,
        params: Arc<PitchParams>,
        shared: Arc<Shared>,
        live: Arc<LiveEdits>,
        ctx: Arc<dyn GuiContext>,
        slot: &px::Slot,
    ) -> Option<px::EditorThread> {
        let scale = if scale > 0.0 { scale } else { 1.0 };
        let px = move |v: u32| (v as f32 * scale).round() as u32;
        let wake = slot.clone();
        let pages = vec![PageIcon::Plugin];
        let link = Peer::register(
            parent as u64,
            pages,
            (view::EDITOR_W, view::HOSTED_EDITOR_H),
            move || px::notify(&wake),
        );
        let peer = link.clone();
        let thread = px::spawn(
            parent,
            (px(view::EDITOR_W), px(view::EDITOR_H)),
            slot,
            move |win| {
                let full = (px(view::W), px(view::H));
                let Ok(canvas) = Canvas::new(full.0, full.1, scale) else {
                    return;
                };
                let logic = Logic::new(params.clone(), shared, live, Some(ctx));
                let ed = Ed {
                    win,
                    canvas,
                    buf: Vec::new(),
                    crop: Vec::new(),
                    scale,
                    size: full,
                    params,
                    logic,
                    palette: os_plugkit::palette(),
                    last_sig: 0,
                    last_view: ViewState::default(),
                    last_hint: String::new(),
                    dirty: true,
                    link: peer,
                    last_page: 0,
                };
                ed.run();
            },
        );
        if thread.is_none() {
            link.unregister();
        }
        thread
    }

    impl Drop for Ed {
        fn drop(&mut self) {
            self.link.unregister();
        }
    }

    fn btn(b: Button) -> Btn {
        match b {
            Button::Left => Btn::Left,
            Button::Right => Btn::Right,
            Button::Middle => Btn::Middle,
        }
    }

    impl Ed {
        fn run(mut self) {
            while let Some(input) = self.win.next() {
                match input {
                    Input::Expose => self.dirty = true,
                    Input::Tick => self.poll(),
                    Input::Down { x, y, btn: b, dbl } => {
                        let (x, y) = self.to_view(x, y);
                        let acts =
                            self.logic
                                .down(x, y, btn(b), self.mods(), dbl && b == Button::Left);
                        self.act(acts);
                    }
                    Input::Move { x, y } => {
                        let (x, y) = self.to_view(x, y);
                        let acts = self.logic.moved(x, y, self.mods());
                        self.act(acts);
                    }
                    Input::Delta { dx, dy } => {
                        let acts = self
                            .logic
                            .delta(dx / self.scale, dy / self.scale, self.mods());
                        self.act(acts);
                    }
                    Input::Up { .. } => {
                        self.win.unlock();
                        let acts = self.logic.up(Btn::Left);
                        self.act(acts);
                    }
                    Input::Leave => {
                        if !self.win.locked() && !self.logic.dragging() {
                            let acts = self.logic.leave();
                            self.act(acts);
                        }
                    }
                    Input::Wheel { x, y, notches } => {
                        let (x, y) = self.to_view(x, y);
                        let acts = self.logic.wheel(x, y, notches, self.mods());
                        self.act(acts);
                    }
                    Input::Key { .. } | Input::Drop { .. } => {}
                }
                if self.dirty && self.win.idle() {
                    self.paint();
                }
            }
            os_plugkit::park!(self);
        }

        fn mods(&self) -> Mods {
            let (ctrl, shift, alt) = self.win.mods();
            Mods { ctrl, shift, alt }
        }

        fn crop_y(&self) -> u32 {
            if self.logic.hosted {
                view::HOSTED_CROP_Y
            } else {
                view::CROP_Y
            }
        }

        fn content(&self) -> (u32, u32) {
            (
                view::EDITOR_W,
                if self.logic.hosted {
                    view::HOSTED_EDITOR_H
                } else {
                    view::EDITOR_H
                },
            )
        }

        fn to_view(&self, x: f32, y: f32) -> (f32, f32) {
            (
                x / self.scale + view::CROP_X as f32,
                y / self.scale + self.crop_y() as f32,
            )
        }

        fn screen(&self, x: f32, y: f32) -> (i32, i32) {
            let cx = ((x - view::CROP_X as f32) * self.scale) as i32;
            let cy = ((y - self.crop_y() as f32) * self.scale) as i32;
            self.win.to_root(cx, cy)
        }

        fn poll(&mut self) {
            let dirty = self.logic.vals.expire();
            self.logic.sync_data();
            let sig = self.logic.vals.signature();
            if dirty
                || sig != self.last_sig
                || self.logic.view != self.last_view
                || self.logic.hint != self.last_hint
            {
                self.dirty = true;
            }
            if self.logic.hint != self.last_hint {
                self.last_hint.clone_from(&self.logic.hint);
                self.link.set_hint(&self.logic.hint);
                self.link.to_host(Cmd::Changed, 0);
            }
            self.sync_link();
        }

        fn sync_link(&mut self) {
            if self.link.hosted() && !self.logic.hosted {
                self.logic.hosted = true;
                let (cw, ch) = self.content();
                let px = |v: u32| (v as f32 * self.scale).round() as u32;
                self.win.resize(px(cw), px(ch));
                self.dirty = true;
            }
            let page = self.link.page();
            if page != self.last_page {
                self.last_page = page;
                self.dirty = true;
            }
        }

        fn act(&mut self, acts: Vec<Act>) {
            for a in acts {
                match a {
                    Act::Repaint => self.dirty = true,
                    Act::Lock => self.win.lock(),
                    Act::Menu(req, x, y) => {
                        let def = self.logic.menu(&req);
                        let (sx, sy) = self.screen(x, y);
                        if let Some(cmd) = self.win.popup(sx, sy, &def, &self.palette, self.scale) {
                            let acts = self.logic.pick(&req, cmd, x, y);
                            self.act(acts);
                        }
                        self.dirty = true;
                    }
                    Act::TypeIn(id, x, y) => {
                        let (sx, sy) = self.screen(x, y);
                        let cur = self.logic.value_text(&id);
                        if let Some(t) = self.win.type_in(sx, sy, &cur, self.scale) {
                            self.logic.typed(&id, &t);
                        }
                        self.dirty = true;
                    }
                    Act::Automate(i) => {
                        self.link.to_host(Cmd::Automate, i as isize);
                    }
                    Act::OpenDialog => {
                        if let Some(path) = file_dialog(&self.win) {
                            self.logic.load(&path);
                        }
                        self.dirty = true;
                    }
                    Act::Export => {
                        self.logic.export();
                        self.dirty = true;
                    }
                }
            }
        }

        fn paint(&mut self) {
            self.dirty = false;
            let hint = if self.logic.hosted {
                String::new()
            } else {
                self.logic.hint.clone()
            };
            self.logic.hits = view::paint(
                &mut self.canvas,
                &self.palette,
                &self.logic.vals,
                &self.logic.view,
                &hint,
                self.logic.hosted,
            );
            self.last_sig = self.logic.vals.signature();
            self.last_view = self.logic.view.clone();
            let (ox, oy) = (
                (view::CROP_X as f32 * self.scale).round() as u32,
                (self.crop_y() as f32 * self.scale).round() as u32,
            );
            let (cw, ch) = self.content();
            let w = ((cw as f32 * self.scale).round() as u32).min(self.size.0 - ox);
            let h = ((ch as f32 * self.scale).round() as u32).min(self.size.1 - oy);
            let full = self.size.0 as usize;
            self.buf.resize(full * self.size.1 as usize, 0);
            self.canvas.copy_to_xrgb(&mut self.buf);
            self.crop.clear();
            for row in oy as usize..(oy + h) as usize {
                let at = row * full + ox as usize;
                self.crop.extend_from_slice(&self.buf[at..at + w as usize]);
            }
            self.win.blit(&self.crop, w, h);
        }
    }

    /// The open sample dialog: the desktop's own, as Lemon Sampler's.
    fn file_dialog(win: &Window) -> Option<String> {
        if os_plugkit::no_modal() {
            return None;
        }
        let path = win.dialog(move || {
            rfd::FileDialog::new()
                .set_title("Sample to correct")
                .add_filter(
                    "Audio files",
                    &["wav", "flac", "ogg", "wv", "aif", "aiff", "mp3", "m4a"],
                )
                .pick_file()
        });
        path.flatten().map(|p| p.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view;

    fn logic() -> Logic {
        Logic::new(
            Arc::new(PitchParams::default()),
            Arc::new(Shared::new()),
            Arc::new(LiveEdits::default()),
            None,
        )
    }

    fn paint(l: &mut Logic) {
        let mut canvas = os_ui::Canvas::new(view::W, view::H, 1.0).unwrap();
        let palette = os_theme::Palette::default();
        l.hits = view::paint(&mut canvas, &palette, &l.vals, &l.view, "", false);
    }

    #[test]
    fn the_load_button_asks_for_a_dialog() {
        let mut l = logic();
        paint(&mut l);
        let acts = l.down(60.0, 88.0, Btn::Left, Mods::default(), false);
        assert!(acts.contains(&Act::OpenDialog), "got {acts:?}");
    }

    #[test]
    fn zoom_controls_change_the_grid_window() {
        let mut l = logic();
        paint(&mut l);
        // V+ button: fewer notes on screen (vertical zoom in).
        l.down(479.0, 88.0, Btn::Left, Mods::default(), false);
        assert!(l.view.visible < 30.0, "visible {}", l.view.visible);
        // Alt+wheel over the grid zooms pitch the other way.
        let g = view::grid_rect();
        l.wheel(
            g.cx(),
            g.cy(),
            -1.0,
            Mods {
                ctrl: false,
                shift: false,
                alt: true,
            },
        );
        assert!(l.view.visible > 15.0, "visible {}", l.view.visible);
        // FULL resets both zooms.
        l.down(526.0, 88.0, Btn::Left, Mods::default(), false);
        assert_eq!(l.view.visible, 30.0);
        assert_eq!(l.view.zoom_x, 1.0);
        assert_eq!(l.view.scroll, 0.0);
    }
}
