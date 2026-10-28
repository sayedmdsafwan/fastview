// FastView - Win32 platform helpers.
//
// Everything here is small and direct on purpose: no GUI framework, no GPU
// initialisation, no runtime. The window is a plain Win32 window, the picture is
// drawn by us into a DIB and blitted with GDI, and dialogs/shell features use
// the Windows APIs directly.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows::core::{w, PCWSTR, Result, BOOL};
use windows::Win32::Foundation::{
    COLORREF, HANDLE, HINSTANCE, HWND, LPARAM, RECT, WPARAM,
};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, HBRUSH, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::HiDpi::{
    GetDpiForSystem, GetDpiForWindow, SetProcessDpiAwarenessContext,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Shell::{
    FileOpenDialog, IFileOpenDialog, FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION,
    FOF_NOERRORUI, FOF_SILENT, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS,
    SHChangeNotify, SHFileOperationW, SHFILEOPSTRUCTW, SHCNE_ASSOCCHANGED, SHCNF_IDLIST,
    SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

pub const APP_NAME: &str = "FastView";
pub const CLASS_NAME: &str = "FastViewMainWindow";

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn wide_os(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

pub fn init_process() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

pub fn dpi() -> u32 {
    unsafe { GetDpiForSystem() }.max(96)
}

pub fn register_class(hinst: HINSTANCE, proc_: WNDPROC) {
    unsafe {
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: proc_,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: HICON::default(),
            hCursor: HCURSOR::default(),
            hbrBackground: HBRUSH::default(),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: w!("FastViewMainWindow"),
        };
        RegisterClassW(&wc);
    }
}

pub fn create_window(hinst: HINSTANCE, title: &str, client_w: i32, client_h: i32) -> Result<HWND> {
    unsafe {
        let style = WS_OVERLAPPEDWINDOW;
        let exstyle = WINDOW_EX_STYLE::default();

        // Never open larger than the screen: this is often a 1366x768 laptop.
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let mon = MonitorFromWindow(HWND::default(), MONITOR_DEFAULTTONEAREST);
        let _ = GetMonitorInfoW(mon, &mut mi);
        let work_w = mi.rcWork.right - mi.rcWork.left;
        let work_h = mi.rcWork.bottom - mi.rcWork.top;
        let client_w = client_w.min((work_w - 70).max(480));
        let client_h = client_h.min((work_h - 90).max(360));

        let mut rc = RECT {
            left: 0,
            top: 0,
            right: client_w,
            bottom: client_h,
        };
        let _ = AdjustWindowRectEx(&mut rc, style, false, exstyle);
        let ww = rc.right - rc.left;
        let wh = rc.bottom - rc.top;
        let x = mi.rcWork.left + (work_w - ww).max(0) / 2;
        let y = mi.rcWork.top + (work_h - wh).max(0) / 2;

        let hwnd = CreateWindowExW(
            exstyle,
            w!("FastViewMainWindow"),
            PCWSTR(wide(title).as_ptr()),
            style | WS_VISIBLE,
            x,
            y,
            ww,
            wh,
            None,
            None,
            Some(hinst),
            None,
        )?;
        Ok(hwnd)
    }
}

pub fn set_window_icon(hwnd: HWND, hinst: HINSTANCE) {
    unsafe {
        let icon = LoadImageW(
            Some(hinst),
            PCWSTR(1 as *const u16),
            IMAGE_ICON,
            0,
            0,
            LR_DEFAULTSIZE | LR_SHARED,
        );
        if let Ok(icon) = icon {
            SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_BIG as usize)),
                Some(LPARAM(icon.0 as isize)),
            );
            SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_SMALL as usize)),
                Some(LPARAM(icon.0 as isize)),
            );
        }
    }
}

pub fn dark_chrome(hwnd: HWND) {
    unsafe {
        // Attribute numbers are stable even where the SDK name is missing.
        let dark: BOOL = true.into();
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(20),
            &dark as *const _ as *const core::ffi::c_void,
            4,
        );
        // Rounded corners (Windows 11).
        let round = 2u32;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(33),
            &round as *const _ as *const core::ffi::c_void,
            4,
        );
        // Caption / text / border colours.
        let caption = 0x0011_0e0b_u32; // COLORREF 0x00BBGGRR = #0b0e11
        let text = 0x00f2_ebe8_u32; // #e8ebf2
        let border = 0x0016_1310_u32; // #101316
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(35),
            &caption as *const _ as *const core::ffi::c_void,
            4,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(36),
            &text as *const _ as *const core::ffi::c_void,
            4,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(34),
            &border as *const _ as *const core::ffi::c_void,
            4,
        );
    }
}

pub fn window_dpi(hwnd: HWND) -> u32 {
    unsafe { GetDpiForWindow(hwnd) }.max(96)
}

pub fn client_size(hwnd: HWND) -> (i32, i32) {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
    }
    ((rc.right - rc.left).max(1), (rc.bottom - rc.top).max(1))
}

pub fn set_title(hwnd: HWND, title: &str) {
    let t = wide(title);
    unsafe {
        let _ = SetWindowTextW(hwnd, PCWSTR(t.as_ptr()));
    }
}

pub fn open_dialog(hwnd: HWND, folder: bool) -> Option<PathBuf> {
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let mut opts = dialog.GetOptions().ok()?;
        opts |= FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST;
        if folder {
            opts |= FOS_PICKFOLDERS;
        }
        let _ = dialog.SetOptions(opts);
        let title = if folder {
            w!("Open a folder")
        } else {
            w!("Open a photo")
        };
        let _ = dialog.SetTitle(title);
        let _ = dialog.SetFileName(w!(""));
        if dialog.Show(Some(hwnd)).is_err() {
            return None;
        }
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = name.to_string().ok()?;
        CoTaskMemFree(Some(name.0 as *const core::ffi::c_void));
        Some(PathBuf::from(s))
    }
}

/// Send a file to the Recycle Bin without a shell confirmation dialog.
pub fn recycle(path: &Path) -> bool {
    let mut from: Vec<u16> = path.as_os_str().encode_wide().collect();
    from.push(0);
    from.push(0);
    unsafe {
        let mut op = SHFILEOPSTRUCTW {
            hwnd: HWND::default(),
            wFunc: FO_DELETE,
            pFrom: PCWSTR(from.as_ptr()),
            pTo: PCWSTR::null(),
            fFlags: (FOF_ALLOWUNDO.0 | FOF_NOCONFIRMATION.0 | FOF_SILENT.0 | FOF_NOERRORUI.0)
                as u16,
            ..Default::default()
        };
        let rc = SHFileOperationW(&mut op);
        rc == 0 && !op.fAnyOperationsAborted.as_bool()
    }
}

/// Put a 32-bit BGRA image on the clipboard as a bottom-up 24-bit DIB.
pub fn copy_to_clipboard(hwnd: HWND, w: u32, h: u32, px: &[u32]) -> bool {
    unsafe {
        let stride = ((w as usize * 3 + 3) & !3) as usize;
        let size = 40 + stride * h as usize;
        let hmem = match GlobalAlloc(GMEM_MOVEABLE, size) {
            Ok(m) if !m.is_invalid() => m,
            _ => return false,
        };
        let base = GlobalLock(hmem);
        if base.is_null() {
            return false;
        }
        let dst = base as *mut u8;
        let mut hdr = [0u8; 40];
        hdr[0..4].copy_from_slice(&40u32.to_le_bytes());
        hdr[4..8].copy_from_slice(&(w as i32).to_le_bytes());
        hdr[8..12].copy_from_slice(&(h as i32).to_le_bytes());
        hdr[12..14].copy_from_slice(&1u16.to_le_bytes());
        hdr[14..16].copy_from_slice(&24u16.to_le_bytes());
        hdr[20..24].copy_from_slice(&((stride * h as usize) as u32).to_le_bytes());
        std::ptr::copy_nonoverlapping(hdr.as_ptr(), dst, 40);
        for row in 0..h as usize {
            let src_row = &px[(h as usize - 1 - row) * w as usize..][..w as usize];
            let out = dst.add(40 + row * stride);
            for (i, c) in src_row.iter().enumerate() {
                *out.add(i * 3) = (*c & 0xff) as u8;
                *out.add(i * 3 + 1) = ((*c >> 8) & 0xff) as u8;
                *out.add(i * 3 + 2) = ((*c >> 16) & 0xff) as u8;
            }
        }
        let _ = GlobalUnlock(hmem);
        if OpenClipboard(Some(hwnd)).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let ok = SetClipboardData(8, Some(HANDLE(hmem.0))).is_ok();
        let _ = CloseClipboard();
        ok
    }
}

pub fn load_cursor(id: PCWSTR) -> HCURSOR {
    unsafe { LoadCursorW(None, id).unwrap_or_default() }
}

pub fn hinstance() -> HINSTANCE {
    unsafe { GetModuleHandleW(None).unwrap_or_default().into() }
}

/// Put FastView into Explorer's per-user "Open with" list.
///
/// Windows 11 builds that list from the Applications key, the extension's
/// OpenWithProgids list and the app capabilities registration. The installer
/// does this too, but doing it at startup means it also works for the portable
/// copy and for people who skipped the installer checkbox.
pub fn register_open_with() {
    #[cfg(debug_assertions)]
    {
        // Never let a development build steal the association from the
        // installed/portable release.
        return;
    }
    #[cfg(not(debug_assertions))]
    let _ = install_open_with();
}

#[cfg(not(debug_assertions))]
fn install_open_with() -> std::io::Result<()> {
    let exe_path = std::env::current_exe()?;
    let exe = exe_path.to_string_lossy().to_string();
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    // Do not touch the shell on every single launch.
    let marker = hkcu.create_subkey("Software\\FastView")?.0;
    let previous: String = marker.get_value("OpenWithExe").unwrap_or_default();
    if previous.eq_ignore_ascii_case(&exe) {
        return Ok(());
    }

    let classes = hkcu.create_subkey("Software\\Classes")?.0;
    let apps = classes.create_subkey("Applications")?.0;
    let app = apps.create_subkey("fastview.exe")?.0;
    app.set_value("", &"FastView")?;
    app.set_value("FriendlyAppName", &"FastView")?;
    app.set_value("ApplicationCompany", &"Md. Safwan")?;
    app.create_subkey("DefaultIcon")?
        .0
        .set_value("", &format!("\"{exe}\",0"))?;
    let shell = app.create_subkey("shell")?.0;
    let open = shell.create_subkey("open")?.0;
    open.create_subkey("command")?
        .0
        .set_value("", &format!("\"{exe}\" \"%1\""))?;
    let supported = app.create_subkey("SupportedTypes")?.0;

    let progid = classes.create_subkey("FastView.Image")?.0;
    progid.set_value("", &"FastView picture")?;
    progid
        .create_subkey("DefaultIcon")?
        .0
        .set_value("", &format!("\"{exe}\",0"))?;
    let pshell = progid.create_subkey("shell")?.0;
    let popen = pshell.create_subkey("open")?.0;
    popen.create_subkey("command")?
        .0
        .set_value("", &format!("\"{exe}\" \"%1\""))?;

    let caps = classes.create_subkey("FastView")?.0;
    let caps = caps.create_subkey("Capabilities")?.0;
    caps.set_value("ApplicationName", &"FastView")?;
    caps.set_value(
        "ApplicationDescription",
        &"A fast, tiny photo viewer for Windows",
    )?;
    caps.set_value("ApplicationIcon", &format!("\"{exe}\",0"))?;
    let assoc = caps.create_subkey("FileAssociations")?.0;
    hkcu.create_subkey("Software\\RegisteredApplications")?
        .0
        .set_value("FastView", &"Software\\Classes\\FastView\\Capabilities")?;

    for ext in crate::library::EXTENSIONS {
        let dotted = format!(".{ext}");
        supported.set_value(dotted.as_str(), &"")?;
        assoc.set_value(dotted.as_str(), &"FastView.Image")?;
        let ext_key = classes.create_subkey(dotted.as_str())?.0;
        let progids = ext_key.create_subkey("OpenWithProgids")?.0;
        progids.set_value("FastView.Image", &"")?;
    }

    marker.set_value("OpenWithExe", &exe)?;
    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
    Ok(())
}

/// Toggle between a normal frame and a borderless full-screen window.
pub struct Fullscreen {
    active: bool,
    style: isize,
    exstyle: isize,
    placement: WINDOWPLACEMENT,
}

impl Fullscreen {
    pub fn new() -> Self {
        Self {
            active: false,
            style: 0,
            exstyle: 0,
            placement: WINDOWPLACEMENT::default(),
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn toggle(&mut self, hwnd: HWND) {
        unsafe {
            if !self.active {
                self.style = GetWindowLongPtrW(hwnd, GWL_STYLE);
                self.exstyle = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                self.placement = WINDOWPLACEMENT {
                    length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                    ..Default::default()
                };
                let _ = GetWindowPlacement(hwnd, &mut self.placement);
                let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                let mut mi = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                let _ = GetMonitorInfoW(mon, &mut mi);
                let _ = SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_POPUP | WS_VISIBLE).0 as isize);
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOP),
                    mi.rcMonitor.left,
                    mi.rcMonitor.top,
                    mi.rcMonitor.right - mi.rcMonitor.left,
                    mi.rcMonitor.bottom - mi.rcMonitor.top,
                    SWP_FRAMECHANGED | SWP_NOOWNERZORDER,
                );
                self.active = true;
            } else {
                let _ = SetWindowLongPtrW(hwnd, GWL_STYLE, self.style);
                let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, self.exstyle);
                let _ = SetWindowPlacement(hwnd, &self.placement);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER,
                );
                self.active = false;
            }
        }
    }
}






