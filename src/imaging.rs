// FastView - decoding, resizing and saving pictures.
//
// Decoding goes through Windows Imaging Component (WIC): it is already on every
// Windows install, is heavily optimised, knows about EXIF orientation and covers
// JPEG/PNG/BMP/GIF/TIFF/WebP (plus HEIC/AVIF when their codec is installed).
// Resizing uses fast_image_resize (SIMD + rayon) because WIC's generic scaler is
// slow on low-power CPUs.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::slice;

use fast_image_resize as fir;
use windows::core::{w, Interface, PCWSTR, Result as WResult};
use windows::Win32::Foundation::GENERIC_READ;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Com::StructuredStorage::{
    PropVariantClear, PropVariantToUInt32, PROPVARIANT,
};

use crate::platform;

#[derive(Clone)]
pub struct Photo {
    pub path: PathBuf,
    pub w: u32,
    pub h: u32,
    pub px: Vec<u32>,
    pub format: &'static str,
    pub file_size: u64,
}

#[derive(Clone)]
pub struct Fit {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u32>,
}

pub struct Wic {
    factory: IWICImagingFactory,
}

impl Wic {
    pub fn new() -> WResult<Self> {
        unsafe {
            let factory: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory2, None, CLSCTX_INPROC_SERVER)?;
            Ok(Self { factory })
        }
    }

    pub fn decode(&self, path: &Path) -> Result<Photo, String> {
        decode_path(&self.factory, path)
    }
}

fn decode_path(factory: &IWICImagingFactory, path: &Path) -> Result<Photo, String> {
    unsafe {
        let wide = platform::wide_os(path.as_os_str());
        let decoder = factory
            .CreateDecoderFromFilename(
                PCWSTR(wide.as_ptr()),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )
            .map_err(|e| format!("Cannot read this file ({e:?})"))?;
        let frame = decoder
            .GetFrame(0)
            .map_err(|e| format!("No image frame ({e:?})"))?;

        let orientation = exif_orientation(&frame).unwrap_or(1);
        let source: IWICBitmapSource = frame
            .cast()
            .map_err(|e| format!("Image source failed ({e:?})"))?;

        let conv = factory
            .CreateFormatConverter()
            .map_err(|e| format!("Image converter failed ({e:?})"))?;
        conv.Initialize(
            &source,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )
        .map_err(|e| format!("Unsupported image format ({e:?})"))?;

        let (mut w, mut h) = (0u32, 0u32);
        conv.GetSize(&mut w, &mut h)
            .map_err(|e| format!("Image size failed ({e:?})"))?;
        if w == 0 || h == 0 {
            return Err("The image is empty".into());
        }
        if (w as u64) * (h as u64) > 400_000_000 {
            return Err("This image is too large to open".into());
        }

        let mut px = vec![0u32; w as usize * h as usize];
        {
            let bytes: &mut [u8] =
                slice::from_raw_parts_mut(px.as_mut_ptr() as *mut u8, px.len() * 4);
            conv.CopyPixels(std::ptr::null(), w * 4, bytes)
                .map_err(|e| format!("Image decode failed ({e:?})"))?;
        }
        let (px, w, h) = apply_orientation(px, w, h, orientation);

        let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        Ok(Photo {
            path: path.to_path_buf(),
            w,
            h,
            px,
            format: format_label(path),
            file_size,
        })
    }
}

fn exif_orientation(frame: &IWICBitmapFrameDecode) -> Option<u32> {
    unsafe {
        let reader = frame.GetMetadataQueryReader().ok()?;
        for query in [w!("/app1/ifd/{ushort=274}"), w!("/ifd/{ushort=274}")] {
            let mut value = PROPVARIANT::default();
            if reader.GetMetadataByName(query, &mut value).is_ok() {
                let out = PropVariantToUInt32(&value).ok();
                let _ = PropVariantClear(&mut value);
                if let Some(v) = out {
                    return Some(v);
                }
            }
        }
        None
    }
}

/// Apply the EXIF orientation to a freshly decoded buffer.
///
/// WIC can rotate during decode, but its flip-rotator hangs on some JPEGs on
/// Windows 11, so the pixels are turned here instead. One pass, cache-friendly
/// tiles, and only for pictures that actually carry an orientation tag.
fn apply_orientation(px: Vec<u32>, w: u32, h: u32, o: u32) -> (Vec<u32>, u32, u32) {
    if !(2..=8).contains(&o) {
        return (px, w, h);
    }
    let (nw, nh) = if o >= 5 { (h, w) } else { (w, h) };
    let mut out = vec![0u32; nw as usize * nh as usize];
    let (ww, hh) = (w as i32, h as i32);
    const T: usize = 32;
    for ty in (0..h as usize).step_by(T) {
        for tx in (0..w as usize).step_by(T) {
            for sy in ty..(ty + T).min(h as usize) {
                for sx in tx..(tx + T).min(w as usize) {
                    let (dx, dy) = match o {
                        2 => (ww - 1 - sx as i32, sy as i32),
                        3 => (ww - 1 - sx as i32, hh - 1 - sy as i32),
                        4 => (sx as i32, hh - 1 - sy as i32),
                        5 => (sy as i32, sx as i32),
                        6 => (hh - 1 - sy as i32, sx as i32),
                        7 => (hh - 1 - sy as i32, ww - 1 - sx as i32),
                        8 => (sy as i32, ww - 1 - sx as i32),
                        _ => (sx as i32, sy as i32),
                    };
                    out[dy as usize * nw as usize + dx as usize] = px[sy * w as usize + sx];
                }
            }
        }
    }
    (out, nw, nh)
}

/// Downscale a photo so it fits inside `max_w` x `max_h` (never upscales).
pub fn make_fit(photo: &Photo, max_w: u32, max_h: u32) -> Option<Fit> {
    let max_w = max_w.max(32);
    let max_h = max_h.max(32);
    let scale = (max_w as f64 / photo.w as f64)
        .min(max_h as f64 / photo.h as f64)
        .min(1.0);
    let dw = ((photo.w as f64 * scale).round() as u32).max(1);
    let dh = ((photo.h as f64 * scale).round() as u32).max(1);
    if dw == photo.w && dh == photo.h {
        return Some(Fit {
            w: dw,
            h: dh,
            px: photo.px.clone(),
        });
    }
    let mut out = vec![0u32; dw as usize * dh as usize];
    {
        let src = fir::images::ImageRef::new(
            photo.w,
            photo.h,
            as_bytes(&photo.px),
            fir::PixelType::U8x4,
        )
        .ok()?;
        let mut dst = fir::images::Image::from_slice_u8(
            dw,
            dh,
            as_bytes_mut(&mut out),
            fir::PixelType::U8x4,
        )
        .ok()?;
        let mut resizer = fir::Resizer::new();
        let opts = fir::ResizeOptions::new()
            .resize_alg(fir::ResizeAlg::Convolution(fir::FilterType::CatmullRom))
            .use_alpha(false);
        resizer.resize(&src, &mut dst, Some(&opts)).ok()?;
    }
    Some(Fit { w: dw, h: dh, px: out })
}

/// Resize a sub-rectangle of `photo` into a new BGRA bitmap.
pub fn resize_region(
    photo: &Photo,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    dw: u32,
    dh: u32,
) -> Option<Vec<u32>> {
    let dw = dw.max(1);
    let dh = dh.max(1);
    let mut out = vec![0u32; dw as usize * dh as usize];
    let src = fir::images::ImageRef::new(
        photo.w,
        photo.h,
        as_bytes(&photo.px),
        fir::PixelType::U8x4,
    )
    .ok()?;
    let mut dst =
        fir::images::Image::from_slice_u8(dw, dh, as_bytes_mut(&mut out), fir::PixelType::U8x4)
            .ok()?;
    let ratio = dw as f32 / w as f32;
    let alg = if ratio >= 1.0 {
        fir::ResizeAlg::Convolution(fir::FilterType::Bilinear)
    } else {
        fir::ResizeAlg::Convolution(fir::FilterType::CatmullRom)
    };
    let opts = fir::ResizeOptions::new()
        .resize_alg(alg)
        .crop(x, y, w, h)
        .use_alpha(false);
    let mut resizer = fir::Resizer::new();
    resizer.resize(&src, &mut dst, Some(&opts)).ok()?;
    Some(out)
}

pub fn crop(photo: &Photo, x: u32, y: u32, w: u32, h: u32) -> Option<Photo> {
    let x = x.min(photo.w.saturating_sub(1));
    let y = y.min(photo.h.saturating_sub(1));
    let w = w.min(photo.w - x).max(1);
    let h = h.min(photo.h - y).max(1);
    let mut px = Vec::with_capacity(w as usize * h as usize);
    let stride = photo.w as usize;
    for row in 0..h as usize {
        let start = (y as usize + row) * stride + x as usize;
        px.extend_from_slice(&photo.px[start..start + w as usize]);
    }
    Some(Photo {
        path: photo.path.clone(),
        w,
        h,
        px,
        format: photo.format,
        file_size: photo.file_size,
    })
}

pub fn rotate(photo: &Photo, clockwise: bool) -> Photo {
    let (w, h) = (photo.w as usize, photo.h as usize);
    let nw = h;
    let nh = w;
    let mut out = vec![0u32; nw * nh];
    const T: usize = 32;
    for ty in (0..h).step_by(T) {
        for tx in (0..w).step_by(T) {
            for y in ty..(ty + T).min(h) {
                for x in tx..(tx + T).min(w) {
                    let c = photo.px[y * w + x];
                    if clockwise {
                        // (x, y) -> (h - 1 - y, x)
                        out[x * nw + (h - 1 - y)] = c;
                    } else {
                        // (x, y) -> (y, w - 1 - x)
                        out[(w - 1 - x) * nw + y] = c;
                    }
                }
            }
        }
    }
    Photo {
        path: photo.path.clone(),
        w: nw as u32,
        h: nh as u32,
        px: out,
        format: photo.format,
        file_size: photo.file_size,
    }
}

pub fn has_alpha(photo: &Photo) -> bool {
    photo.px.iter().any(|p| (p >> 24) & 0xff != 0xff)
}

/// Save an edited photo next to the original, never overwriting anything.
/// Returns the new path. JPEG sources stay JPEG, everything else becomes PNG.
pub fn save_edited(photo: &Photo, suffix: &str) -> Result<PathBuf, String> {
    let ext = photo
        .path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let jpeg_source = matches!(ext.as_str(), "jpg" | "jpeg" | "jpe" | "jfif");
    let use_png = !jpeg_source || has_alpha(photo);
    let out_ext = if use_png { "png" } else { "jpg" };
    let stem = photo
        .path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("photo");
    let dir = photo.path.parent().unwrap_or(Path::new("."));
    let dest = unique_path(dir, stem, suffix, out_ext);

    if use_png {
        let file = File::create(&dest).map_err(|e| format!("Cannot save: {e}"))?;
        let mut enc = png::Encoder::new(BufWriter::new(file), photo.w, photo.h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc
            .write_header()
            .map_err(|e| format!("Cannot save: {e}"))?;
        let mut rgba = Vec::with_capacity(photo.px.len() * 4);
        for &p in &photo.px {
            rgba.push((p >> 16) as u8);
            rgba.push((p >> 8) as u8);
            rgba.push(p as u8);
            rgba.push((p >> 24) as u8);
        }
        writer
            .write_image_data(&rgba)
            .map_err(|e| format!("Cannot save: {e}"))?;
    } else {
        if photo.w > u16::MAX as u32 || photo.h > u16::MAX as u32 {
            return Err("This image is too large for JPEG".into());
        }
        let mut rgb = Vec::with_capacity(photo.px.len() * 3);
        for &p in &photo.px {
            rgb.push((p >> 16) as u8);
            rgb.push((p >> 8) as u8);
            rgb.push(p as u8);
        }
        let enc =
            jpeg_encoder::Encoder::new_file(&dest, 92).map_err(|e| format!("Cannot save: {e}"))?;
        enc.encode(
            &rgb,
            photo.w as u16,
            photo.h as u16,
            jpeg_encoder::ColorType::Rgb,
        )
        .map_err(|e| format!("Cannot save: {e}"))?;
    }
    Ok(dest)
}

fn unique_path(dir: &Path, stem: &str, suffix: &str, ext: &str) -> PathBuf {
    let mut name = format!("{stem}{suffix}.{ext}");
    let mut p = dir.join(&name);
    let mut n = 2;
    while p.exists() {
        name = format!("{stem}{suffix}-{n}.{ext}");
        p = dir.join(&name);
        n += 1;
    }
    p
}

pub fn format_label(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" | "jpe" | "jfif" => "JPEG",
        "png" => "PNG",
        "bmp" => "BMP",
        "gif" => "GIF",
        "tif" | "tiff" => "TIFF",
        "webp" => "WebP",
        "ico" => "ICO",
        "heic" => "HEIC",
        "heif" => "HEIF",
        "avif" => "AVIF",
        "jxl" => "JPEG XL",
        _ => "Image",
    }
}

fn as_bytes(p: &[u32]) -> &[u8] {
    unsafe { slice::from_raw_parts(p.as_ptr() as *const u8, p.len() * 4) }
}

fn as_bytes_mut(p: &mut [u32]) -> &mut [u8] {
    unsafe { slice::from_raw_parts_mut(p.as_mut_ptr() as *mut u8, p.len() * 4) }
}
