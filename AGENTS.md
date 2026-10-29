# FastView — notes for the AI assistant

## Language
- Owner: Safwan. He speaks Bangla / Banglish.
- If he writes Bangla or Banglish, answer in **Bengali script (বাংলা)**.
- If he writes English, answer in English.

## How he works
- He is a "vibe coder": he does not read or write Rust.
- Prefer making the change directly in the project files. If he asks for a
  single file, give him the complete file, not a diff.
- Keep answers short and step by step. Do not redesign the UI unless asked.
- He cares most about: instant opening, smooth zoom/pan, tiny memory use, and
  an installer that works on a normal user account (no admin).

## Project facts
- Windows photo viewer, Rust 2024, native Win32 (no GUI framework, no GPU).
- Decode: WIC (built into Windows) + CPU EXIF orientation.
- Scale: `fast_image_resize` (SIMD + rayon). Presentation: one 32-bit DIB and
  one `BitBlt` per frame. Text: GDI `DrawTextW` into the same DIB.
- Source layout:
  - `src/main.rs` window procedure, app state, input, actions, paint pipeline
  - `src/platform.rs` window, dialogs, Recycle Bin, clipboard, fullscreen
  - `src/imaging.rs` WIC decode, EXIF, resize, crop/rotate, JPEG/PNG saving
  - `src/loader.rs` background decode thread + prefetch queue
  - `src/ui.rs` layout and all chrome/overlays
  - `src/gfx.rs` software canvas primitives + GDI text
  - `src/library.rs` folder scan + natural sort
- Build: `cargo build --release`
- Installer: `FastView.iss` -> `installer\FastView_Setup.exe` with Inno Setup 6
- Release profile: opt-level 3, thin LTO, 1 CGU, panic=abort, strip, static CRT.

## Gotchas
- The window is a windowed exe: never use `println!` for user-facing messages.
  Use the in-app toast or the title bar.
- All mouse move coordinates come from `lParam` (`lo`/`hi`), wheel coordinates
  are screen coordinates (`ScreenToClient`).
- `view.zoom` is only meaningful when `view.fit == false`; use `zoom_now()` when
  converting between screen and image space.
- Do not write source files with PowerShell `Get-Content`/`Set-Content`: it
  mangles the UTF-8 strings (·, ×, —, arrows). Use the editor/`edit` tool.
- GDI handles (`HDC`, `HFONT`, `HBITMAP`) live in `windows::Win32::Graphics::Gdi`
  in windows 0.61, not in `Foundation`.
- `PostMessageW`/`SendMessageW` take `Option<WPARAM>`/`Option<LPARAM>` and
  `Option<HWND>` in windows 0.61.
- WIC's `IWICBitmapFlipRotator` hangs on some JPEGs; EXIF orientation is applied
  on the CPU in `imaging::apply_orientation` instead.
- Keep the UI thread free of decode work; the loader owns WIC and the previews.

## Tests
- Test pictures in `E:\Rust Projects\_bench\photos` (24 MP, portrait, EXIF 6,
  transparent PNG, broken file, etc.).
- `FASTVIEW_TIMING=1` writes window/decode timings to
  `%TEMP%\fastview_timing.txt`.
