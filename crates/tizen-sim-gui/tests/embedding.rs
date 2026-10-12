//! The Engine window embedded in a window of this process, as the GUI embeds
//! it (ADR-0006), on a real Session. Windows only: elsewhere the Engine keeps
//! its own window. Runs on `TIZEN_SIM_TEST_PROFILE`'s Engine, or the latest.

#![cfg(windows)]

use std::path::PathBuf;
use std::time::{Duration, Instant};
use tizen_sim_core::app::AppSource;
use tizen_sim_core::home::Home;
use tizen_sim_core::registry::SessionRegistry;
use tizen_sim_core::session::{Session, SessionConfig};
use tizen_sim_gui::embed::{Embedder, ScreenArea};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GRAY_BRUSH, GetDC, GetPixel, GetStockObject, HBRUSH, ReleaseDC,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GW_CHILD, GetWindow,
    GetWindowRect, MSG, PM_REMOVE, PeekMessageW, RegisterClassW, TranslateMessage, WNDCLASSW,
    WS_CLIPCHILDREN, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
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
fn gui_window() -> HWND {
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
            1300,
            740,
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

/// The `screen` fixture draws a red box at the top left of the TV screen.
#[test]
fn an_embedded_engine_window_shows_the_app() {
    let gui = gui_window();
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
            resolution: Default::default(),
            checkerboard: false,
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
            width: 1280.0,
            height: 720.0,
        }),
        1.0,
    );
    assert!(embedder.embed(entry.window.unwrap(), entry.engine_pid, false));
    let host = unsafe { GetWindow(gui, GW_CHILD) }.unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut color = screen_color(host, 30, 30);
    while !is_red(color) && Instant::now() < deadline {
        pump(Duration::from_millis(200));
        color = screen_color(host, 30, 30);
    }

    embedder.release();
    runtime.block_on(session.stop());
    unsafe { DestroyWindow(gui).unwrap() };
    assert!(
        is_red(color),
        "the screen shows {color:?} where the App's red box is"
    );
}

fn is_red((r, g, b): (u32, u32, u32)) -> bool {
    r > 200 && g < 60 && b < 60
}
