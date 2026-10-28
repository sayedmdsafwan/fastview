// FastView - one background loader thread.
//
// Decoding a photo takes ~100 ms on a slow CPU, so it must never happen on the UI
// thread. The loader has one "current" slot (new requests overwrite it) and a
// small prefetch queue for the neighbouring pictures, so pressing the arrow keys
// is instant after the first look-ahead.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::imaging::{self, Fit, Photo, Wic};

pub const WM_APP_LOADED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 1;

pub struct Meta {
    pub w: u32,
    pub h: u32,
    pub format: &'static str,
    pub file_size: u64,
}

pub enum Kind {
    Full,
    Fit,
}

pub struct Loaded {
    pub seq: u64,
    pub path: PathBuf,
    pub kind: Kind,
    pub photo: Option<Arc<Photo>>,
    pub fit: Option<Fit>,
    pub fit_w: u32,
    pub fit_h: u32,
    pub meta: Option<Meta>,
    pub error: Option<String>,
}

struct Job {
    path: PathBuf,
    seq: u64,
    want_full: bool,
    fit_w: u32,
    fit_h: u32,
}

struct Shared {
    current: Mutex<Option<Job>>,
    prefetch: Mutex<VecDeque<Job>>,
    results: Mutex<Vec<Loaded>>,
    cv: Condvar,
    seq: AtomicU64,
    quit: AtomicBool,
    hwnd: AtomicIsize,
}

pub struct Loader {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    stop_tx: mpsc::Sender<()>,
}

impl Loader {
    pub fn new(hwnd: HWND) -> Self {
        let shared = Arc::new(Shared {
            current: Mutex::new(None),
            prefetch: Mutex::new(VecDeque::new()),
            results: Mutex::new(Vec::new()),
            cv: Condvar::new(),
            seq: AtomicU64::new(0),
            quit: AtomicBool::new(false),
            hwnd: AtomicIsize::new(hwnd.0 as isize),
        });
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let s = shared.clone();
        let thread = thread::Builder::new()
            .name("fastview-loader".into())
            .spawn(move || {
                let _ = stop_rx;
                worker(s);
            })
            .ok();
        Self {
            shared,
            thread,
            stop_tx,
        }
    }

    /// Replace the image the user wants right now. Pending prefetches are dropped.
    pub fn current(&self, path: PathBuf, seq: u64, want_full: bool, fit_w: u32, fit_h: u32) {
        self.shared.seq.store(seq, Ordering::SeqCst);
        if let Ok(mut q) = self.shared.prefetch.lock() {
            q.clear();
        }
        if let Ok(mut c) = self.shared.current.lock() {
            *c = Some(Job {
                path,
                seq,
                want_full,
                fit_w,
                fit_h,
            });
        }
        self.shared.cv.notify_all();
    }

    pub fn prefetch(&self, path: PathBuf, seq: u64, fit_w: u32, fit_h: u32) {
        if self.shared.seq.load(Ordering::SeqCst) != seq {
            return;
        }
        if let Ok(mut q) = self.shared.prefetch.lock() {
            if q.len() < 6 && !q.iter().any(|j| j.path == path) {
                q.push_back(Job {
                    path,
                    seq,
                    want_full: false,
                    fit_w,
                    fit_h,
                });
            }
        }
        self.shared.cv.notify_all();
    }

    pub fn clear_prefetch(&self) {
        if let Ok(mut q) = self.shared.prefetch.lock() {
            q.clear();
        }
    }

    pub fn poll(&self) -> Vec<Loaded> {
        match self.shared.results.lock() {
            Ok(mut r) => std::mem::take(&mut *r),
            Err(_) => Vec::new(),
        }
    }

    pub fn stop(&mut self) {
        self.shared.quit.store(true, Ordering::SeqCst);
        let _ = self.stop_tx.send(());
        self.shared.cv.notify_all();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        self.stop();
    }
}

fn worker(shared: Arc<Shared>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let mut wic: Option<Wic> = None;
    loop {
        if shared.quit.load(Ordering::SeqCst) {
            break;
        }
        let job = {
            let mut cur = match shared.current.lock() {
                Ok(c) => c,
                Err(_) => break,
            };
            if let Some(j) = cur.take() {
                Some(j)
            } else {
                drop(cur);
                let mut q = match shared.prefetch.lock() {
                    Ok(q) => q,
                    Err(_) => break,
                };
                if let Some(j) = q.pop_front() {
                    Some(j)
                } else {
                    let Ok((guard, _)) = shared.cv.wait_timeout(q, Duration::from_millis(150)) else {
                        continue;
                    };
                    drop(guard);
                    continue;
                }
            }
        };
        let Some(job) = job else { continue };
        if job.seq != shared.seq.load(Ordering::SeqCst) {
            continue;
        }
        if wic.is_none() {
            wic = Wic::new().ok();
        }
        let Some(w) = wic.as_ref() else {
            let loaded = Loaded {
                seq: job.seq,
                path: job.path,
                kind: Kind::Fit,
                photo: None,
                fit: None,
                fit_w: 0,
                fit_h: 0,
                meta: None,
                error: Some("Windows imaging is unavailable".into()),
            };
            push_result(&shared, loaded);
            continue;
        };

        let path = job.path.clone();
        let start = std::time::Instant::now();
        let result = match w.decode(&path) {
            Ok(photo) => {
                let meta = Meta {
                    w: photo.w,
                    h: photo.h,
                    format: photo.format,
                    file_size: photo.file_size,
                };
                let fit = if job.fit_w > 0 && job.fit_h > 0 {
                    imaging::make_fit(&photo, job.fit_w, job.fit_h)
                } else {
                    None
                };
                let photo = if job.want_full {
                    Some(Arc::new(photo))
                } else {
                    None
                };
                Loaded {
                    seq: job.seq,
                    path,
                    kind: if job.want_full { Kind::Full } else { Kind::Fit },
                    photo,
                    fit,
                    fit_w: job.fit_w,
                    fit_h: job.fit_h,
                    meta: Some(meta),
                    error: None,
                }
            }
            Err(e) => Loaded {
                seq: job.seq,
                path,
                kind: if job.want_full { Kind::Full } else { Kind::Fit },
                photo: None,
                fit: None,
                fit_w: 0,
                fit_h: 0,
                meta: None,
                error: Some(e),
            },
        };
        crate::timing::mark(&format!(
            "decode {}ms seq={} full={}",
            start.elapsed().as_millis(),
            job.seq,
            job.want_full
        ));
        push_result(&shared, result);
    }
    unsafe {
        let _ = windows::Win32::System::Com::CoUninitialize();
    }
}

fn push_result(shared: &Shared, loaded: Loaded) {
    if let Ok(mut r) = shared.results.lock() {
        r.push(loaded);
    }
    unsafe {
        let hwnd = HWND(shared.hwnd.load(Ordering::SeqCst) as *mut core::ffi::c_void);
        let _ = PostMessageW(Some(hwnd), WM_APP_LOADED, WPARAM(0), LPARAM(0));
    }
}

