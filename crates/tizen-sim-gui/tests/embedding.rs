//! The Engine window embedded in a window of this process, as the GUI embeds
//! it (ADR-0006), on a real Session. Windows only: elsewhere the Engine keeps
//! its own window. Runs on `TIZEN_SIM_TEST_PROFILE`'s Engine, or the latest.

#![cfg(windows)]

use std::path::PathBuf;
use std::time::{Duration, Instant};
use tizen_sim_core::app::AppSource;
use tizen_sim_core::home::Home;
use tizen_sim_core::registry::SessionRegistry;
use tizen_sim_core::screen::AppResolution;
use tizen_sim_core::session::{Session, SessionConfig};
use tizen_sim_gui::embed::{Embedder, ScreenArea};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GRAY_BRUSH, GetDC, GetPixel, GetStockObject, HBRUSH, ReleaseDC,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GW_CHILD, GetSystemMetrics,
    GetWindow, GetWindowRect, MSG, PM_REMOVE, PeekMessageW, RegisterClassW, SM_CXSCREEN,
    SM_CYSCREEN, TranslateMessage, WNDCLASSW, WS_CLIPCHILDREN, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};
use windows::core::w;

/// Runs this thread's windows, which the Embedder's host belongs to.
fn pump(duration: Duration) {
    let until = Instant::now() + duration;
    while Instant::now() < until {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A gray window standing in for the GUI's, above every other window so
/// the screen shows what is embedded in it.
fn gui_window(width: i32, height: i32) -> HWND {
    unsafe extern "system" fn proc_(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        unsafe { DefWindowProcW(h, m, w, l) }
    }
    unsafe {
        let instance = GetModuleHandleW(None).unwrap();
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc_),
            hInstance: instance.into(),
            lpszClassName: w!("TizenSimEmbeddingTest"),
            hbrBackground: HBRUSH(GetStockObject(GRAY_BRUSH).0),
            ..Default::default()
        };
        RegisterClassW(&class);
        CreateWindowExW(
            WS_EX_TOPMOST,
            w!("TizenSimEmbeddingTest"),
            w!("embedding test"),
            WS_POPUP | WS_VISIBLE | WS_CLIPCHILDREN,
            0,
            0,
            width,
            height,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap()
    }
}

/// The color on the screen at `(x, y)` from the top left of `window`.
fn screen_color(window: HWND, x: i32, y: i32) -> (u32, u32, u32) {
    unsafe {
        let mut rect = RECT::default();
        GetWindowRect(window, &mut rect).unwrap();
        let dc = GetDC(None);
        let color = GetPixel(dc, rect.left + x, rect.top + y).0;
        ReleaseDC(None, dc);
        (color & 0xff, (color >> 8) & 0xff, (color >> 16) & 0xff)
    }
}

/// A windowed Session on the `screen` fixture, its Engine window embedded in
/// a stand-in for the GUI's window, on a 1280x720 screen area.
struct Embedded {
    gui: HWND,
    host: HWND,
    embedder: Embedder,
    session: Session,
    runtime: tokio::runtime::Runtime,
    _state: tempfile::TempDir,
}

fn embedded() -> Embedded {
    embedded_in(AppResolution::Uhd, 1280.0, 720.0)
}

/// As `embedded`, for an App at `resolution` on a `width` by `height` screen
/// area.
fn embedded_in(resolution: AppResolution, width: f64, height: f64) -> Embedded {
    let gui = gui_window(width as i32 + 20, height as i32 + 20);
    let state = tempfile::tempdir().unwrap();
    let home = Home::from_env().with_state_dir(state.path().to_path_buf());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let session = runtime.block_on(async {
        let session = Session::start(SessionConfig {
            source: AppSource::ProjectFolder(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/apps/screen"),
            ),
            profile: std::env::var("TIZEN_SIM_TEST_PROFILE")
                .ok()
                .filter(|version| !version.is_empty()),
            engine_path: None,
            engine_has_headless: None,
            // Only a windowed Engine has a window to embed.
            headless: false,
            lenient: false,
            key_map: Default::default(),
            device: Default::default(),
            resolution,
            checkerboard: false,
            gamepads: Vec::new(),
            home: home.clone(),
        });
        session.wait_until_loaded().await.unwrap();
        session
    });
    let entry = SessionRegistry::new(&home)
        .list()
        .into_iter()
        .next()
        .unwrap();

    let embedder = Embedder::new(gui.0 as isize);
    embedder.set_area(
        Some(ScreenArea {
            x: 10.0,
            y: 10.0,
            width,
            height,
        }),
        1.0,
    );
    assert!(embedder.embed(entry.window.unwrap(), entry.engine_pid, false));
    let host = unsafe { GetWindow(gui, GW_CHILD) }.unwrap();
    pump(Duration::from_millis(500));
    Embedded {
        gui,
        host,
        embedder,
        session,
        runtime,
        _state: state,
    }
}

impl Embedded {
    fn end(self) {
        self.embedder.release();
        self.runtime.block_on(self.session.stop());
        unsafe { DestroyWindow(self.gui).unwrap() };
    }
}

/// The `screen` fixture draws a red box at the top left of the TV screen.
#[test]
fn an_embedded_engine_window_shows_the_app() {
    let t = embedded();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut color = screen_color(t.host, 30, 30);
    while !is_red(color) && Instant::now() < deadline {
        pump(Duration::from_millis(200));
        color = screen_color(t.host, 30, 30);
    }
    t.end();
    assert!(
        is_red(color),
        "the screen shows {color:?} where the App's red box is"
    );
}

/// A screenshot is the App's screen whether or not its window is embedded:
/// an App filling it in blue is blue to the bottom right corner.
#[test]
fn a_screenshot_of_an_embedded_session_is_the_whole_app() {
    let t = embedded();
    t.runtime
        .block_on(t.session.evaluate(
            "var fill = document.createElement('div');              fill.style.cssText = 'position:fixed;left:0;top:0;width:1920px;height:1080px;background:#00f';              document.body.appendChild(fill); 0",
        ))
        .unwrap();
    pump(Duration::from_millis(500));
    let shot = t.runtime.block_on(t.session.screenshot()).unwrap();
    t.end();
    let (width, height) = png_size(&shot);
    assert_eq!((width, height), (1920, 1080));
    for (x, y) in [(5, 5), (1915, 5), (5, 1075), (1915, 1075)] {
        assert_eq!(pixel(&shot, x, y), [0, 0, 255], "at ({x}, {y})");
    }
}

/// An App scaled up to a screen area larger than its resolution fills it
/// (#148): Chromium kept drawing it in a frame of the App's size, so it was
/// cut off at 1280x720 px, as a 1920x1080 App on a larger area is at
/// 1920x1080. The FHD resolution keeps the area within a small display.
#[test]
fn an_app_scaled_up_fills_the_screen_area() {
    // Off the display, the screen's colour can't be read.
    let display = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    assert!(
        display.0 >= 1460 && display.1 >= 830,
        "the display is {display:?}, too small for the screen area"
    );
    let t = embedded_in(AppResolution::Fhd, 1440.0, 810.0);
    t.runtime
        .block_on(t.session.evaluate(
            "var fill = document.createElement('div');              fill.style.cssText = 'position:fixed;left:0;top:0;width:100%;height:100%;background:#00f';              document.body.appendChild(fill); 0",
        ))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut color = screen_color(t.host, 1430, 800);
    while !is_blue(color) && Instant::now() < deadline {
        pump(Duration::from_millis(200));
        color = screen_color(t.host, 1430, 800);
    }
    t.end();
    assert!(
        is_blue(color),
        "the screen shows {color:?} at the bottom right of the App"
    );
}

fn is_blue((r, g, b): (u32, u32, u32)) -> bool {
    r < 60 && g < 60 && b > 200
}

fn is_red((r, g, b): (u32, u32, u32)) -> bool {
    r > 200 && g < 60 && b < 60
}

fn png_size(png: &[u8]) -> (u32, u32) {
    let reader = png::Decoder::new(std::io::Cursor::new(png))
        .read_info()
        .unwrap();
    (reader.info().width, reader.info().height)
}

/// The RGB colour of a pixel of a PNG screenshot.
fn pixel(png: &[u8], x: u32, y: u32) -> [u8; 3] {
    let mut reader = png::Decoder::new(std::io::Cursor::new(png))
        .read_info()
        .unwrap();
    let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
    let frame = reader.next_frame(&mut buffer).unwrap();
    let at = (y * frame.width + x) as usize * frame.color_type.samples();
    [buffer[at], buffer[at + 1], buffer[at + 2]]
}
