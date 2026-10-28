// FastView - a tiny software canvas.
//
// The whole window is rendered into one 32-bit DIB (top-down, BGRA). The photo is
// resized with SIMD code (fast_image_resize) and copied in; the chrome is drawn
// with the small anti-aliased primitives below; text is drawn with GDI into the
// same DIB so it stays crisp. Finally one BitBlt puts the frame on screen.

use std::ptr;
use std::slice;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, RECT, SIZE};
use windows::Win32::Graphics::Gdi::*;


use crate::ui::Rect;

pub const fn rgb(r: u32, g: u32, b: u32) -> u32 {
    (r << 16) | (g << 8) | b
}

fn colorref(c: u32) -> COLORREF {
    COLORREF((c & 0xff) << 16 | (c & 0xff00) | ((c >> 16) & 0xff))
}

#[inline]
fn blend(dst: u32, src: u32, a: u32) -> u32 {
    if a == 0 {
        return dst;
    }
    if a >= 255 {
        return src;
    }
    let inv = 255 - a;
    let r = (((dst >> 16) & 0xff) * inv + ((src >> 16) & 0xff) * a) >> 8;
    let g = (((dst >> 8) & 0xff) * inv + ((src >> 8) & 0xff) * a) >> 8;
    let b = ((dst & 0xff) * inv + (src & 0xff) * a) >> 8;
    (dst & 0xff00_0000) | (r << 16) | (g << 8) | b
}

pub struct Canvas {
    pub w: i32,
    pub h: i32,
    mem: HDC,
    dib: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u32,
}

impl Canvas {
    pub fn new(hwnd: windows::Win32::Foundation::HWND, w: i32, h: i32) -> Option<Self> {
        unsafe {
            let hdc_screen = GetDC(Some(hwnd));
            let mem = CreateCompatibleDC(Some(hdc_screen));
            let _ = ReleaseDC(Some(hwnd), hdc_screen);
            if mem.is_invalid() {
                return None;
            }
            let mut c = Self {
                w: 0,
                h: 0,
                mem,
                dib: HBITMAP::default(),
                old: HGDIOBJ::default(),
                bits: ptr::null_mut(),
            };
            if !c.resize(w, h) {
                let _ = DeleteDC(mem);
                return None;
            }
            Some(c)
        }
    }

    pub fn resize(&mut self, w: i32, h: i32) -> bool {
        let w = w.max(1);
        let h = h.max(1);
        if w == self.w && h == self.h && !self.bits.is_null() {
            return true;
        }
        unsafe {
            if !self.dib.is_invalid() {
                let _ = SelectObject(self.mem, self.old);
                let _ = DeleteObject(self.dib.into());
                self.dib = HBITMAP::default();
                self.bits = ptr::null_mut();
            }
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut core::ffi::c_void = ptr::null_mut();
            let dib = CreateDIBSection(Some(self.mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
            let Ok(dib) = dib else { return false };
            if dib.is_invalid() || bits.is_null() {
                return false;
            }
            self.old = SelectObject(self.mem, dib.into());
            self.dib = dib;
            self.bits = bits as *mut u32;
            self.w = w;
            self.h = h;
            true
        }
    }

    pub fn hdc(&self) -> HDC {
        self.mem
    }

    #[inline]
    pub fn pix(&mut self) -> &mut [u32] {
        unsafe { slice::from_raw_parts_mut(self.bits, (self.w * self.h) as usize) }
    }

    pub fn clear(&mut self, color: u32) {
        self.pix().fill(color | 0xff00_0000);
    }

    pub fn present(&self, hdc: HDC) {
        unsafe {
            let _ = BitBlt(hdc, 0, 0, self.w, self.h, Some(self.mem), 0, 0, SRCCOPY);
        }
    }

    pub fn fill(&mut self, r: Rect, color: u32) {
        let (x0, y0, x1, y1) = clip(r, self.w, self.h);
        if x1 <= x0 {
            return;
        }
        let c = color | 0xff00_0000;
        let stride = self.w as usize;
        let px = self.pix();
        for y in y0..y1 {
            let row = &mut px[y as usize * stride + x0 as usize..y as usize * stride + x1 as usize];
            row.fill(c);
        }
    }

    pub fn fill_a(&mut self, r: Rect, color: u32, alpha: f32) {
        let a = (alpha.clamp(0.0, 1.0) * 255.0) as u32;
        if a == 0 {
            return;
        }
        let (x0, y0, x1, y1) = clip(r, self.w, self.h);
        if x1 <= x0 {
            return;
        }
        let c = color | 0xff00_0000;
        let stride = self.w as usize;
        let px = self.pix();
        for y in y0..y1 {
            for x in x0..x1 {
                let p = &mut px[y as usize * stride + x as usize];
                *p = blend(*p, c, a);
            }
        }
    }

    pub fn vgrad(&mut self, r: Rect, top_alpha: f32, bottom_alpha: f32, color: u32) {
        let (_, y0, _, y1) = clip(r, self.w, self.h);
        if y1 <= y0 {
            return;
        }
        let n = (y1 - y0).max(1) as f32;
        for y in y0..y1 {
            let t = (y - y0) as f32 / n;
            let a = top_alpha + (bottom_alpha - top_alpha) * t;
            self.fill_a(Rect::new(r.x, y, r.w, 1), color, a);
        }
    }

    /// Anti-aliased rounded rectangle fill.
    pub fn rrect(&mut self, r: Rect, radius: i32, color: u32, alpha: f32) {
        let (x0, y0, x1, y1) = clip(r, self.w, self.h);
        if x1 <= x0 || alpha <= 0.0 {
            return;
        }
        let rad = radius.max(0) as f32;
        let cx = r.x as f32 + r.w as f32 / 2.0;
        let cy = r.y as f32 + r.h as f32 / 2.0;
        let hw = r.w as f32 / 2.0;
        let hh = r.h as f32 / 2.0;
        let stride = self.w as usize;
        let px = self.pix();
        for y in y0..y1 {
            let fy = y as f32 + 0.5;
            for x in x0..x1 {
                let fx = x as f32 + 0.5;
                let cov = rrect_cov(fx, fy, cx, cy, hw, hh, rad);
                if cov > 0.0 {
                    let a = (alpha * cov * 255.0) as u32;
                    let p = &mut px[y as usize * stride + x as usize];
                    *p = blend(*p, color | 0xff00_0000, a);
                }
            }
        }
    }

    pub fn rrect_stroke(&mut self, r: Rect, radius: i32, thickness: f32, color: u32, alpha: f32) {
        let (x0, y0, x1, y1) = clip(Rect::new(r.x - 1, r.y - 1, r.w + 2, r.h + 2), self.w, self.h);
        if x1 <= x0 || alpha <= 0.0 {
            return;
        }
        let rad = radius.max(0) as f32;
        let cx = r.x as f32 + r.w as f32 / 2.0;
        let cy = r.y as f32 + r.h as f32 / 2.0;
        let hw = r.w as f32 / 2.0;
        let hh = r.h as f32 / 2.0;
        let stride = self.w as usize;
        let px = self.pix();
        for y in y0..y1 {
            let fy = y as f32 + 0.5;
            for x in x0..x1 {
                let fx = x as f32 + 0.5;
                let d = rrect_dist(fx, fy, cx, cy, hw, hh, rad);
                let cov = (thickness / 2.0 + 0.5 - d.abs()).clamp(0.0, 1.0);
                if cov > 0.0 {
                    let a = (alpha * cov * 255.0) as u32;
                    let p = &mut px[y as usize * stride + x as usize];
                    *p = blend(*p, color | 0xff00_0000, a);
                }
            }
        }
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, color: u32, alpha: f32) {
        let rr = Rect::new(
            (cx - r) as i32,
            (cy - r) as i32,
            (r * 2.0) as i32 + 2,
            (r * 2.0) as i32 + 2,
        );
        let (x0, y0, x1, y1) = clip(rr, self.w, self.h);
        let stride = self.w as usize;
        let px = self.pix();
        for y in y0..y1 {
            let fy = y as f32 + 0.5 - cy;
            for x in x0..x1 {
                let fx = x as f32 + 0.5 - cx;
                let d = (fx * fx + fy * fy).sqrt();
                let cov = (r + 0.5 - d).clamp(0.0, 1.0);
                if cov > 0.0 {
                    let a = (alpha * cov * 255.0) as u32;
                    let p = &mut px[y as usize * stride + x as usize];
                    *p = blend(*p, color | 0xff00_0000, a);
                }
            }
        }
    }

    pub fn arc(
        &mut self,
        cx: f32,
        cy: f32,
        r: f32,
        thickness: f32,
        start_deg: f32,
        end_deg: f32,
        color: u32,
        alpha: f32,
    ) {
        let pad = thickness + 1.0;
        let rr = Rect::new(
            (cx - r - pad) as i32,
            (cy - r - pad) as i32,
            ((r + pad) * 2.0) as i32,
            ((r + pad) * 2.0) as i32,
        );
        let (x0, y0, x1, y1) = clip(rr, self.w, self.h);
        let stride = self.w as usize;
        let px = self.pix();
        let mut span = end_deg - start_deg;
        while span < 0.0 {
            span += 360.0;
        }
        for y in y0..y1 {
            let fy = y as f32 + 0.5 - cy;
            for x in x0..x1 {
                let fx = x as f32 + 0.5 - cx;
                let d = (fx * fx + fy * fy).sqrt();
                let dcov = (thickness / 2.0 + 0.5 - (d - r).abs()).clamp(0.0, 1.0);
                if dcov <= 0.0 {
                    continue;
                }
                let mut ang = fy.atan2(fx).to_degrees();
                if ang < 0.0 {
                    ang += 360.0;
                }
                let mut rel = ang - start_deg;
                while rel < 0.0 {
                    rel += 360.0;
                }
                // Soft endpoints.
                let edge = (rel.min(span - rel) + 0.5).clamp(0.0, 1.0);
                let cov = dcov * edge;
                if cov > 0.0 {
                    let a = (alpha * cov * 255.0) as u32;
                    let p = &mut px[y as usize * stride + x as usize];
                    *p = blend(*p, color | 0xff00_0000, a);
                }
            }
        }
    }

    pub fn triangle(&mut self, a: (f32, f32), b: (f32, f32), c: (f32, f32), color: u32, alpha: f32) {
        let minx = a.0.min(b.0).min(c.0).floor() as i32 - 1;
        let maxx = a.0.max(b.0).max(c.0).ceil() as i32 + 1;
        let miny = a.1.min(b.1).min(c.1).floor() as i32 - 1;
        let maxy = a.1.max(b.1).max(c.1).ceil() as i32 + 1;
        let (x0, y0, x1, y1) = clip(
            Rect::new(minx, miny, maxx - minx, maxy - miny),
            self.w,
            self.h,
        );
        let edge = |p: (f32, f32), q: (f32, f32), x: f32, y: f32| -> f32 {
            (q.0 - p.0) * (y - p.1) - (q.1 - p.1) * (x - p.0)
        };
        let stride = self.w as usize;
        let px = self.pix();
        let samples = 4;
        let inv = 1.0 / (samples * samples) as f32;
        for y in y0..y1 {
            for x in x0..x1 {
                let mut hits = 0u32;
                for sy in 0..samples {
                    for sx in 0..samples {
                        let fx = x as f32 + (sx as f32 + 0.5) / samples as f32;
                        let fy = y as f32 + (sy as f32 + 0.5) / samples as f32;
                        let w0 = edge(b, c, fx, fy);
                        let w1 = edge(c, a, fx, fy);
                        let w2 = edge(a, b, fx, fy);
                        if (w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0)
                            || (w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0)
                        {
                            hits += 1;
                        }
                    }
                }
                if hits > 0 {
                    let cov = hits as f32 * inv;
                    let al = (alpha * cov * 255.0) as u32;
                    let p = &mut px[y as usize * stride + x as usize];
                    *p = blend(*p, color | 0xff00_0000, al);
                }
            }
        }
    }


    /// Copy a sub-rectangle of an opaque image with clipping.
    pub fn blit_sub(
        &mut self,
        dx: i32,
        dy: i32,
        src: &[u32],
        sw: i32,
        sh: i32,
        sx: i32,
        sy: i32,
        w: i32,
        h: i32,
    ) {
        let (x0, y0, x1, y1) = clip(Rect::new(dx, dy, w, h), self.w, self.h);
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let sstride = sw as usize;
        let dstride = self.w as usize;
        let px = self.pix();
        for y in y0..y1 {
            let syi = sy + (y - dy);
            if syi < 0 || syi >= sh {
                continue;
            }
            let sxi = sx + (x0 - dx);
            let n = (x1 - x0) as usize;
            if sxi < 0 || sxi + n as i32 > sw {
                continue;
            }
            let src_row = &src[syi as usize * sstride + sxi as usize..syi as usize * sstride + sxi as usize + n];
            let dst_row = &mut px[y as usize * dstride + x0 as usize..y as usize * dstride + x0 as usize + n];
            dst_row.copy_from_slice(src_row);
            for p in dst_row.iter_mut() {
                *p |= 0xff00_0000;
            }
        }
    }
    //
    /// Copy an opaque image (BGRA u32) with clipping. No scaling.
    pub fn blit(&mut self, dx: i32, dy: i32, src: &[u32], sw: i32, sh: i32) {
        if sw <= 0 || sh <= 0 {
            return;
        }
        let (x0, y0, x1, y1) = clip(Rect::new(dx, dy, sw, sh), self.w, self.h);
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let sstride = sw as usize;
        let dstride = self.w as usize;
        let px = self.pix();
        for y in y0..y1 {
            let sy = (y - dy) as usize;
            let sx = (x0 - dx) as usize;
            let n = (x1 - x0) as usize;
            let src_row = &src[sy * sstride + sx..sy * sstride + sx + n];
            let dst_row = &mut px[y as usize * dstride + x0 as usize..y as usize * dstride + x1 as usize];
            dst_row.copy_from_slice(src_row);
            for p in dst_row.iter_mut() {
                *p |= 0xff00_0000;
            }
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            if !self.dib.is_invalid() {
                let _ = SelectObject(self.mem, self.old);
                let _ = DeleteObject(self.dib.into());
            }
            let _ = DeleteDC(self.mem);
        }
    }
}

fn clip(r: Rect, w: i32, h: i32) -> (i32, i32, i32, i32) {
    let x0 = r.x.max(0);
    let y0 = r.y.max(0);
    let x1 = (r.x + r.w).min(w);
    let y1 = (r.y + r.h).min(h);
    (x0, y0, x1, y1)
}

#[inline]
fn rrect_dist(fx: f32, fy: f32, cx: f32, cy: f32, hw: f32, hh: f32, rad: f32) -> f32 {
    let qx = (fx - cx).abs() - (hw - rad);
    let qy = (fy - cy).abs() - (hh - rad);
    let ax = qx.max(0.0);
    let ay = qy.max(0.0);
    (ax * ax + ay * ay).sqrt() + qx.max(qy).min(0.0) - rad
}

#[inline]
fn rrect_cov(fx: f32, fy: f32, cx: f32, cy: f32, hw: f32, hh: f32, rad: f32) -> f32 {
    let d = rrect_dist(fx, fy, cx, cy, hw, hh, rad);
    (0.5 - d).clamp(0.0, 1.0)
}

pub struct Fonts {
    pub ui_11: HFONT,
    pub ui_12: HFONT,
    pub ui_13: HFONT,
    pub ui_13b: HFONT,
    pub ui_15: HFONT,
    pub icon_16: HFONT,
    pub icon_18: HFONT,
}

impl Fonts {
    pub fn new(dpi: u32) -> Self {
        let s = dpi as f32 / 96.0;
        let mk = |px: f32, weight: i32, face: PCWSTR| unsafe {
            CreateFontW(
                -(px * s).round() as i32,
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_TT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
                face,
            )
        };
        Self {
            ui_11: mk(11.5, 400, w!("Segoe UI")),
            ui_12: mk(12.5, 400, w!("Segoe UI")),
            ui_13: mk(13.5, 400, w!("Segoe UI")),
            ui_13b: mk(13.5, 600, w!("Segoe UI Semibold")),
            ui_15: mk(15.0, 600, w!("Segoe UI Semibold")),
            icon_16: mk(16.0, 400, w!("Segoe Fluent Icons")),
            icon_18: mk(18.0, 400, w!("Segoe Fluent Icons")),
        }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        unsafe {
            for f in [
                self.ui_11,
                self.ui_12,
                self.ui_13,
                self.ui_13b,
                self.ui_15,
                self.icon_16,
                self.icon_18,
            ] {
                if !f.is_invalid() {
                    let _ = DeleteObject(f.into());
                }
            }
        }
    }
}

pub fn measure(hdc: HDC, text: &str, font: HFONT) -> (i32, i32) {
    unsafe {
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        if wide.is_empty() {
            return (0, 0);
        }
        let old = SelectObject(hdc, font.into());
        let mut size = SIZE::default();
        let _ = GetTextExtentPoint32W(hdc, &wide, &mut size);
        let _ = SelectObject(hdc, old);
        (size.cx, size.cy)
    }
}

pub fn text(
    hdc: HDC,
    text: &str,
    r: Rect,
    font: HFONT,
    color: u32,
    flags: u32,
) {
    if text.is_empty() {
        return;
    }
    unsafe {
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        let old = SelectObject(hdc, font.into());
        let _ = SetBkMode(hdc, TRANSPARENT);
        let _ = SetTextColor(hdc, colorref(color));
        let mut rc = RECT {
            left: r.x,
            top: r.y,
            right: r.x + r.w,
            bottom: r.y + r.h,
        };
        let fmt = DRAW_TEXT_FORMAT(DT_SINGLELINE.0 | DT_VCENTER.0 | DT_NOPREFIX.0 | flags);
        let _ = DrawTextW(hdc, &mut wide, &mut rc, fmt);
        let _ = SelectObject(hdc, old);
    }
}

pub fn text_block(
    hdc: HDC,
    text: &str,
    r: Rect,
    font: HFONT,
    color: u32,
) {
    unsafe {
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        let old = SelectObject(hdc, font.into());
        let _ = SetBkMode(hdc, TRANSPARENT);
        let _ = SetTextColor(hdc, colorref(color));
        let mut rc = RECT {
            left: r.x,
            top: r.y,
            right: r.x + r.w,
            bottom: r.y + r.h,
        };
        let fmt = DRAW_TEXT_FORMAT(DT_WORDBREAK.0 | DT_TOP.0 | DT_NOPREFIX.0);
        let _ = DrawTextW(hdc, &mut wide, &mut rc, fmt);
        let _ = SelectObject(hdc, old);
    }
}


