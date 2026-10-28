// FastView - chrome: layout, toolbar, overlays, tooltips and messages.
//
// Everything is drawn by hand, which is why the app can open this fast: there is
// no widget toolkit to start up, no theme engine and no extra window classes.

use crate::gfx::{self, measure, rgb, text, Canvas, Fonts};
use windows::Win32::Graphics::Gdi::{DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_RIGHT};

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.right() && py < self.bottom()
    }
    pub fn inset(&self, n: i32) -> Self {
        Self::new(self.x + n, self.y + n, self.w - n * 2, self.h - n * 2)
    }
}

pub const BG: u32 = 0x0a0c10;
pub const PANEL: u32 = 0x151922;
pub const PANEL2: u32 = 0x202634;
pub const LINE: u32 = 0x2b3342;
pub const TEXT: u32 = 0xe9edf5;
pub const MUTED: u32 = 0x9aa4b4;
pub const FAINT: u32 = 0x667081;
pub const ACCENT: u32 = 0x5aa9ff;
pub const DANGER: u32 = 0xff6b6b;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Btn {
    Open,
    Prev,
    Next,
    ZoomOut,
    Fit,
    Actual,
    ZoomIn,
    RotateL,
    RotateR,
    Crop,
    Delete,
    Fullscreen,
}

impl Btn {
    pub fn icon(self) -> (&'static str, bool) {
        match self {
            Btn::Open => ("\u{e8e5}", true),
            Btn::Prev => ("\u{e76b}", true),
            Btn::Next => ("\u{e76c}", true),
            Btn::ZoomOut => ("\u{e71f}", true),
            Btn::Fit => ("Fit", false),
            Btn::Actual => ("1:1", false),
            Btn::ZoomIn => ("\u{e710}", true),
            Btn::RotateL => ("\u{21ba}", false),
            Btn::RotateR => ("\u{21bb}", false),
            Btn::Crop => ("\u{e7a8}", true),
            Btn::Delete => ("\u{e74d}", true),
            Btn::Fullscreen => ("\u{e740}", true),
        }
    }

    pub fn tip(self) -> &'static str {
        match self {
            Btn::Open => "Open  ·  Ctrl+O",
            Btn::Prev => "Previous  ·  ←",
            Btn::Next => "Next  ·  →",
            Btn::ZoomOut => "Zoom out  ·  −",
            Btn::Fit => "Fit to window  ·  0",
            Btn::Actual => "Actual size  ·  1",
            Btn::ZoomIn => "Zoom in  ·  +",
            Btn::RotateL => "Rotate left  ·  Shift+R",
            Btn::RotateR => "Rotate right  ·  R",
            Btn::Crop => "Crop  ·  C",
            Btn::Delete => "Delete to Recycle Bin  ·  Del",
            Btn::Fullscreen => "Fullscreen  ·  F11",
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum CropBtn {
    Free,
    Square,
    FourThree,
    SixteenNine,
    Apply,
    Cancel,
}

impl CropBtn {
    pub fn label(self) -> &'static str {
        match self {
            CropBtn::Free => "Free",
            CropBtn::Square => "1:1",
            CropBtn::FourThree => "4:3",
            CropBtn::SixteenNine => "16:9",
            CropBtn::Apply => "Apply",
            CropBtn::Cancel => "Cancel",
        }
    }
}

pub struct Layout {
    pub w: i32,
    pub h: i32,
    pub s: f32,
    pub content: Rect,
    pub toolbar: Rect,
    pub buttons: Vec<(Btn, Rect)>,
    pub cropbar: Rect,
    pub crop_buttons: Vec<(CropBtn, Rect)>,
    pub empty_open: Rect,
}

impl Layout {
    pub fn new(w: i32, h: i32, dpi: u32) -> Self {
        let s = dpi as f32 / 96.0;
        let px = |v: f32| (v * s).round() as i32;
        let top = px(56.0);
        let bottom = px(96.0);
        let pad = px(14.0);
        let content = Rect::new(
            pad,
            top + px(6.0),
            (w - pad * 2).max(40),
            (h - top - bottom - px(12.0)).max(40),
        );

        let bw = px(36.0);
        let gap = px(4.0);
        let group_gap = px(10.0);
        let pad_x = px(10.0);
        let groups: [&[Btn]; 7] = [
            &[Btn::Open],
            &[Btn::Prev, Btn::Next],
            &[Btn::ZoomOut, Btn::Fit, Btn::Actual, Btn::ZoomIn],
            &[Btn::RotateL, Btn::RotateR],
            &[Btn::Crop],
            &[Btn::Delete],
            &[Btn::Fullscreen],
        ];
        let mut total = pad_x * 2;
        for (gi, g) in groups.iter().enumerate() {
            total += g.len() as i32 * bw;
            if g.len() > 1 {
                total += (g.len() as i32 - 1) * gap;
            }
            if gi + 1 < groups.len() {
                total += group_gap;
            }
        }
        let bar_h = px(52.0);
        let bar_x = (w - total) / 2;
        let bar_y = h - px(18.0) - bar_h;
        let toolbar = Rect::new(bar_x, bar_y, total, bar_h);
        let mut buttons = Vec::with_capacity(12);
        let mut x = bar_x + pad_x;
        for (gi, g) in groups.iter().enumerate() {
            for (i, b) in g.iter().enumerate() {
                buttons.push((*b, Rect::new(x, bar_y + (bar_h - bw) / 2, bw, bw)));
                x += bw;
                if i + 1 < g.len() {
                    x += gap;
                }
            }
            if gi + 1 < groups.len() {
                x += group_gap;
            }
        }

        // Crop bar: Free, 1:1, 4:3, 16:9, Apply, Cancel.
        let labels: [(CropBtn, f32); 6] = [
            (CropBtn::Free, 62.0),
            (CropBtn::Square, 54.0),
            (CropBtn::FourThree, 54.0),
            (CropBtn::SixteenNine, 60.0),
            (CropBtn::Apply, 84.0),
            (CropBtn::Cancel, 78.0),
        ];
        let cgap = px(6.0);
        let cpad = px(10.0);
        let mut cw = cpad * 2 + cgap * 5;
        for (_, wl) in &labels {
            cw += px(*wl);
        }
        let cropbar = Rect::new((w - cw) / 2, bar_y, cw, bar_h);
        let mut crop_buttons = Vec::with_capacity(6);
        let mut cx = cropbar.x + cpad;
        for (b, wl) in labels {
            crop_buttons.push((b, Rect::new(cx, bar_y + (bar_h - px(36.0)) / 2, px(wl), px(36.0))));
            cx += px(wl) + cgap;
        }

        let empty_open = Rect::new(w / 2 - px(84.0), h / 2 + px(74.0), px(168.0), px(44.0));
        Self {
            w,
            h,
            s,
            content,
            toolbar,
            buttons,
            cropbar,
            crop_buttons,
            empty_open,
        }
    }

    pub fn px(&self, v: f32) -> i32 {
        (v * self.s).round() as i32
    }

    pub fn button_at(&self, x: i32, y: i32) -> Option<Btn> {
        self.buttons
            .iter()
            .find(|(_, r)| r.contains(x, y))
            .map(|(b, _)| *b)
    }

    pub fn crop_button_at(&self, x: i32, y: i32) -> Option<CropBtn> {
        self.crop_buttons
            .iter()
            .find(|(_, r)| r.contains(x, y))
            .map(|(b, _)| *b)
    }
}

pub struct Chrome<'a> {
    pub c: &'a mut Canvas,
    pub f: &'a Fonts,
    pub l: &'a Layout,
}

impl<'a> Chrome<'a> {
    pub fn background(&mut self) {
        self.c.clear(BG);
    }

    pub fn top_bar(&mut self, name: &str, meta: &str, zoom: &str, hint: Option<&str>) {
        let s = self.l.s;
        let scrim_h = self.l.px(132.0);
        self.c
            .vgrad(Rect::new(0, 0, self.l.w, scrim_h), 0.80, 0.0, 0x04060a);

        let left = self.l.px(22.0);
        if let Some(hint) = hint {
            text(
                self.c.hdc(),
                "Crop",
                Rect::new(left, self.l.px(9.0), self.l.px(160.0), self.l.px(24.0)),
                self.f.ui_15,
                TEXT,
                DT_LEFT.0,
            );
            text(
                self.c.hdc(),
                hint,
                Rect::new(left, self.l.px(31.0), self.l.w / 2, self.l.px(20.0)),
                self.f.ui_11,
                MUTED,
                DT_LEFT.0,
            );
        } else {
            text(
                self.c.hdc(),
                name,
                Rect::new(left, self.l.px(7.0), self.l.w - self.l.px(220.0), self.l.px(26.0)),
                self.f.ui_15,
                TEXT,
                DT_LEFT.0 | DT_END_ELLIPSIS.0,
            );
            text(
                self.c.hdc(),
                meta,
                Rect::new(left, self.l.px(32.0), self.l.w - self.l.px(220.0), self.l.px(18.0)),
                self.f.ui_11,
                MUTED,
                DT_LEFT.0 | DT_END_ELLIPSIS.0,
            );
        }

        // Zoom chip, top right.
        let label = zoom;
        if !label.is_empty() {
            let (tw, _) = measure(self.c.hdc(), label, self.f.ui_12);
            let chip_w = tw + self.l.px(22.0);
            let chip = Rect::new(
                self.l.w - self.l.px(20.0) - chip_w,
                self.l.px(13.0),
                chip_w,
                self.l.px(28.0),
            );
            self.c.rrect(chip, chip.h / 2, PANEL, 0.82);
            self.c.rrect_stroke(chip, chip.h / 2, 1.0, LINE, 0.9);
            text(
                self.c.hdc(),
                label,
                chip,
                self.f.ui_12,
                TEXT,
                DT_CENTER.0,
            );
        }
        let _ = s;
    }

    pub fn toolbar(
        &mut self,
        enabled: bool,
        hover: Option<Btn>,
        active_fit: bool,
        active_actual: bool,
        fullscreen: bool,
    ) {
        if !enabled {
            return;
        }
        self.c.vgrad(
            Rect::new(0, self.l.h - self.l.px(170.0), self.l.w, self.l.px(170.0)),
            0.0,
            0.72,
            0x04060a,
        );
        let bar = self.l.toolbar;
        self.c.rrect(bar, self.l.px(14.0), PANEL, 0.97);
        self.c
            .rrect_stroke(bar, self.l.px(14.0), 1.0, LINE, 0.85);

        for (b, r) in &self.l.buttons {
            let active = (*b == Btn::Fit && active_fit)
                || (*b == Btn::Actual && active_actual)
                || (*b == Btn::Fullscreen && fullscreen);
            self.button(*r, *b, enabled, hover == Some(*b), active);
        }
    }

    fn button(&mut self, r: Rect, b: Btn, enabled: bool, hovered: bool, active: bool) {
        let rad = self.l.px(9.0);
        if active {
            self.c.rrect(r, rad, ACCENT, 0.18);
            self.c.rrect_stroke(r, rad, 1.0, ACCENT, 0.35);
        } else if hovered && enabled {
            let col = if b == Btn::Delete { 0x3a2026 } else { PANEL2 };
            self.c.rrect(r, rad, col, 0.95);
        }
        let (glyph, icon) = b.icon();
        let color = if !enabled {
            FAINT
        } else if b == Btn::Delete && hovered {
            DANGER
        } else {
            TEXT
        };
        let font = if icon { self.f.icon_18 } else { self.f.ui_12 };
        text(self.c.hdc(), glyph, r, font, color, DT_CENTER.0);
    }

    pub fn empty(&mut self, hover_open: bool) {
        let cx = self.l.w / 2;
        let cy = self.l.h / 2;
        let s = self.l.s;
        // Little picture illustration.
        let ic = Rect::new(cx - self.l.px(48.0), cy - self.l.px(116.0), self.l.px(96.0), self.l.px(96.0));
        self.c.rrect(ic, self.l.px(18.0), PANEL, 0.9);
        self.c.rrect_stroke(ic, self.l.px(18.0), 1.0, LINE, 0.9);
        self.c
            .circle(ic.x as f32 + ic.w as f32 * 0.68, ic.y as f32 + ic.h as f32 * 0.3, 8.0 * s, 0xffc65c, 0.95);
        self.c.triangle(
            (ic.x as f32 + ic.w as f32 * 0.12, ic.y as f32 + ic.h as f32 * 0.82),
            (ic.x as f32 + ic.w as f32 * 0.42, ic.y as f32 + ic.h as f32 * 0.46),
            (ic.x as f32 + ic.w as f32 * 0.72, ic.y as f32 + ic.h as f32 * 0.82),
            0x2f6d9c,
            1.0,
        );
        self.c.triangle(
            (ic.x as f32 + ic.w as f32 * 0.36, ic.y as f32 + ic.h as f32 * 0.84),
            (ic.x as f32 + ic.w as f32 * 0.62, ic.y as f32 + ic.h as f32 * 0.56),
            (ic.x as f32 + ic.w as f32 * 0.9, ic.y as f32 + ic.h as f32 * 0.84),
            0x63c0ff,
            1.0,
        );

        text(
            self.c.hdc(),
            "Open a photo to begin",
            Rect::new(0, cy - self.l.px(6.0), self.l.w, self.l.px(34.0)),
            self.f.ui_15,
            TEXT,
            DT_CENTER.0,
        );
        text(
            self.c.hdc(),
            "Ctrl+O  ·  or drop an image here",
            Rect::new(0, cy + self.l.px(26.0), self.l.w, self.l.px(24.0)),
            self.f.ui_11,
            MUTED,
            DT_CENTER.0,
        );
        let btn = self.l.empty_open;
        self.c
            .rrect(btn, self.l.px(11.0), if hover_open { 0x76b8ff } else { ACCENT }, 0.96);
        text(
            self.c.hdc(),
            "Open photo",
            btn,
            self.f.ui_13b,
            0x08111d,
            DT_CENTER.0,
        );
    }

    pub fn spinner(&mut self, phase: f32, label: &str) {
        let cx = self.l.content.x as f32 + self.l.content.w as f32 / 2.0;
        let cy = self.l.content.y as f32 + self.l.content.h as f32 / 2.0;
        let r = self.l.px(15.0) as f32;
        let start = phase * 360.0;
        self.c
            .arc(cx, cy, r, 2.6 * self.l.s, start, start + 110.0, ACCENT, 1.0);
        self.c.arc(
            cx,
            cy,
            r,
            2.6 * self.l.s,
            start + 150.0,
            start + 240.0,
            ACCENT,
            0.35,
        );
        text(
            self.c.hdc(),
            label,
            Rect::new(
                self.l.content.x,
                (cy + r + 14.0 * self.l.s) as i32,
                self.l.content.w,
                self.l.px(22.0),
            ),
            self.f.ui_12,
            MUTED,
            DT_CENTER.0,
        );
    }

    pub fn toast(&mut self, msg: &str, alpha: f32) {
        if msg.is_empty() || alpha <= 0.0 {
            return;
        }
        let (tw, _) = measure(self.c.hdc(), msg, self.f.ui_12);
        let w = (tw + self.l.px(34.0)).min(self.l.w - self.l.px(40.0));
        let h = self.l.px(36.0);
        let y = if self.l.toolbar.y > 0 {
            self.l.toolbar.y - h - self.l.px(12.0)
        } else {
            self.l.h - h - self.l.px(24.0)
        };
        let r = Rect::new((self.l.w - w) / 2, y, w, h);
        self.c.rrect(r, h / 2, PANEL2, 0.98 * alpha);
        self.c
            .rrect_stroke(r, h / 2, 1.0, LINE, 0.9 * alpha);
        text(
            self.c.hdc(),
            msg,
            r,
            self.f.ui_12,
            TEXT,
            DT_CENTER.0,
        );
    }

    pub fn tooltip(&mut self, b: Btn, alpha: f32) {
        let Some((_, br)) = self.l.buttons.iter().find(|(x, _)| *x == b) else {
            return;
        };
        let tip = b.tip();
        let (tw, th) = measure(self.c.hdc(), tip, self.f.ui_11);
        let w = tw + self.l.px(20.0);
        let h = th + self.l.px(12.0);
        let r = Rect::new(
            (br.x + br.w / 2 - w / 2).clamp(self.l.px(6.0), self.l.w - w - self.l.px(6.0)),
            br.y - h - self.l.px(9.0),
            w,
            h,
        );
        self.c.rrect(r, self.l.px(7.0), 0x0a0d13, 0.97 * alpha);
        self.c
            .rrect_stroke(r, self.l.px(7.0), 1.0, LINE, alpha);
        text(
            self.c.hdc(),
            tip,
            r,
            self.f.ui_11,
            TEXT,
            DT_CENTER.0,
        );
    }

    pub fn crop_overlay(
        &mut self,
        selection: Rect,
        area: Rect,
        hot_handle: Option<u8>,
        size_label: &str,
    ) {
        let a = area;
        let s = selection;
        let dim = 0.58;
        // Dim everything except the selection.
        self.c
            .fill_a(Rect::new(a.x, a.y, a.w, (s.y - a.y).max(0)), 0x000000, dim);
        self.c.fill_a(
            Rect::new(a.x, s.y + s.h, a.w, (a.bottom() - s.bottom()).max(0)),
            0x000000,
            dim,
        );
        self.c.fill_a(
            Rect::new(a.x, s.y, (s.x - a.x).max(0), s.h),
            0x000000,
            dim,
        );
        self.c.fill_a(
            Rect::new(s.right(), s.y, (a.right() - s.right()).max(0), s.h),
            0x000000,
            dim,
        );
        // Rule of thirds.
        for i in 1..3 {
            let x = s.x + s.w * i / 3;
            let y = s.y + s.h * i / 3;
            self.c.fill_a(Rect::new(x, s.y, 1, s.h), 0xffffff, 0.20);
            self.c.fill_a(Rect::new(s.x, y, s.w, 1), 0xffffff, 0.20);
        }
        self.c.fill_a(Rect::new(s.x, s.y, s.w, 1), 0xffffff, 0.95);
        self.c.fill_a(Rect::new(s.x, s.bottom() - 1, s.w, 1), 0xffffff, 0.95);
        self.c.fill_a(Rect::new(s.x, s.y, 1, s.h), 0xffffff, 0.95);
        self.c.fill_a(Rect::new(s.right() - 1, s.y, 1, s.h), 0xffffff, 0.95);
        // Size pill.
        let label = size_label;
        let (tw, th) = measure(self.c.hdc(), label, self.f.ui_11);
        let pr = Rect::new(
            s.x,
            (s.y - th - self.l.px(14.0)).max(self.l.px(4.0)),
            tw + self.l.px(18.0),
            th + self.l.px(10.0),
        );
        self.c.rrect(pr, self.l.px(6.0), 0x0a0d13, 0.92);
        text(
            self.c.hdc(),
            label,
            pr,
            self.f.ui_11,
            TEXT,
            DT_CENTER.0,
        );

        // Handles.
        let hs = self.l.px(9.0);
        let points = handle_points(s, hs);
        for (i, p) in points.iter().enumerate() {
            let col = if hot_handle == Some(i as u8) { ACCENT } else { 0xffffff };
            self.c.fill(Rect::new(p.0, p.1, hs, hs), col);
            self.c.rrect_stroke(
                Rect::new(p.0, p.1, hs, hs),
                1,
                1.0,
                0x0a0d13,
                0.7,
            );
        }
    }

    pub fn crop_bar(&mut self, aspect: u8, hover: Option<CropBtn>, apply_enabled: bool) {
        self.c.vgrad(
            Rect::new(0, self.l.h - self.l.px(170.0), self.l.w, self.l.px(170.0)),
            0.0,
            0.72,
            0x04060a,
        );
        let bar = self.l.cropbar;
        self.c.rrect(bar, self.l.px(14.0), PANEL, 0.97);
        self.c
            .rrect_stroke(bar, self.l.px(14.0), 1.0, LINE, 0.85);
        for (b, r) in &self.l.crop_buttons {
            let active = match b {
                CropBtn::Free => aspect == 0,
                CropBtn::Square => aspect == 1,
                CropBtn::FourThree => aspect == 2,
                CropBtn::SixteenNine => aspect == 3,
                _ => false,
            };
            let enabled = !matches!(b, CropBtn::Apply) || apply_enabled;
            if *b == CropBtn::Apply {
                let col = if !enabled {
                    0x2a3444
                } else if hover == Some(*b) {
                    0x76b8ff
                } else {
                    ACCENT
                };
                self.c.rrect(*r, self.l.px(9.0), col, 1.0);
                text(
                    self.c.hdc(),
                    b.label(),
                    *r,
                    self.f.ui_13b,
                    0x08111d,
                    DT_CENTER.0,
                );
            } else if *b == CropBtn::Cancel {
                if hover == Some(*b) {
                    self.c.rrect(*r, self.l.px(9.0), PANEL2, 0.95);
                }
                text(
                    self.c.hdc(),
                    b.label(),
                    *r,
                    self.f.ui_12,
                    MUTED,
                    DT_CENTER.0,
                );
            } else {
                if active {
                    self.c.rrect(*r, self.l.px(9.0), ACCENT, 0.18);
                    self.c.rrect_stroke(*r, self.l.px(9.0), 1.0, ACCENT, 0.35);
                } else if hover == Some(*b) {
                    self.c.rrect(*r, self.l.px(9.0), PANEL2, 0.95);
                }
                text(
                    self.c.hdc(),
                    b.label(),
                    *r,
                    self.f.ui_12,
                    if active { TEXT } else { MUTED },
                    DT_CENTER.0,
                );
            }
        }
    }

    pub fn confirm(&mut self, title: &str, body: &str, hover_yes: bool, hover_no: bool) -> (Rect, Rect) {
        self.c.fill_a(Rect::new(0, 0, self.l.w, self.l.h), 0x000000, 0.55);
        let cw = self.l.px(430.0).min(self.l.w - self.l.px(40.0));
        let ch = self.l.px(188.0);
        let card = Rect::new((self.l.w - cw) / 2, (self.l.h - ch) / 2, cw, ch);
        self.c.rrect(card, self.l.px(16.0), PANEL, 0.99);
        self.c
            .rrect_stroke(card, self.l.px(16.0), 1.0, LINE, 1.0);
        text(
            self.c.hdc(),
            title,
            Rect::new(card.x + self.l.px(24.0), card.y + self.l.px(20.0), card.w - self.l.px(48.0), self.l.px(28.0)),
            self.f.ui_15,
            TEXT,
            DT_LEFT.0,
        );
        gfx::text_block(
            self.c.hdc(),
            body,
            Rect::new(
                card.x + self.l.px(24.0),
                card.y + self.l.px(56.0),
                card.w - self.l.px(48.0),
                self.l.px(52.0),
            ),
            self.f.ui_12,
            MUTED,
        );
        let bw = self.l.px(104.0);
        let bh = self.l.px(38.0);
        let yes = Rect::new(card.right() - self.l.px(24.0) - bw, card.bottom() - self.l.px(22.0) - bh, bw, bh);
        let no = Rect::new(yes.x - self.l.px(10.0) - bw, yes.y, bw, bh);
        self.c.rrect(yes, self.l.px(9.0), if hover_yes { 0xff8585 } else { DANGER }, 1.0);
        text(self.c.hdc(), "Delete", yes, self.f.ui_13b, 0x1c0b0b, DT_CENTER.0);
        self.c.rrect(no, self.l.px(9.0), if hover_no { PANEL2 } else { 0x1a202b }, 1.0);
        self.c.rrect_stroke(no, self.l.px(9.0), 1.0, LINE, 1.0);
        text(self.c.hdc(), "Cancel", no, self.f.ui_12, TEXT, DT_CENTER.0);
        (yes, no)
    }

    pub fn help(&mut self) {
        self.c.fill_a(Rect::new(0, 0, self.l.w, self.l.h), 0x000000, 0.6);
        let cw = self.l.px(620.0).min(self.l.w - self.l.px(40.0));
        let ch = self.l.px(470.0).min(self.l.h - self.l.px(40.0));
        let card = Rect::new((self.l.w - cw) / 2, (self.l.h - ch) / 2, cw, ch);
        self.c.rrect(card, self.l.px(16.0), PANEL, 0.99);
        self.c.rrect_stroke(card, self.l.px(16.0), 1.0, LINE, 1.0);
        text(
            self.c.hdc(),
            "Keyboard shortcuts",
            Rect::new(card.x + self.l.px(28.0), card.y + self.l.px(20.0), card.w - self.l.px(56.0), self.l.px(30.0)),
            self.f.ui_15,
            TEXT,
            DT_LEFT.0,
        );
        text(
            self.c.hdc(),
            "Esc or F1 to close",
            Rect::new(card.x + self.l.px(28.0), card.y + self.l.px(46.0), card.w - self.l.px(56.0), self.l.px(20.0)),
            self.f.ui_11,
            FAINT,
            DT_LEFT.0,
        );
        let rows: [(&str, &str); 16] = [
            ("← / →", "Previous / next photo"),
            ("Space / Backspace", "Next / previous"),
            ("Home / End", "First / last in folder"),
            ("+ / −", "Zoom in / out"),
            ("Wheel", "Zoom at the pointer"),
            ("0 / 1", "Fit window / actual size"),
            ("Double-click", "Toggle fit and 100%"),
            ("Drag", "Pan a zoomed photo"),
            ("C", "Crop"),
            ("Enter / Esc", "Apply / cancel crop"),
            ("R / Shift+R", "Rotate right / left"),
            ("Del", "Delete to Recycle Bin"),
            ("F11", "Fullscreen"),
            ("F5", "Start / stop slideshow"),
            ("Ctrl+O", "Open another photo"),
            ("Ctrl+C", "Copy the photo"),
        ];
        let col_w = (card.w - self.l.px(56.0)) / 2;
        let top = card.y + self.l.px(82.0);
        let step = self.l.px(44.0).min((card.h - self.l.px(104.0)) / 8);
        for (i, (key, what)) in rows.iter().enumerate() {
            let col = (i % 2) as i32;
            let row = (i / 2) as i32;
            let x = card.x + self.l.px(28.0) + col * col_w;
            let y = top + row * step;
            let (kw, _) = measure(self.c.hdc(), key, self.f.ui_11);
            let pad = self.l.px(7.0);
            let chip = Rect::new(x, y, kw + pad * 2, self.l.px(21.0));
            self.c.rrect(chip, self.l.px(5.0), 0x242b38, 1.0);
            text(self.c.hdc(), key, chip, self.f.ui_11, TEXT, DT_CENTER.0);
            text(
                self.c.hdc(),
                what,
                Rect::new(x + chip.w + self.l.px(10.0), y, col_w - chip.w - self.l.px(10.0), chip.h),
                self.f.ui_11,
                TEXT,
                DT_LEFT.0,
            );
        }
    }
}

/// Positions of the eight crop handles (top-left, top, top-right, ... ).
pub fn handle_points(r: Rect, hs: i32) -> [(i32, i32); 8] {
    let h = hs / 2;
    let cx = r.x + r.w / 2;
    let cy = r.y + r.h / 2;
    [
        (r.x - h, r.y - h),
        (cx - h, r.y - h),
        (r.right() - h, r.y - h),
        (r.right() - h, cy - h),
        (r.right() - h, r.bottom() - h),
        (cx - h, r.bottom() - h),
        (r.x - h, r.bottom() - h),
        (r.x - h, cy - h),
    ]
}

pub fn accent_color() -> u32 {
    ACCENT
}

pub fn muted_color() -> u32 {
    MUTED
}

pub fn text_color() -> u32 {
    TEXT
}

pub fn panel_color() -> u32 {
    PANEL
}

pub fn danger_color() -> u32 {
    DANGER
}

pub fn rgb_line() -> u32 {
    rgb(43, 51, 66)
}




