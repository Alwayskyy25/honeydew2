// Copyright (C) 2026 Lemon Studio contributors
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Lemon Additive — original implementation (see params.rs note).

//! The plugin editor: knob drags with the cursor held in place, spectrum band drags, wheel and reset gestures,
//! part tabs and the parameter menus (reset / copy / paste / type a value / create automation clip), following
//! the other Lemon instruments' editor pattern. Edits show and sound at once (`os_plugkit::vals`).

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

use nih_plug::prelude::*;
use os_plugkit::MenuDef;
use os_plugkit::live::LiveEdits;
use os_plugkit::vals::Vals;
use os_ui::kit::menu::{MenuItem, MenuStyle};

use crate::params::AdditiveParams;
use crate::view::{self, Hit, Target, ViewState};

pub struct AdditiveEditor {
    pub params: Arc<AdditiveParams>,
    /// DPI scale as f32 bits.
    pub scale: AtomicU32,
    #[cfg_attr(not(windows), allow(dead_code))]
    hwnd: Arc<AtomicIsize>,
    #[cfg(any(target_os = "linux", target_os = "macos", feature = "embed"))]
    slot: os_plugkit::gui::Slot,
    live: Arc<LiveEdits>,
}

impl AdditiveEditor {
    pub fn new(params: Arc<AdditiveParams>) -> Self {
        AdditiveEditor {
            params,
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

impl Editor for AdditiveEditor {
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
                    self.live.clone(),
                    context,
                    &self.slot,
                ))
            }
            // Elsewhere no editor opens yet (Honeydew hosts the editor itself through `os-plugkit`'s embed).
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

// ---------------------------------------------------------------------------------------------------------------
// Laws

/// Pixels of drag for a knob's whole range.
pub const KNOB_PX: f32 = 200.0;
const FINE: f32 = 0.25;

// ---------------------------------------------------------------------------------------------------------------
// Menus

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

// ---------------------------------------------------------------------------------------------------------------
// Editor logic

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
    /// Knob: `v` = the unrounded normalised value the drag has reached.
    Knob { id: String, v: f32 },
    /// Painting the shown part's harmonic spectrum: the last partial painted.
    Spec { last: usize },
}

pub struct Logic {
    pub vals: Vals,
    pub params: Arc<AdditiveParams>,
    pub view: ViewState,
    pub hits: Vec<Hit>,
    drag: Option<Drag>,
    pub hint: String,
    copied: Option<f32>,
    pub hosted: bool,
}

impl Logic {
    pub fn new(
        params: Arc<AdditiveParams>,
        live: Arc<LiveEdits>,
        ctx: Option<Arc<dyn GuiContext>>,
    ) -> Self {
        let vals = Vals::new(&*params, live, ctx);
        Logic {
            vals,
            params,
            view: ViewState::default(),
            hits: Vec::new(),
            drag: None,
            hint: String::new(),
            copied: None,
            hosted: false,
        }
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
            Some(t) => view::hint(t, &self.vals, self.view.part),
            None => String::new(),
        }
    }

    fn reset(&mut self, id: &str) {
        let d = self.vals.default_norm(id);
        self.vals.edit(id, d);
    }

    /// Paints the spectrum level at (x, y) into the shown part's spectrum, filling the columns
    /// between the last painted partial and this one so fast strokes stay continuous.
    fn paint_spec(&mut self, x: f32, y: f32) {
        let k = view::spec_at_x(x);
        let v = view::spec_value_at_y(y);
        let part = self.view.part as usize;
        let last = match &self.drag {
            Some(Drag::Spec { last }) => Some(*last),
            _ => None,
        };
        match last {
            Some(l0) => {
                let (a, b) = if l0 <= k { (l0, k) } else { (k, l0) };
                for i in a..=b {
                    self.params.set_spectrum(part, i, v);
                }
            }
            None => self.params.set_spectrum(part, k, v),
        }
        self.drag = Some(Drag::Spec { last: k });
        self.hint = format!("Partial {}: {:.0} %", k + 1, v * 100.0);
    }

    /// Applies a spectrum preset to the shown part.
    fn spec_preset(&mut self, p: u8) {
        let vals: Vec<f32> = (0..crate::params::PARTIALS)
            .map(|k| match p {
                0 => 1.0,
                1 => 1.0 / (k as f32 + 1.0).sqrt(),
                2 => {
                    if k % 2 == 0 {
                        1.0
                    } else {
                        0.0
                    }
                }
                _ => {
                    if k == 0 {
                        1.0
                    } else {
                        0.0
                    }
                }
            })
            .collect();
        self.params.set_spectrum_all(self.view.part as usize, &vals);
    }

    /// Copies the shown part's parameter values onto the other part (Harmor's Copy part).
    fn copy_part(&mut self) {
        let (from, to) = if self.view.part == 0 {
            ("aa_", "ab_")
        } else {
            ("ab_", "aa_")
        };
        let pairs: Vec<(String, String)> = self
            .params
            .param_map()
            .iter()
            .filter_map(|(pid, _, _)| {
                pid.strip_prefix(from)
                    .map(|rest| (pid.clone(), format!("{to}{rest}")))
            })
            .collect();
        for (src, dst) in pairs {
            let v = self.vals.norm(&src);
            self.vals.edit(&dst, v);
        }
        // The spectrum is patch data, not a parameter: copy it across too.
        let spec = self.params.spectrum(self.view.part as usize);
        self.params
            .set_spectrum_all(1 - self.view.part as usize, &spec);
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
            (Target::Knob(id) | Target::Toggle(id) | Target::Set(id, _), Btn::Right) => {
                acts.push(Act::Menu(MenuReq::Param(id.clone()), x, y))
            }
            (Target::Toggle(id), Btn::Left) => {
                let on = self.vals.plain(id) > 0.5;
                self.vals.edit_plain(id, if on { 0.0 } else { 1.0 });
            }
            (Target::Set(id, v), Btn::Left) => {
                self.vals.edit_plain(id, *v);
            }
            (Target::PartTab(p), Btn::Left) => {
                self.view.part = *p;
            }
            (Target::Spec(_), Btn::Left) => {
                self.view.pressed = Some(t.clone());
                self.paint_spec(x, y);
            }
            (Target::SpecPreset(p), Btn::Left) => {
                self.spec_preset(*p);
            }
            (Target::CopyPart, Btn::Left) => {
                self.copy_part();
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
        let Some(d) = self.drag.take() else {
            return vec![];
        };
        if let Drag::Knob { id, .. } = d {
            self.vals.end(&id);
        }
        self.view.pressed = None;
        vec![Act::Repaint]
    }

    pub fn moved(&mut self, x: f32, y: f32, _m: Mods) -> Vec<Act> {
        self.view.mouse = Some((x, y));
        match self.drag.clone() {
            Some(Drag::Spec { .. }) => {
                self.paint_spec(x, y);
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

    /// Wheel: knobs step by 1/32 of their range, bands by 1/20, steppers by one step.
    pub fn wheel(&mut self, x: f32, y: f32, notches: f32, _m: Mods) -> Vec<Act> {
        let Some(t) = self.hit(x, y) else {
            return vec![];
        };
        match &t {
            Target::Knob(id) => {
                let v = (self.vals.norm(id) + notches.signum() / 32.0).clamp(0.0, 1.0);
                self.vals.edit(id, v);
            }
            Target::Spec(k) => {
                let part = self.view.part as usize;
                let cur = self.params.spectrum(part)[*k];
                let v = (cur + notches.signum() / 20.0).clamp(0.0, 1.0);
                self.params.set_spectrum(part, *k, v);
            }
            Target::Set(id, _) if id.ends_with("octave") || id == "unison" => {
                let v = self.vals.plain(id) + notches.signum();
                self.vals.edit_plain(id, v);
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
    use crate::params::AdditiveParams;
    use crate::view::{self, ViewState};

    /// The editor on its GUI thread.
    struct Ed {
        win: Window,
        canvas: Canvas,
        buf: Vec<u32>,
        crop: Vec<u32>,
        scale: f32,
        size: (u32, u32),
        params: Arc<AdditiveParams>,
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
        params: Arc<AdditiveParams>,
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
                let logic = Logic::new(params.clone(), live, Some(ctx));
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
            let spec = self.logic.params.spectrum(self.logic.view.part as usize);
            self.logic.hits = view::paint(
                &mut self.canvas,
                &self.palette,
                &self.logic.vals,
                &spec,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view;

    fn logic() -> Logic {
        Logic::new(
            Arc::new(AdditiveParams::default()),
            Arc::new(LiveEdits::default()),
            None,
        )
    }

    fn paint(l: &mut Logic) {
        let mut canvas = os_ui::Canvas::new(view::W, view::H, 1.0).unwrap();
        let palette = os_theme::Palette::default();
        let spec = l.params.spectrum(l.view.part as usize);
        l.hits = view::paint(&mut canvas, &palette, &l.vals, &spec, &l.view, "", false);
    }

    #[test]
    fn a_spectrum_drag_draws_partials() {
        let mut l = logic();
        paint(&mut l);
        let area = view::spectrum_rect();
        // Press near the top of partial 3's column: near-full level; drag to the bottom: near zero.
        let x = view::spec_x(2);
        l.down(x, area.y + 4.0, Btn::Left, Mods::default(), false);
        let top = l.params.spectrum(0)[2];
        assert!(top > 0.8, "partial at top should be near 1, got {top}");
        l.moved(x, area.y + area.h - 4.0, Mods::default());
        let bot = l.params.spectrum(0)[2];
        assert!(bot < 0.2, "partial at bottom should be near 0, got {bot}");
        l.up(Btn::Left);
    }

    #[test]
    fn part_tab_switches_the_shown_part() {
        let mut l = logic();
        paint(&mut l);
        l.down(170.0, 66.0, Btn::Left, Mods::default(), false);
        assert_eq!(l.view.part, 1);
    }

    #[test]
    fn copy_part_mirrors_the_shown_part() {
        let mut l = logic();
        paint(&mut l);
        // Change a part A knob away from its default, then copy A onto B.
        l.vals.edit("aa_prism", 0.7);
        l.down(320.0, 66.0, Btn::Left, Mods::default(), false); // COPY A>B
        assert!(
            (l.vals.norm("ab_prism") - 0.7).abs() < 0.01,
            "got {}",
            l.vals.norm("ab_prism")
        );
    }
}
