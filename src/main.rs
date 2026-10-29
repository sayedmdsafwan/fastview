// ============================================================================
// FastView - a tiny, instant photo viewer for Windows, written in Rust.
// ============================================================================
//
// Design goals, in order:
//   1. Open instantly, even on a low-power CPU. No GPU initialisation, no
//      framework, no runtime: a plain Win32 window, one background decoder.
//   2. Keep navigating instantly: the current picture is decoded full-size and
//      the previous/next fit previews are prefetched by the loader thread.
//   3. Stay out of the way: fit to window, wheel zoom, drag to pan, crop, rotate,
//      delete to the Recycle Bin, fullscreen and a slideshow. No editing suite.
//
// Rendering: one 32-bit DIB, drawn with hand-written AA primitives + GDI text,
// presented with a single BitBlt. Photo resizing uses fast_image_resize (SIMD).
//
// Shortcuts are listed in the in-app help (F1).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(dead_code, unused_imports, unused_variables, unused_mut, unused_assignments)]

mod gfx;
mod imaging;
mod library;
mod loader;
mod platform;
mod timing;
mod ui;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, EndPaint, HDC, InvalidateRect, PAINTSTRUCT,
    ScreenToClient, UpdateWindow,
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::*;

use gfx::{Canvas, Fonts};
use imaging::{Fit, Photo};
use ui::{Btn, CropBtn, Layout, Rect};

const WM_MOUSELEAVE_MSG: u32 = 0x02A3;
const WM_ANIM: usize = 1;
const WM_SLIDESHOW: usize = 2;
const SLIDE_MS: u32 = 4200;

struct Entry {
    meta: Option<loader::Meta>,
    fit: Option<Fit>,
    fit_size: (u32, u32),
    full: Option<Arc<Photo>>,
}

impl Entry {
    fn new() -> Self {
        Self {
            meta: None,
            fit: None,
            fit_size: (0, 0),
            full: None,
        }
    }
}

struct View {
    zoom: f32,
    cx: f32,
    cy: f32,
    fit: bool,
    initialized: bool,
}

impl View {
    fn new() -> Self {
        Self {
            zoom: 1.0,
            cx: 0.0,
            cy: 0.0,
            fit: true,
            initialized: false,
        }
    }
}

struct ViewCache {
    key: (usize, u32, u32, u32, i32, i32),
    dest: Rect,
    px: Vec<u32>,
    w: i32,
    h: i32,
}

struct Toast {
    text: String,
    start: Instant,
    until: Instant,
}

#[derive(Clone)]
struct Confirm {
    path: PathBuf,
    title: String,
    body: String,
}

#[derive(Copy, Clone, PartialEq)]
enum Drag {
    None,
    Pan {
        mx: i32,
        my: i32,
        cx: f32,
        cy: f32,
    },
    New {
        x: f32,
        y: f32,
    },
    Move {
        x: f32,
        y: f32,
        rect: [f32; 4],
    },
    Handle {
        idx: u8,
        x: f32,
        y: f32,
        rect: [f32; 4],
    },
}

#[derive(Clone)]
struct CropState {
    rect: [f32; 4], // x, y, w, h in image pixels
    aspect: u8,     // 0 free, 1 1:1, 2 4:3, 3 16:9
    drag: Drag,
}

struct App {
    hwnd: HWND,
    dpi: u32,
    fonts: Fonts,
    canvas: Canvas,
    layout: Layout,
    lib: Vec<PathBuf>,
    index: Option<usize>,
    current: Option<PathBuf>,
    cache: HashMap<PathBuf, Entry>,
    view: View,
    view_cache: Option<ViewCache>,
    loader: loader::Loader,
    seq: u64,
    loading: bool,
    fit_pending: bool,
    full_pending: bool,
    error: Option<String>,
    hover: Option<Btn>,
    hover_crop: Option<CropBtn>,
    hover_empty: bool,
    hover_since: Option<Instant>,
    pressed: Option<Btn>,
    mouse: (i32, i32),
    track_leave: bool,
    crop: Option<CropState>,
    confirm: Option<Confirm>,
    confirm_yes: Rect,
    confirm_no: Rect,
    confirm_hover: u8,
    help: bool,
    slideshow: bool,
    toast: Option<Toast>,
    fullscreen: platform::Fullscreen,
    anim_on: bool,
    in_size_move: bool,
    pan: Option<Drag>,
    drag: Drag,
    arrow: HCURSOR,
    hand: HCURSOR,
    cross: HCURSOR,
    size_nwse: HCURSOR,
    size_nesw: HCURSOR,
    size_we: HCURSOR,
    size_ns: HCURSOR,
    size_all: HCURSOR,
}

fn main() {
    if std::env::args().any(|a| a == "--set-default") {
        platform::open_default_apps();
        return;
    }
    timing::start();
    platform::init_process();
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let hinst = platform::hinstance();
    platform::register_class(hinst, Some(wndproc));
    let dpi = platform::dpi();
    let scale = dpi as f32 / 96.0;
    let cw = (1240.0 * scale) as i32;
    let ch = (820.0 * scale) as i32;
    let hwnd = match platform::create_window(hinst, "FastView", cw, ch) {
        Ok(h) => h,
        Err(_) => return,
    };
    platform::set_window_icon(hwnd, hinst);
    platform::dark_chrome(hwnd);
    unsafe {
        DragAcceptFiles(hwnd, true);
    }

    let mut app = Box::new(App::new(hwnd, dpi));
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(first) = args.iter().find(|a| !a.is_empty()) {
        app.open_path(PathBuf::from(first));
    }
    // Per-user "Open with" registration, off the UI thread. The installer does
    // this too, but this also covers portable copies and skipped checkboxes.
    let _ = std::thread::Builder::new()
        .name("fastview-assoc".into())
        .spawn(platform::register_open_with);
    timing::mark("window ready");
    let app_ptr = Box::into_raw(app);
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, app_ptr as isize);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);
    }

    unsafe {
        let mut msg = MSG::default();
        loop {
            let rc = GetMessageW(&mut msg, None, 0, 0).0;
            if rc <= 0 {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    // Keep the box alive until the message loop ends.
    let _app = unsafe { Box::from_raw(app_ptr) };
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    }
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut App;
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    }
    unsafe { (*ptr).handle(hwnd, msg, wp, lp) }
}

impl App {
    fn new(hwnd: HWND, dpi: u32) -> Self {
        let (w, h) = platform::client_size(hwnd);
        let canvas = Canvas::new(hwnd, w, h).expect("canvas");
        Self {
            hwnd,
            dpi,
            fonts: Fonts::new(dpi),
            canvas,
            layout: Layout::new(w, h, dpi),
            lib: Vec::new(),
            index: None,
            current: None,
            cache: HashMap::new(),
            view: View::new(),
            view_cache: None,
            loader: loader::Loader::new(hwnd),
            seq: 0,
            loading: false,
            fit_pending: false,
            full_pending: false,
            error: None,
            hover: None,
            hover_crop: None,
            hover_empty: false,
            hover_since: None,
            pressed: None,
            mouse: (0, 0),
            track_leave: false,
            crop: None,
            confirm: None,
            confirm_yes: Rect::default(),
            confirm_no: Rect::default(),
            confirm_hover: 0,
            help: false,
            slideshow: false,
            toast: None,
            fullscreen: platform::Fullscreen::new(),
            anim_on: false,
            in_size_move: false,
            pan: None,
            drag: Drag::None,
            arrow: platform::load_cursor(IDC_ARROW),
            hand: platform::load_cursor(IDC_HAND),
            cross: platform::load_cursor(IDC_CROSS),
            size_nwse: platform::load_cursor(IDC_SIZENWSE),
            size_nesw: platform::load_cursor(IDC_SIZENESW),
            size_we: platform::load_cursor(IDC_SIZEWE),
            size_ns: platform::load_cursor(IDC_SIZENS),
            size_all: platform::load_cursor(IDC_SIZEALL),
        }
    }

    // ------------------------------------------------------------------ events

    fn handle(&mut self, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
                self.paint(hdc);
                unsafe {
                    let _ = EndPaint(hwnd, &ps);
                }
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_SIZE => {
                if wp.0 != SIZE_MINIMIZED as usize {
                    let (w, h) = platform::client_size(hwnd);
                    self.on_size(w, h);
                }
                LRESULT(0)
            }
            WM_ENTERSIZEMOVE => {
                self.in_size_move = true;
                LRESULT(0)
            }
            WM_EXITSIZEMOVE => {
                self.in_size_move = false;
                self.on_size(self.layout.w, self.layout.h);
                LRESULT(0)
            }
            WM_DPICHANGED => {
                self.dpi = (wp.0 & 0xffff) as u32;
                self.fonts = Fonts::new(self.dpi);
                let r = unsafe { &*(lp.0 as *const RECT) };
                unsafe {
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                let (w, h) = platform::client_size(hwnd);
                self.on_size(w, h);
                LRESULT(0)
            }
            WM_GETMINMAXINFO => {
                let mmi = unsafe { &mut *(lp.0 as *mut MINMAXINFO) };
                let s = self.layout.s;
                mmi.ptMinTrackSize.x = (560.0 * s) as i32;
                mmi.ptMinTrackSize.y = (420.0 * s) as i32;
                LRESULT(0)
            }
            WM_TIMER => {
                self.on_timer(wp.0);
                LRESULT(0)
            }
            loader::WM_APP_LOADED => {
                self.on_loaded();
                LRESULT(0)
            }
            WM_KEYDOWN => {
                self.on_key(wp.0 as u16);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let x = lo(lp.0 as usize);
                let y = hi(lp.0 as usize);
                self.on_mouse_move(x, y);
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let x = lo(lp.0 as usize);
                let y = hi(lp.0 as usize);
                self.on_mouse_down(x, y);
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let x = lo(lp.0 as usize);
                let y = hi(lp.0 as usize);
                self.on_mouse_up(x, y);
                LRESULT(0)
            }
            WM_LBUTTONDBLCLK => {
                let x = lo(lp.0 as usize);
                let y = hi(lp.0 as usize);
                self.on_double_click(x, y);
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                self.on_wheel(wp.0 as usize, lp.0 as usize);
                LRESULT(0)
            }
            WM_MOUSELEAVE_MSG => {
                self.track_leave = false;
                self.hover = None;
                self.hover_crop = None;
                self.hover_empty = false;
                self.hover_since = None;
                self.invalidate();
                LRESULT(0)
            }
            WM_SETCURSOR => {
                if (lp.0 as usize & 0xffff) as u32 == HTCLIENT {
                    self.apply_cursor();
                    return LRESULT(1);
                }
                unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
            }
            WM_DROPFILES => {
                self.on_drop(HDROP(wp.0 as *mut core::ffi::c_void));
                LRESULT(0)
            }
            WM_CLOSE => {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                self.loader.stop();
                unsafe { PostQuitMessage(0) };
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
        }
    }

    fn on_size(&mut self, w: i32, h: i32) {
        if w <= 0 || h <= 0 {
            return;
        }
        if !self.canvas.resize(w, h) {
            return;
        }
        self.layout = Layout::new(w, h, self.dpi);
        self.view_cache = None;
        self.fit_pending = false;
        if let Some((iw, ih)) = self.dims() {
            if self.view.fit {
                self.view.zoom = self.fit_zoom(iw, ih);
                self.view.cx = iw as f32 / 2.0;
                self.view.cy = ih as f32 / 2.0;
                if !self.in_size_move {
                    self.ensure_fit();
                }
            } else {
                self.clamp_center(iw, ih);
            }
        }
        self.invalidate();
    }

    // -------------------------------------------------------------- file flow

    fn open_path(&mut self, path: PathBuf) {
        if !path.exists() {
            self.toast("That file does not exist");
            return;
        }
        if path.is_dir() {
            let list = library::scan_folder(&path);
            if let Some(first) = list.first().cloned() {
                self.lib = list;
                self.index = Some(0);
                self.open_current(&first, true);
            } else {
                self.lib.clear();
                self.index = None;
                self.current = None;
                self.toast("No pictures in this folder");
                self.invalidate();
            }
            return;
        }
        let mut list = library::scan_folder(&path);
        let mut idx = list.iter().position(|p| Self::same_path(p, &path));
        if idx.is_none() {
            list.push(path.clone());
            idx = Some(list.len() - 1);
        }
        self.lib = list;
        self.index = idx;
        self.open_current(&path, true);
    }

    fn same_path(a: &Path, b: &Path) -> bool {
        a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
    }

    fn open_current(&mut self, path: &Path, reset_view: bool) {
        self.current = Some(path.to_path_buf());
        self.error = None;
        self.seq = self.seq.wrapping_add(1);
        self.view_cache = None;
        self.fit_pending = false;
        self.full_pending = false;
        self.confirm = None;
        self.help = false;
        self.crop = None;
        let dims = self
            .cache
            .get(path)
            .and_then(|e| e.meta.as_ref())
            .map(|m| (m.w, m.h))
            .or_else(|| self.cache.get(path).and_then(|e| e.full.as_ref()).map(|p| (p.w, p.h)));
        if reset_view || !self.view.initialized {
            self.view = View::new();
            if let Some((iw, ih)) = dims {
                self.view.initialized = true;
                self.view.zoom = self.fit_zoom(iw, ih);
                self.view.cx = iw as f32 / 2.0;
                self.view.cy = ih as f32 / 2.0;
            }
        }
        if let Some((iw, ih)) = dims {
            self.view.initialized = true;
            if self.view.fit {
                self.view.zoom = self.fit_zoom(iw, ih);
                self.view.cx = iw as f32 / 2.0;
                self.view.cy = ih as f32 / 2.0;
            }
        }

        // Drop full-resolution buffers for the pictures we are not looking at.
        for (p, e) in self.cache.iter_mut() {
            if p != path {
                e.full = None;
            }
        }

        let (cw, ch) = (self.layout.content.w as u32, self.layout.content.h as u32);
        let entry = self.cache.get(path);
        let fit_ok = entry
            .and_then(|e| e.fit.as_ref().map(|_| e.fit_size == (cw, ch)))
            .unwrap_or(false);
        let has_full = entry.and_then(|e| e.full.as_ref()).is_some();
        self.loading = !fit_ok;
        self.fit_pending = false;
        self.full_pending = false;
        if !has_full {
            self.loader
                .current(path.to_path_buf(), self.seq, true, cw, ch);
            self.full_pending = true;
            self.fit_pending = true;
        } else if !fit_ok {
            self.loader
                .current(path.to_path_buf(), self.seq, false, cw, ch);
            self.fit_pending = true;
        }
        self.update_title();
        if fit_ok {
            self.prefetch_neighbours();
        }
        self.invalidate();
    }

    fn reload_current(&mut self, reset: bool) {
        if let Some(path) = self.current.clone() {
            self.open_current(&path, reset);
        }
    }

    fn dims(&self) -> Option<(u32, u32)> {
        let path = self.current.as_ref()?;
        let e = self.cache.get(path)?;
        e.meta
            .as_ref()
            .map(|m| (m.w, m.h))
            .or_else(|| e.full.as_ref().map(|p| (p.w, p.h)))
    }

    fn fit_zoom(&self, iw: u32, ih: u32) -> f32 {
        let cw = self.layout.content.w.max(1) as f32;
        let ch = self.layout.content.h.max(1) as f32;
        (cw / iw as f32).min(ch / ih as f32).min(1.0)
    }

    fn reset_fit(&mut self) {
        if let Some((iw, ih)) = self.dims() {
            self.view.fit = true;
            self.view.initialized = true;
            self.view.zoom = self.fit_zoom(iw, ih);
            self.view.cx = iw as f32 / 2.0;
            self.view.cy = ih as f32 / 2.0;
            self.view_cache = None;
            self.ensure_fit();
            self.invalidate();
        }
    }

    fn actual_size(&mut self) {
        if let Some((iw, ih)) = self.dims() {
            self.view.fit = false;
            self.view.initialized = true;
            self.view.zoom = 1.0;
            self.view.cx = iw as f32 / 2.0;
            self.view.cy = ih as f32 / 2.0;
            self.view_cache = None;
            self.request_full();
            self.invalidate();
        }
    }

    fn nav(&mut self, delta: i32, wrap: bool) {
        if self.lib.is_empty() {
            return;
        }
        let Some(mut i) = self.index else { return };
        if wrap {
            i = (i as i32 + delta).rem_euclid(self.lib.len() as i32) as usize;
        } else {
            let n = i as i32 + delta;
            if n < 0 || n >= self.lib.len() as i32 {
                return;
            }
            i = n as usize;
        }
        self.index = Some(i);
        let path = self.lib[i].clone();
        self.open_current(&path, true);
    }

    fn prefetch_neighbours(&mut self) {
        if self.lib.len() < 2 {
            return;
        }
        let Some(i) = self.index else { return };
        let (cw, ch) = (self.layout.content.w as u32, self.layout.content.h as u32);
        let mut jobs = Vec::new();
        if i > 0 {
            jobs.push(self.lib[i - 1].clone());
        }
        if i + 1 < self.lib.len() {
            jobs.push(self.lib[i + 1].clone());
        }
        // A second look-ahead makes holding the arrow key feel instant.
        if i + 2 < self.lib.len() {
            jobs.push(self.lib[i + 2].clone());
        }
        if i >= 2 {
            jobs.push(self.lib[i - 2].clone());
        }
        for p in jobs {
            let cached = self
                .cache
                .get(&p)
                .map(|e| e.fit.is_some() && e.fit_size == (cw, ch))
                .unwrap_or(false);
            if !cached {
                self.loader
                    .prefetch(p, self.seq, cw, ch);
            }
        }
    }

    fn trim_cache(&mut self) {
        if self.cache.len() <= 12 {
            return;
        }
        let mut keep: HashSet<PathBuf> = HashSet::new();
        if let Some(p) = &self.current {
            keep.insert(p.clone());
        }
        if let Some(idx) = self.index {
            for d in -3i32..=3 {
                let i = idx as i32 + d;
                if i >= 0 && (i as usize) < self.lib.len() {
                    keep.insert(self.lib[i as usize].clone());
                }
            }
        }
        self.cache
            .retain(|p, e| keep.contains(p) || e.full.is_some());
    }

    fn request_full(&mut self) {        if self.full_pending {
            return;
        }
        let Some(path) = self.current.clone() else { return };
        let has_full = self
            .cache
            .get(&path)
            .map(|e| e.full.is_some())
            .unwrap_or(false);
        if has_full {
            return;
        }
        self.full_pending = true;
        self.loader.current(path, self.seq, true, 0, 0);
    }

    fn ensure_fit(&mut self) {
        if !self.view.fit || self.in_size_move {
            return;
        }
        let Some(path) = self.current.clone() else { return };
        let (cw, ch) = (self.layout.content.w as u32, self.layout.content.h as u32);
        let fit_ok = self
            .cache
            .get(&path)
            .map(|e| e.fit.is_some() && e.fit_size == (cw, ch))
            .unwrap_or(false);
        if fit_ok {
            self.loading = false;
            return;
        }
        let full = self.cache.get(&path).and_then(|e| e.full.clone());
        if let Some(full) = full {
            if let Some(fit) = imaging::make_fit(&full, cw, ch) {
                if let Some(e) = self.cache.get_mut(&path) {
                    e.fit = Some(fit);
                    e.fit_size = (cw, ch);
                }
                self.loading = false;
                timing::mark("fit view ready");
                return;
            }
        }
        if !self.fit_pending {
            self.fit_pending = true;
            self.loading = true;
            let want_full = self
                .cache
                .get(&path)
                .map(|e| e.full.is_none())
                .unwrap_or(true);
            if want_full {
                self.full_pending = true;
            }
            self.loader.current(path, self.seq, want_full, cw, ch);
        }
    }

    fn on_loaded(&mut self) {
        let results = self.loader.poll();
        let mut current_changed = false;
        for r in results {
            if r.seq != self.seq {
                continue;
            }
            let is_current = self.current.as_ref() == Some(&r.path);
            let (cw, ch) = (self.layout.content.w as u32, self.layout.content.h as u32);
            let e = self.cache.entry(r.path.clone()).or_insert_with(Entry::new);
            if let Some(meta) = r.meta {
                e.meta = Some(meta);
            }
            if let Some(fit) = r.fit {
                e.fit = Some(fit);
                if r.fit_w > 0 && r.fit_h > 0 {
                    e.fit_size = (r.fit_w, r.fit_h);
                }
            }
            if let Some(photo) = r.photo {
                e.full = Some(photo);
            }
            if is_current {
                self.loading = false;
                if !self.view.initialized {
                    if let Some((iw, ih)) = self.dims() {
                        self.view.initialized = true;
                        self.view.zoom = self.fit_zoom(iw, ih);
                        self.view.cx = iw as f32 / 2.0;
                        self.view.cy = ih as f32 / 2.0;
                    }
                }
                if let Some(err) = r.error {
                    self.error = Some(err);
                }
                self.fit_pending = false;
                self.full_pending = false;
                // If the result carried a full decode but no fit (zoom path),
                // build the fit view lazily on the next paint.
                if self.view.fit {
                    self.ensure_fit();
                }
                self.prefetch_neighbours();
                self.trim_cache();
                current_changed = true;
            }
        }
        if current_changed {
            self.view_cache = None;
            self.invalidate();
        }
        self.update_timer();
    }

    // ------------------------------------------------------------------ paint

    fn paint(&mut self, hdc: HDC) {
        if self.canvas.w != self.layout.w || self.canvas.h != self.layout.h {
            let _ = self.canvas.resize(self.layout.w, self.layout.h);
        }
        if self.view.fit {
            self.ensure_fit();
        }
        self.canvas.clear(ui::BG);

        let has_image = self.current.is_some();
        if has_image {
            self.draw_photo();
            let (name, meta, zoom) = self.top_texts();
            {
                let mut ch = ui::Chrome {
                    c: &mut self.canvas,
                    f: &self.fonts,
                    l: &self.layout,
                };
                if let Some(crop) = &self.crop {
                    let hint = "Drag the corners  ·  Enter to apply  ·  Esc to cancel";
                    ch.top_bar(&name, &meta, &zoom, Some(hint));
                } else {
                    ch.top_bar(&name, &meta, &zoom, None);
                }
            }
            if self.crop.is_some() {
                self.draw_crop();
            } else {
                let mut ch = ui::Chrome {
                    c: &mut self.canvas,
                    f: &self.fonts,
                    l: &self.layout,
                };
                let enabled = self.error.is_none();
                ch.toolbar(
                    enabled,
                    self.hover,
                    self.view.fit,
                    !self.view.fit && (self.view.zoom - 1.0).abs() < 0.001,
                    self.fullscreen.is_active(),
                );
                if let Some(h) = self.hover {
                    if enabled {
                        let now = Instant::now();
                        if let Some(t) = self.hover_since {
                            let e = now.duration_since(t).as_secs_f32();
                            if e > 0.45 {
                                let a = ((e - 0.45) / 0.12).clamp(0.0, 1.0);
                                ch.tooltip(h, a);
                            }
                        }
                    }
                }
            }
        } else {
            let mut ch = ui::Chrome {
                c: &mut self.canvas,
                f: &self.fonts,
                l: &self.layout,
            };
            ch.empty(self.hover_empty);
        }

        if self.loading && self.error.is_none() {
            let phase = (Instant::now() - self.start_time()).as_secs_f32() % 1.0;
            let label = if self.full_pending && self.current.as_ref().and_then(|p| self.cache.get(p)).and_then(|e| e.fit.as_ref()).is_some() {
                "Loading full resolution…"
            } else {
                "Opening photo…"
            };
            let mut ch = ui::Chrome {
                c: &mut self.canvas,
                f: &self.fonts,
                l: &self.layout,
            };
            ch.spinner(phase, label);
        }

        if let Some(toast) = &self.toast {
            let now = Instant::now();
            if now < toast.until {
                let e = now.duration_since(toast.start).as_secs_f32();
                let remain = toast.until.saturating_duration_since(now).as_secs_f32();
                let alpha = (e / 0.10).clamp(0.0, 1.0) * (remain / 0.25).clamp(0.0, 1.0);
                let text = toast.text.clone();
                let mut ch = ui::Chrome {
                    c: &mut self.canvas,
                    f: &self.fonts,
                    l: &self.layout,
                };
                ch.toast(&text, alpha);
            }
        }

        if let Some(confirm) = self.confirm.clone() {
            let mut ch = ui::Chrome {
                c: &mut self.canvas,
                f: &self.fonts,
                l: &self.layout,
            };
            let (yes, no) = ch.confirm(
                &confirm.title,
                &confirm.body,
                self.confirm_hover == 1,
                self.confirm_hover == 2,
            );
            self.confirm_yes = yes;
            self.confirm_no = no;
        }
        if self.help {
            let mut ch = ui::Chrome {
                c: &mut self.canvas,
                f: &self.fonts,
                l: &self.layout,
            };
            ch.help();
        }

        self.canvas.present(hdc);
        self.update_timer();
    }

    fn start_time(&self) -> Instant {
        // The spinner only needs a monotonic phase; a shared epoch is enough.
        static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *EPOCH.get_or_init(Instant::now)
    }

    fn draw_photo(&mut self) {
        let Some(path) = self.current.clone() else { return };
        if let Some(err) = &self.error {
            let content = self.layout.content;
            let w = self.layout.px(540.0).min(content.w - self.layout.px(40.0));
            let h = self.layout.px(210.0);
            let card = Rect::new(
                content.x + (content.w - w) / 2,
                content.y + (content.h - h) / 2,
                w,
                h,
            );
            self.canvas.rrect(card, self.layout.px(16.0), ui::PANEL, 0.98);
            self.canvas
                .rrect_stroke(card, self.layout.px(16.0), 1.0, ui::LINE, 0.9);
            let icon_cx = card.x as f32 + self.layout.px(38.0) as f32;
            let icon_cy = card.y as f32 + self.layout.px(44.0) as f32;
            self.canvas
                .circle(icon_cx, icon_cy, self.layout.px(15.0) as f32, ui::DANGER, 0.15);
            self.canvas.fill(
                Rect::new(
                    icon_cx as i32 - self.layout.px(1.5),
                    icon_cy as i32 - self.layout.px(7.0),
                    self.layout.px(3.0),
                    self.layout.px(10.0),
                ),
                ui::DANGER,
            );
            self.canvas.circle(
                icon_cx,
                icon_cy + self.layout.px(6.0) as f32,
                self.layout.px(1.6) as f32,
                ui::DANGER,
                1.0,
            );
            gfx::text(
                self.canvas.hdc(),
                "Couldn't open this photo",
                Rect::new(
                    card.x + self.layout.px(64.0),
                    card.y + self.layout.px(24.0),
                    card.w - self.layout.px(88.0),
                    self.layout.px(26.0),
                ),
                self.fonts.ui_15,
                ui::TEXT,
                DT_LEFT.0,
            );
            let name = self
                .current
                .as_ref()
                .map(|p| library::file_name(p))
                .unwrap_or_default();
            gfx::text(
                self.canvas.hdc(),
                &name,
                Rect::new(
                    card.x + self.layout.px(64.0),
                    card.y + self.layout.px(50.0),
                    card.w - self.layout.px(88.0),
                    self.layout.px(20.0),
                ),
                self.fonts.ui_11,
                ui::MUTED,
                DT_LEFT.0 | DT_END_ELLIPSIS.0,
            );
            gfx::text_block(
                self.canvas.hdc(),
                "The file may be damaged, or it uses a format whose Windows codec is not installed.",
                Rect::new(
                    card.x + self.layout.px(26.0),
                    card.y + self.layout.px(84.0),
                    card.w - self.layout.px(52.0),
                    self.layout.px(46.0),
                ),
                self.fonts.ui_12,
                ui::MUTED,
            );
            gfx::text(
                self.canvas.hdc(),
                "Press  →  for the next photo   ·   Ctrl+O to open another",
                Rect::new(
                    card.x + self.layout.px(26.0),
                    card.y + self.layout.px(136.0),
                    card.w - self.layout.px(52.0),
                    self.layout.px(22.0),
                ),
                self.fonts.ui_11,
                ui::FAINT,
                DT_LEFT.0,
            );
            let _ = err;
            return;
        }
        let Some(entry) = self.cache.get(&path) else {
            return;
        };
        let (cw, ch) = (self.layout.content.w, self.layout.content.h);
        let content = self.layout.content;

        if self.view.fit {
            let Some(fit) = entry.fit.as_ref() else { return };
            if entry.fit_size != (cw as u32, ch as u32) {
                return;
            }
            let x = content.x + (cw - fit.w as i32) / 2;
            let y = content.y + (ch - fit.h as i32) / 2;
            self.canvas.blit(x, y, &fit.px, fit.w as i32, fit.h as i32);
            return;
        }

        let Some(full) = entry.full.as_ref() else {
            // Full resolution is still on its way; show the sharp fit preview.
            if let Some(fit) = entry.fit.as_ref() {
                let x = content.x + (cw - fit.w as i32) / 2;
                let y = content.y + (ch - fit.h as i32) / 2;
                self.canvas.blit(x, y, &fit.px, fit.w as i32, fit.h as i32);
            }
            return;
        };

        let (left, top) = self.view_origin(full.w, full.h);
        let z = self.view.zoom;
        if (z - 1.0).abs() < 0.001 {
            let left_i = left.round() as i32;
            let top_i = top.round() as i32;
            let sx = left_i.max(0);
            let sy = top_i.max(0);
            let ex = (left_i + cw).min(full.w as i32);
            let ey = (top_i + ch).min(full.h as i32);
            if ex > sx && ey > sy {
                self.canvas.blit_sub(
                    content.x + sx - left_i,
                    content.y + sy - top_i,
                    &full.px,
                    full.w as i32,
                    full.h as i32,
                    sx,
                    sy,
                    ex - sx,
                    ey - sy,
                );
            }
            return;
        }

        // Arbitrary zoom: resize only the visible part at screen resolution.
        let sx0 = left.max(0.0).floor();
        let sy0 = top.max(0.0).floor();
        let sx1 = (left + cw as f32 / z).min(full.w as f32).ceil();
        let sy1 = (top + ch as f32 / z).min(full.h as f32).ceil();
        if sx1 <= sx0 || sy1 <= sy0 {
            return;
        }
        let dw = (((sx1 - sx0) * z).round() as i32).max(1);
        let dh = (((sy1 - sy0) * z).round() as i32).max(1);
        let dx = content.x + ((sx0 - left) * z).round() as i32;
        let dy = content.y + ((sy0 - top) * z).round() as i32;
        let key = (
            full.px.as_ptr() as usize,
            z.to_bits(),
            self.view.cx.to_bits(),
            self.view.cy.to_bits(),
            cw,
            ch,
        );
        let need = self
            .view_cache
            .as_ref()
            .map(|v| v.key != key)
            .unwrap_or(true);
        if need {
            let px = imaging::resize_region(
                full,
                sx0 as f64,
                sy0 as f64,
                (sx1 - sx0) as f64,
                (sy1 - sy0) as f64,
                dw as u32,
                dh as u32,
            );
            if let Some(px) = px {
                self.view_cache = Some(ViewCache {
                    key,
                    dest: Rect::new(dx, dy, dw, dh),
                    px,
                    w: dw,
                    h: dh,
                });
            }
        }
        if let Some(vc) = &self.view_cache {
            if vc.key == key {
                self.canvas
                    .blit(vc.dest.x, vc.dest.y, &vc.px, vc.w, vc.h);
            }
        }
    }

    fn draw_crop(&mut self) {
        let Some(crop) = &self.crop else { return };
        let Some((iw, ih)) = self.dims() else { return };
        let (left, top) = self.view_origin(iw, ih);
        let z = self.zoom_now(iw, ih);
        let content = self.layout.content;
        let sx = content.x as f32 + (crop.rect[0] - left) * z;
        let sy = content.y as f32 + (crop.rect[1] - top) * z;
        let sw = crop.rect[2] * z;
        let sh = crop.rect[3] * z;
        let sel = Rect::new(sx.round() as i32, sy.round() as i32, sw.round() as i32, sh.round() as i32);
        let hot = match crop.drag {
            Drag::Handle { idx, .. } => Some(idx),
            _ => None,
        };
        let label = format!(
            "{} × {}",
            crop.rect[2].round().max(1.0) as u32,
            crop.rect[3].round().max(1.0) as u32
        );
        {
            let mut ch = ui::Chrome {
                c: &mut self.canvas,
                f: &self.fonts,
                l: &self.layout,
            };
            ch.crop_overlay(sel, content, hot, &label);
            ch.crop_bar(crop.aspect, self.hover_crop, crop.rect[2] >= 1.0 && crop.rect[3] >= 1.0);
        }
        let _ = sel;
    }

    fn top_texts(&self) -> (String, String, String) {
        let Some(path) = &self.current else {
            return (String::new(), String::new(), String::new());
        };
        let name = library::file_name(path);
        let meta = if let Some(e) = self.cache.get(path) {
            let idx = self
                .index
                .map(|i| format!("{} / {}", i + 1, self.lib.len()))
                .unwrap_or_default();
            let m = match &e.meta {
                Some(m) => {
                    let mut s = format!(
                        "{}  ·  {} × {}  ·  {}  ·  {}",
                        idx,
                        m.w,
                        m.h,
                        m.format,
                        library::size_label(m.file_size)
                    );
                    if self.slideshow {
                        s.push_str("  ·  Slideshow");
                    }
                    s
                }
                None => idx,
            };
            m
        } else {
            String::new()
        };
        let zoom = if self.error.is_some() {
            String::new()
        } else if self.view.fit {
            "Fit".to_string()
        } else {
            format!("{}%", (self.view.zoom * 100.0).round() as i32)
        };
        (name, meta, zoom)
    }

    // ------------------------------------------------------------------ input

    fn on_key(&mut self, vk: u16) {
        let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) } as u16 & 0x8000 != 0;
        let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } as u16 & 0x8000 != 0;

        if self.help {
            if vk == VK_ESCAPE.0 || vk == VK_F1.0 {
                self.help = false;
                self.invalidate();
            }
            return;
        }
        if self.confirm.is_some() {
            match vk {
                v if v == VK_RETURN.0 => self.confirm_delete(),
                v if v == VK_ESCAPE.0 => {
                    self.confirm = None;
                    self.invalidate();
                }
                _ => {}
            }
            return;
        }

        if ctrl && (vk == b'O' as u16) {
            self.open_dialog();
            return;
        }
        if ctrl && (vk == b'D' as u16) {
            platform::open_default_apps();
            return;
        }
        if ctrl && (vk == b'C' as u16) {
            self.copy_image();
            return;
        }

        if let Some(crop) = &mut self.crop {
            match vk {
                v if v == VK_RETURN.0 => {
                    self.apply_crop();
                }
                v if v == VK_ESCAPE.0 => {
                    self.crop = None;
                    self.invalidate();
                }
                v if v == VK_LEFT.0 => {
                    nudge(crop, -1.0, 0.0);
                    self.invalidate();
                }
                v if v == VK_RIGHT.0 => {
                    nudge(crop, 1.0, 0.0);
                    self.invalidate();
                }
                v if v == VK_UP.0 => {
                    nudge(crop, 0.0, -1.0);
                    self.invalidate();
                }
                v if v == VK_DOWN.0 => {
                    nudge(crop, 0.0, 1.0);
                    self.invalidate();
                }
                _ => {}
            }
            return;
        }

        match vk {
            v if v == VK_ESCAPE.0 => {
                if self.slideshow {
                    self.toggle_slideshow();
                }
                if self.fullscreen.is_active() {
                    self.fullscreen.toggle(self.hwnd);
                    let (w, h) = platform::client_size(self.hwnd);
                    self.on_size(w, h);
                }
            }
            v if v == VK_LEFT.0 || v == VK_PRIOR.0 => self.nav(-1, false),
            v if v == VK_RIGHT.0 || v == VK_NEXT.0 => self.nav(1, false),
            v if v == VK_SPACE.0 => self.nav(1, false),
            v if v == VK_BACK.0 => self.nav(-1, false),
            v if v == VK_HOME.0 => {
                if !self.lib.is_empty() {
                    self.index = Some(0);
                    self.nav(0, false);
                }
            }
            v if v == VK_END.0 => {
                if !self.lib.is_empty() {
                    let last = self.lib.len() - 1;
                    self.index = Some(last);
                    let p = self.lib[last].clone();
                    self.open_current(&p, true);
                }
            }
            v if v == VK_OEM_PLUS.0 || v == VK_ADD.0 || v == 0xbb => self.zoom_center(1.25),
            v if v == VK_OEM_MINUS.0 || v == VK_SUBTRACT.0 || v == 0xbd => {
                self.zoom_center(1.0 / 1.25)
            }
            v if v == b'0' as u16 => self.reset_fit(),
            v if v == b'1' as u16 => self.actual_size(),
            v if v == b'C' as u16 => self.toggle_crop(),
            v if v == b'R' as u16 => self.rotate(!shift),
            v if v == VK_DELETE.0 => self.ask_delete(),
            v if v == VK_F11.0 => self.toggle_fullscreen(),
            v if v == VK_F5.0 => self.toggle_slideshow(),
            v if v == VK_F1.0 || v == VK_OEM_2.0 => {
                self.help = true;
                self.invalidate();
            }
            _ => {}
        }
        self.update_timer();
    }

    fn on_mouse_move(&mut self, x: i32, y: i32) {
        self.mouse = (x, y);
        if !self.track_leave {
            self.track_leave = true;
            let mut tme = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: self.hwnd,
                dwHoverTime: 0,
            };
            unsafe {
                let _ = TrackMouseEvent(&mut tme);
            }
        }

        if self.confirm.is_some() {
            let h = if self.confirm_yes.contains(x, y) {
                1
            } else if self.confirm_no.contains(x, y) {
                2
            } else {
                0
            };
            if h != self.confirm_hover {
                self.confirm_hover = h;
                self.invalidate();
            }
            return;
        }

        if let Some(crop) = &mut self.crop {
            let drag = crop.drag;
            if drag != Drag::None {
                self.drag_crop(drag, x, y);
                return;
            }
            let hover_bar = self.layout.crop_button_at(x, y);
            if hover_bar != self.hover_crop {
                self.hover_crop = hover_bar;
                self.invalidate();
            }
            return;
        }

        let hover = if self.current.is_some() && self.error.is_none() {
            self.layout.button_at(x, y)
        } else {
            None
        };
        let hover_empty = self.current.is_none() && self.layout.empty_open.contains(x, y);
        let changed = hover != self.hover || hover_empty != self.hover_empty;
        if changed {
            self.hover = hover;
            self.hover_empty = hover_empty;
            self.hover_since = Some(Instant::now());
            self.invalidate();
        }
        if let Drag::Pan { .. } = self.drag {
            self.pan_to(x, y);
        }
    }

    fn on_mouse_down(&mut self, x: i32, y: i32) {
        if self.help {
            self.help = false;
            self.invalidate();
            return;
        }
        if self.confirm.is_some() {
            if self.confirm_yes.contains(x, y) {
                self.confirm_delete();
            } else if self.confirm_no.contains(x, y) {
                self.confirm = None;
                self.invalidate();
            }
            return;
        }
        if self.crop.is_some() {
            if let Some(b) = self.layout.crop_button_at(x, y) {
                self.crop_button(b);
                return;
            }
            self.start_crop_drag(x, y);
            return;
        }
        if self.current.is_none() {
            if self.layout.empty_open.contains(x, y) {
                self.open_dialog();
            }
            return;
        }
        if let Some(b) = self.layout.button_at(x, y) {
            self.pressed = Some(b);
            self.invalidate();
            return;
        }
        if self.layout.content.contains(x, y) {
            if let Some((iw, ih)) = self.dims() {
                let z = self.view.zoom;
                let need_pan = iw as f32 * z > self.layout.content.w as f32 + 0.5
                    || ih as f32 * z > self.layout.content.h as f32 + 0.5;
                if need_pan {
                    self.drag = Drag::Pan {
                        mx: x,
                        my: y,
                        cx: self.view.cx,
                        cy: self.view.cy,
                    };
                    unsafe {
                        SetCapture(self.hwnd);
                    }
                    self.apply_cursor();
                }
            }
        }
    }

    fn on_mouse_up(&mut self, x: i32, y: i32) {
        if let Some(b) = self.pressed.take() {
            if self.layout.button_at(x, y) == Some(b) {
                self.action(b);
            }
            self.invalidate();
            return;
        }
        if self.crop.is_some() {
            if let Some(crop) = &mut self.crop {
                crop.drag = Drag::None;
            }
            unsafe {
                let _ = ReleaseCapture();
            }
            self.invalidate();
            return;
        }
        if let Drag::Pan { .. } = self.drag {
            self.drag = Drag::None;
            unsafe {
                let _ = ReleaseCapture();
            }
            self.apply_cursor();
        }
    }

    fn on_double_click(&mut self, x: i32, y: i32) {
        if self.crop.is_some() || self.current.is_none() || self.confirm.is_some() {
            return;
        }
        if self.layout.content.contains(x, y) {
            if self.view.fit {
                self.actual_size();
            } else {
                self.reset_fit();
            }
        }
    }

    fn on_wheel(&mut self, wp: usize, lp: usize) {
        if self.crop.is_some() || self.current.is_none() {
            return;
        }
        let delta = ((wp >> 16) & 0xffff) as u16 as i16 as f32 / 120.0;
        if delta.abs() < 0.01 {
            return;
        }
        let mut pt = POINT {
            x: lo(lp) as i32,
            y: hi(lp) as i32,
        };
        unsafe {
            let _ = ScreenToClient(self.hwnd, &mut pt);
        }
        let factor = 1.18f32.powf(delta);
        self.zoom_at(pt.x, pt.y, factor);
    }

    fn on_drop(&mut self, hdrop: HDROP) {
        unsafe {
            let count = DragQueryFileW(hdrop, u32::MAX, None);
            if count > 0 {
                let mut buf = vec![0u16; 32768];
                let n = DragQueryFileW(hdrop, 0, Some(&mut buf));
                if n > 0 {
                    let s = String::from_utf16_lossy(&buf[..n as usize]);
                    let p = PathBuf::from(s.trim_matches('\0'));
                    DragFinish(hdrop);
                    self.open_path(p);
                    return;
                }
            }
            DragFinish(hdrop);
        }
    }

    // ---------------------------------------------------------------- actions

    fn action(&mut self, b: Btn) {
        match b {
            Btn::Open => self.open_dialog(),
            Btn::Prev => self.nav(-1, false),
            Btn::Next => self.nav(1, false),
            Btn::ZoomOut => self.zoom_center(1.0 / 1.25),
            Btn::ZoomIn => self.zoom_center(1.25),
            Btn::Fit => self.reset_fit(),
            Btn::Actual => self.actual_size(),
            Btn::RotateL => self.rotate(false),
            Btn::RotateR => self.rotate(true),
            Btn::Crop => self.toggle_crop(),
            Btn::Delete => self.ask_delete(),
            Btn::Fullscreen => self.toggle_fullscreen(),
        }
    }

    fn open_dialog(&mut self) {
        if let Some(p) = platform::open_dialog(self.hwnd, false) {
            self.open_path(p);
        }
    }

    fn toggle_fullscreen(&mut self) {
        self.fullscreen.toggle(self.hwnd);
        let (w, h) = platform::client_size(self.hwnd);
        self.on_size(w, h);
    }

    fn toggle_slideshow(&mut self) {
        self.slideshow = !self.slideshow;
        unsafe {
            if self.slideshow {
                SetTimer(Some(self.hwnd), WM_SLIDESHOW, SLIDE_MS, None);
                self.toast("Slideshow started  ·  F5 to stop");
            } else {
                let _ = KillTimer(Some(self.hwnd), WM_SLIDESHOW);
                self.toast("Slideshow stopped");
            }
        }
    }

    fn on_timer(&mut self, id: usize) {
        if id == WM_SLIDESHOW as usize {
            if self.slideshow {
                if self.crop.is_none() && self.confirm.is_none() && !self.help {
                    self.nav(1, true);
                }
            } else {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), WM_SLIDESHOW);
                }
            }
            return;
        }
        if id == WM_ANIM as usize {
            if !self.needs_anim() {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), WM_ANIM);
                }
                self.anim_on = false;
                return;
            }
            self.invalidate();
        }
    }

    fn needs_anim(&self) -> bool {
        let now = Instant::now();
        if self.loading {
            return true;
        }
        if let Some(t) = &self.toast {
            if now < t.until + Duration::from_millis(50) {
                return true;
            }
        }
        if let Some(t) = self.hover_since {
            if now.duration_since(t) < Duration::from_millis(800) {
                return true;
            }
        }
        false
    }

    fn update_timer(&mut self) {
        let need = self.needs_anim();
        unsafe {
            if need && !self.anim_on {
                SetTimer(Some(self.hwnd), WM_ANIM, 33, None);
                self.anim_on = true;
            } else if !need && self.anim_on {
                let _ = KillTimer(Some(self.hwnd), WM_ANIM);
                self.anim_on = false;
            }
        }
    }

    fn invalidate(&mut self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
            let _ = UpdateWindow(self.hwnd);
        }
    }

    fn update_title(&mut self) {
        let title = match &self.current {
            Some(p) => format!("{}  —  FastView", library::file_name(p)),
            None => "FastView".into(),
        };
        platform::set_title(self.hwnd, &title);
    }

    fn toast(&mut self, msg: impl Into<String>) {
        let now = Instant::now();
        self.toast = Some(Toast {
            text: msg.into(),
            start: now,
            until: now + Duration::from_millis(2400),
        });
        self.update_timer();
        self.invalidate();
    }

    fn zoom_center(&mut self, factor: f32) {
        let cx = self.layout.content.x + self.layout.content.w / 2;
        let cy = self.layout.content.y + self.layout.content.h / 2;
        self.zoom_at(cx, cy, factor);
    }

    fn zoom_at(&mut self, px: i32, py: i32, factor: f32) {
        if self.crop.is_some() {
            return;
        }
        let Some((iw, ih)) = self.dims() else { return };
        let content = self.layout.content;
        let cw = content.w as f32;
        let ch = content.h as f32;
        let fit = self.fit_zoom(iw, ih);
        let old = if self.view.fit { fit } else { self.view.zoom };
        let min = (fit * 0.5).max(0.02);
        let max = 12.0f32;
        let new = (old * factor).clamp(min, max);
        if (new - old).abs() < 0.0001 {
            return;
        }
        let left = self.view.cx - cw / (2.0 * old);
        let top = self.view.cy - ch / (2.0 * old);
        let ix = left + (px - content.x) as f32 / old;
        let iy = top + (py - content.y) as f32 / old;
        self.view.zoom = new;
        self.view.cx = ix + cw / (2.0 * new) - (px - content.x) as f32 / new;
        self.view.cy = iy + ch / (2.0 * new) - (py - content.y) as f32 / new;
        self.view.fit = (new - fit).abs() < 0.0008;
        if self.view.fit {
            self.view.cx = iw as f32 / 2.0;
            self.view.cy = ih as f32 / 2.0;
        }
        self.clamp_center(iw, ih);
        self.view_cache = None;
        if !self.view.fit {
            self.request_full();
        }
        self.invalidate();
    }

    fn clamp_center(&mut self, iw: u32, ih: u32) {
        let cw = self.layout.content.w as f32;
        let ch = self.layout.content.h as f32;
        let z = self.view.zoom.max(0.0001);
        let hw = cw / (2.0 * z);
        let hh = ch / (2.0 * z);
        if iw as f32 <= hw * 2.0 {
            self.view.cx = iw as f32 / 2.0;
        } else {
            self.view.cx = self.view.cx.clamp(hw, iw as f32 - hw);
        }
        if ih as f32 <= hh * 2.0 {
            self.view.cy = ih as f32 / 2.0;
        } else {
            self.view.cy = self.view.cy.clamp(hh, ih as f32 - hh);
        }
    }

    fn view_origin(&self, iw: u32, ih: u32) -> (f32, f32) {
        if self.view.fit {
            let z = self.fit_zoom(iw, ih);
            (
                iw as f32 / 2.0 - self.layout.content.w as f32 / (2.0 * z),
                ih as f32 / 2.0 - self.layout.content.h as f32 / (2.0 * z),
            )
        } else {
            (
                self.view.cx - self.layout.content.w as f32 / (2.0 * self.view.zoom),
                self.view.cy - self.layout.content.h as f32 / (2.0 * self.view.zoom),
            )
        }
    }

    /// The zoom actually used for drawing, including fit mode.
    fn zoom_now(&self, iw: u32, ih: u32) -> f32 {
        if self.view.fit {
            self.fit_zoom(iw, ih)
        } else {
            self.view.zoom
        }
    }

    fn pan_to(&mut self, x: i32, y: i32) {
        if let Drag::Pan { mx, my, cx, cy } = self.drag {
            let z = self.view.zoom.max(0.0001);
            self.view.cx = cx - (x - mx) as f32 / z;
            self.view.cy = cy - (y - my) as f32 / z;
            if let Some((iw, ih)) = self.dims() {
                self.clamp_center(iw, ih);
            }
            self.view_cache = None;
            self.invalidate();
        }
    }

    fn apply_cursor(&self) {
        let c = if self.crop.is_some() {
            let idx = self.crop_handle_at(self.mouse.0, self.mouse.1);
            match idx {
                Some(0) | Some(4) => self.size_nwse,
                Some(2) | Some(6) => self.size_nesw,
                Some(1) | Some(5) => self.size_ns,
                Some(3) | Some(7) => self.size_we,
                _ => self.cross,
            }
        } else if self.current.is_none() {
            if self.layout.empty_open.contains(self.mouse.0, self.mouse.1) {
                self.hand
            } else {
                self.arrow
            }
        } else if self.layout.button_at(self.mouse.0, self.mouse.1).is_some() {
            self.hand
        } else if let Some((iw, ih)) = self.dims() {
            let z = self.zoom_now(iw, ih);
            if (iw as f32 * z > self.layout.content.w as f32 + 0.5
                || ih as f32 * z > self.layout.content.h as f32 + 0.5)
                && self.layout.content.contains(self.mouse.0, self.mouse.1)
            {
                self.hand
            } else {
                self.arrow
            }
        } else {
            self.arrow
        };
        unsafe {
            SetCursor(Some(c));
        }
    }

    // ------------------------------------------------------------------- crop

    fn toggle_crop(&mut self) {
        if self.crop.is_some() {
            self.crop = None;
            self.invalidate();
            return;
        }
        let Some((iw, ih)) = self.dims() else { return };
        let has_full = self
            .current
            .as_ref()
            .and_then(|p| self.cache.get(p))
            .and_then(|e| e.full.as_ref())
            .is_some();
        if !has_full {
            self.toast("Still loading this photo…");
            return;
        }
        let w = iw as f32 * 0.82;
        let h = ih as f32 * 0.82;
        // Crop is easiest (and safest) with the whole picture visible.
        self.view.fit = true;
        self.view.zoom = self.fit_zoom(iw, ih);
        self.view.cx = iw as f32 / 2.0;
        self.view.cy = ih as f32 / 2.0;
        self.view_cache = None;
        self.ensure_fit();
        self.crop = Some(CropState {
            rect: [(iw as f32 - w) / 2.0, (ih as f32 - h) / 2.0, w, h],
            aspect: 0,
            drag: Drag::None,
        });
        self.hover_crop = None;
        self.slideshow = false;
        unsafe {
            let _ = KillTimer(Some(self.hwnd), WM_SLIDESHOW);
        }
        self.invalidate();
    }

    fn crop_button(&mut self, b: CropBtn) {
        match b {
            CropBtn::Apply => self.apply_crop(),
            CropBtn::Cancel => {
                self.crop = None;
                self.invalidate();
            }
            _ => {
                let aspect = match b {
                    CropBtn::Free => 0,
                    CropBtn::Square => 1,
                    CropBtn::FourThree => 2,
                    CropBtn::SixteenNine => 3,
                    _ => 0,
                };
                let dims = self.dims();
                if let Some(c) = &mut self.crop {
                    c.aspect = aspect;
                    if aspect > 0 {
                        if let Some((iw, ih)) = dims {
                            fit_aspect(c, aspect, iw, ih);
                        }
                    }
                }
                self.invalidate();
            }
        }
    }

    fn crop_screen_rect(&self) -> Option<(Rect, f32, f32)> {
        let crop = self.crop.as_ref()?;
        let (iw, ih) = self.dims()?;
        let (left, top) = self.view_origin(iw, ih);
        let z = self.zoom_now(iw, ih);
        let content = self.layout.content;
        let x = content.x as f32 + (crop.rect[0] - left) * z;
        let y = content.y as f32 + (crop.rect[1] - top) * z;
        let w = crop.rect[2] * z;
        let h = crop.rect[3] * z;
        Some((
            Rect::new(x.round() as i32, y.round() as i32, w.round() as i32, h.round() as i32),
            left,
            top,
        ))
    }

    fn crop_handle_at(&self, x: i32, y: i32) -> Option<usize> {
        if self.crop.is_none() {
            return None;
        }
        let (rect, _, _) = self.crop_screen_rect()?;
        let hs = self.layout.px(9.0);
        let pts = ui::handle_points(rect, hs);
        for (i, (px, py)) in pts.iter().enumerate() {
            let hr = Rect::new(*px - 4, *py - 4, hs + 8, hs + 8);
            if hr.contains(x, y) {
                return Some(i);
            }
        }
        None
    }

    fn start_crop_drag(&mut self, x: i32, y: i32) {
        let Some((iw, ih)) = self.dims() else { return };
        crate::timing::mark(&format!("crop down {x},{y} fit={} zoom={:.3} rect={:?}", self.view.fit, self.view.zoom, self.crop.as_ref().map(|c| c.rect)));
        if !self.layout.content.contains(x, y) {
            return;
        }
        if let Some(idx) = self.crop_handle_at(x, y) {
            let (mx, my) = self.screen_to_image(x as f32, y as f32, iw, ih);
            if let Some(crop) = &mut self.crop {
                crop.drag = Drag::Handle {
                    idx: idx as u8,
                    x: mx,
                    y: my,
                    rect: crop.rect,
                };
            }
            unsafe {
                SetCapture(self.hwnd);
            }
            self.apply_cursor();
            return;
        }
        let (mx, my) = self.screen_to_image(x as f32, y as f32, iw, ih);
        let inside = self
            .crop_screen_rect()
            .map(|(r, _, _)| r.contains(x, y))
            .unwrap_or(false);
        if let Some(crop) = &mut self.crop {
            if inside {
                crop.drag = Drag::Move {
                    x: mx,
                    y: my,
                    rect: crop.rect,
                };
            } else {
                crop.drag = Drag::New { x: mx, y: my };
                crop.rect = [mx, my, 1.0, 1.0];
            }
        }
        unsafe {
            SetCapture(self.hwnd);
        }
        self.apply_cursor();
        self.invalidate();
    }

    fn drag_crop(&mut self, drag: Drag, x: i32, y: i32) {
        let Some((iw, ih)) = self.dims() else { return };
        let (mx, my) = self.screen_to_image(x as f32, y as f32, iw, ih);
        crate::timing::mark(&format!("crop move {x},{y} -> {mx:.0},{my:.0}"));
        let Some(crop) = &mut self.crop else { return };
        let iw = iw as f32;
        let ih = ih as f32;
        match drag {
            Drag::New { x: ax, y: ay } => {
                let x0 = ax.min(mx).max(0.0);
                let y0 = ay.min(my).max(0.0);
                let x1 = ax.max(mx).min(iw);
                let y1 = ay.max(my).min(ih);
                crop.rect = [x0, y0, (x1 - x0).max(1.0), (y1 - y0).max(1.0)];
                if crop.aspect > 0 {
                    fit_aspect(crop, crop.aspect, iw as u32, ih as u32);
                }
            }
            Drag::Move { x: ox, y: oy, rect } => {
                let dx = mx - ox;
                let dy = my - oy;
                let nx = (rect[0] + dx).clamp(0.0, (iw - rect[2]).max(0.0));
                let ny = (rect[1] + dy).clamp(0.0, (ih - rect[3]).max(0.0));
                crop.rect[0] = nx;
                crop.rect[1] = ny;
            }
            Drag::Handle { idx, x: ox, y: oy, rect } => {
                let dx = mx - ox;
                let dy = my - oy;
                let (mut x0, mut y0, mut x1, mut y1) =
                    (rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]);
                match idx {
                    0 => {
                        x0 += dx;
                        y0 += dy;
                    }
                    1 => y0 += dy,
                    2 => {
                        x1 += dx;
                        y0 += dy;
                    }
                    3 => x1 += dx,
                    4 => {
                        x1 += dx;
                        y1 += dy;
                    }
                    5 => y1 += dy,
                    6 => {
                        x0 += dx;
                        y1 += dy;
                    }
                    7 => x0 += dx,
                    _ => {}
                }
                x0 = x0.clamp(0.0, iw - 1.0);
                y0 = y0.clamp(0.0, ih - 1.0);
                x1 = x1.clamp(x0 + 1.0, iw);
                y1 = y1.clamp(y0 + 1.0, ih);
                crop.rect = [x0, y0, x1 - x0, y1 - y0];
                if crop.aspect > 0 {
                    fit_aspect(crop, crop.aspect, iw as u32, ih as u32);
                }
            }
            _ => {}
        }
        self.invalidate();
    }

    fn screen_to_image(&self, px: f32, py: f32, iw: u32, ih: u32) -> (f32, f32) {
        let (left, top) = self.view_origin(iw, ih);
        let z = self.zoom_now(iw, ih).max(0.0001);
        let x = left + (px - self.layout.content.x as f32) / z;
        let y = top + (py - self.layout.content.y as f32) / z;
        (x.clamp(0.0, iw as f32), y.clamp(0.0, ih as f32))
    }

    fn apply_crop(&mut self) {
        let Some(crop) = self.crop.clone() else { return };
        let Some(path) = self.current.clone() else { return };
        let Some(full) = self.cache.get(&path).and_then(|e| e.full.clone()) else {
            self.toast("Still loading this photo…");
            return;
        };
        let x = crop.rect[0].round().max(0.0) as u32;
        let y = crop.rect[1].round().max(0.0) as u32;
        let w = crop.rect[2].round().max(1.0) as u32;
        let h = crop.rect[3].round().max(1.0) as u32;
        if w < 2 || h < 2 {
            self.toast("Select a larger area");
            return;
        }
        let Some(mut cut) = imaging::crop(&full, x, y, w, h) else {
            return;
        };
        // Save asynchronously-ish: draw a message first, then encode.
        self.toast("Saving crop…");
        unsafe {
            let _ = SetCursor(Some(platform::load_cursor(IDC_WAIT)));
        }
        match imaging::save_edited(&cut, "_cropped") {
            Ok(new_path) => {
                cut.path = new_path.clone();
                cut.file_size = std::fs::metadata(&new_path).map(|m| m.len()).unwrap_or(0);
                cut.format = imaging::format_label(&new_path);
                self.adopt_edited(cut, &new_path);
                self.crop = None;
                self.toast(format!("Saved {}", library::file_name(&new_path)));
            }
            Err(e) => self.toast(e),
        }
        self.apply_cursor();
        self.invalidate();
    }

    fn adopt_edited(&mut self, photo: Photo, path: &Path) {
        let meta = loader::Meta {
            w: photo.w,
            h: photo.h,
            format: photo.format,
            file_size: photo.file_size,
        };
        let arc = Arc::new(photo);
        for (p, e) in self.cache.iter_mut() {
            if p != path {
                e.full = None;
            }
        }
        self.cache.insert(
            path.to_path_buf(),
            Entry {
                meta: Some(meta),
                fit: None,
                fit_size: (0, 0),
                full: Some(arc),
            },
        );
        if !self.lib.iter().any(|p| Self::same_path(p, path)) {
            self.lib.push(path.to_path_buf());
            self.lib.sort_by(|a, b| {
                library::natural_cmp(
                    &library::file_name(a).to_lowercase(),
                    &library::file_name(b).to_lowercase(),
                )
            });
        }
        self.index = self.lib.iter().position(|p| Self::same_path(p, path));
        self.current = Some(path.to_path_buf());
        self.error = None;
        self.loading = false;
        self.fit_pending = false;
        self.full_pending = false;
        self.seq = self.seq.wrapping_add(1);
        self.view = View::new();
        if let Some((iw, ih)) = self.dims() {
            self.view.initialized = true;
            self.view.zoom = self.fit_zoom(iw, ih);
            self.view.cx = iw as f32 / 2.0;
            self.view.cy = ih as f32 / 2.0;
        }
        self.view_cache = None;
        self.ensure_fit();
        self.update_title();
    }

    fn rotate(&mut self, clockwise: bool) {
        let Some(path) = self.current.clone() else { return };
        let Some(full) = self.cache.get(&path).and_then(|e| e.full.clone()) else {
            self.toast("Still loading this photo…");
            return;
        };
        unsafe {
            let _ = SetCursor(Some(platform::load_cursor(IDC_WAIT)));
        }
        self.toast("Saving rotated copy…");
        let mut rotated = imaging::rotate(&full, clockwise);
        match imaging::save_edited(&rotated, "_rotated") {
            Ok(new_path) => {
                rotated.path = new_path.clone();
                rotated.file_size = std::fs::metadata(&new_path).map(|m| m.len()).unwrap_or(0);
                rotated.format = imaging::format_label(&new_path);
                self.adopt_edited(rotated, &new_path);
                self.toast(format!("Saved {}", library::file_name(&new_path)));
            }
            Err(e) => self.toast(e),
        }
        self.apply_cursor();
        self.invalidate();
    }

    fn ask_delete(&mut self) {
        let Some(path) = self.current.clone() else { return };
        let name = library::file_name(&path);
        self.confirm = Some(Confirm {
            path,
            title: "Delete this photo?".into(),
            body: format!("“{name}” will be moved to the Recycle Bin. You can restore it from there."),
        });
        self.confirm_hover = 0;
        self.invalidate();
    }

    fn confirm_delete(&mut self) {
        let Some(c) = self.confirm.take() else { return };
        if platform::recycle(&c.path) {
            self.cache.remove(&c.path);
            let removed_idx = self.lib.iter().position(|p| Self::same_path(p, &c.path));
            self.lib.retain(|p| !Self::same_path(p, &c.path));
            if self.lib.is_empty() {
                self.index = None;
                self.current = None;
                self.view = View::new();
                self.view_cache = None;
                self.update_title();
                self.toast("Moved to Recycle Bin");
            } else {
                let idx = removed_idx.unwrap_or(0).min(self.lib.len() - 1);
                self.index = Some(idx);
                let p = self.lib[idx].clone();
                self.open_current(&p, true);
                self.toast("Moved to Recycle Bin");
            }
        } else {
            self.toast("Could not move the file to the Recycle Bin");
        }
        self.invalidate();
    }

    fn copy_image(&mut self) {
        let Some(path) = self.current.clone() else { return };
        let full = self.cache.get(&path).and_then(|e| e.full.clone());
        if let Some(p) = full {
            if platform::copy_to_clipboard(self.hwnd, p.w, p.h, &p.px) {
                self.toast("Copied to clipboard");
            } else {
                self.toast("Could not copy the photo");
            }
        } else {
            self.toast("Still loading this photo…");
        }
    }
}

fn nudge(crop: &mut CropState, dx: f32, dy: f32) {
    crop.rect[0] += dx;
    crop.rect[1] += dy;
    crop.rect[0] = crop.rect[0].max(0.0);
    crop.rect[1] = crop.rect[1].max(0.0);
}

fn fit_aspect(crop: &mut CropState, aspect: u8, iw: u32, ih: u32) {
    let ratio = match aspect {
        1 => 1.0,
        2 => 4.0 / 3.0,
        3 => 16.0 / 9.0,
        _ => return,
    };
    let rect = crop.rect;
    let cx = rect[0] + rect[2] / 2.0;
    let cy = rect[1] + rect[3] / 2.0;
    let mut w = rect[2].max(rect[3] * ratio).min(iw as f32).min(ih as f32 * ratio);
    w = w.max(1.0);
    let mut h = w / ratio;
    if h > ih as f32 {
        h = ih as f32;
        w = h * ratio;
    }
    let x = (cx - w / 2.0).clamp(0.0, (iw as f32 - w).max(0.0));
    let y = (cy - h / 2.0).clamp(0.0, (ih as f32 - h).max(0.0));
    crop.rect = [x, y, w, h];
}

fn lo(v: usize) -> i32 {
    (v & 0xffff) as u16 as i16 as i32
}

fn hi(v: usize) -> i32 {
    ((v >> 16) & 0xffff) as u16 as i16 as i32
}










