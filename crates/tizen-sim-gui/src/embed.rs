//! The TV screen inside the GUI (ADR-0006). On Windows the Engine's native
//! window is re-parented into a host window that sits centered at 16:9 in
//! the GUI's screen area, follows it as the GUI resizes or changes DPI, and
//! hands keyboard focus to the Engine when clicked. Elsewhere the Engine
//! keeps its own window and nothing here does anything.
//!
//! Every method must run on the thread that owns the GUI's window.

use serde::Deserialize;

/// The space the TV screen goes in, in the GUI's window: the screen area's
/// box inside its padding, in CSS px of the GUI's webview, which fills the
/// window's client area.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct ScreenArea {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A box in physical pixels of the window's client area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalBox {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl ScreenArea {
    /// The area in physical pixels at `scale` physical pixels per CSS px.
    /// Edges are rounded, so neighbouring boxes never overlap or leave a gap.
    pub fn to_physical(self, scale: f64) -> PhysicalBox {
        let left = (self.x * scale).round();
        let top = (self.y * scale).round();
        let right = ((self.x + self.width) * scale).round();
        let bottom = ((self.y + self.height) * scale).round();
        PhysicalBox {
            x: left as i32,
            y: top as i32,
            width: (right - left).max(0.0) as i32,
            height: (bottom - top).max(0.0) as i32,
        }
    }
}

impl PhysicalBox {
    /// Where the TV screen goes in this box: as large as fits at 16:9, the
    /// aspect of both App resolutions, and centered. The App is scaled to
    /// fill the Engine window (`tizen_sim_core::screen`), so the leftover is
    /// the letterbox.
    pub fn fit_tv_screen(self) -> PhysicalBox {
        let width = self
            .width
            .min((f64::from(self.height) * 16.0 / 9.0).round() as i32);
        let height = self
            .height
            .min((f64::from(self.width) * 9.0 / 16.0).round() as i32);
        PhysicalBox {
            x: self.x + (self.width - width) / 2,
            y: self.y + (self.height - height) / 2,
            width,
            height,
        }
    }
}

#[cfg(windows)]
pub use windows_impl::Embedder;

#[cfg(not(windows))]
pub use unsupported::Embedder;

/// Embedding is Windows-only (ADR-0006): the Engine keeps its own window.
#[cfg(not(windows))]
mod unsupported {
    use super::ScreenArea;

    pub struct Embedder;

    impl Embedder {
        pub const SUPPORTED: bool = false;

        pub fn new(_parent: isize) -> Embedder {
            Embedder
        }

        pub fn embed(&self, _window: u64, _engine_pid: Option<u32>, _owned: bool) -> bool {
            false
        }

        pub fn release(&self) {}

        pub fn is_embedded(&self) -> bool {
            false
        }

        pub fn set_area(&self, _area: Option<ScreenArea>, _scale: f64) {}

        pub fn set_scale(&self, _scale: f64) {}
    }
}

#[cfg(windows)]
mod windows_impl {
    use super::ScreenArea;
    use std::sync::Mutex;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{BLACK_BRUSH, GetStockObject, HBRUSH};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows::Win32::UI::WindowsAndMessaging::*;
    use windows::core::BOOL;
    use windows::core::w;

    /// The host window's class: a black box the Engine window fills.
    const HOST_CLASS: windows::core::PCWSTR = w!("TizenSimScreen");

    /// The host's timer that keeps the page fitted to it.
    const REFIT_TIMER: usize = 1;
    const REFIT_EVERY_MS: u32 = 250;

    /// Where the host keeps the page's offset in the Engine window from
    /// when that was its own window: its window extra bytes.
    const OWN_OFFSET: WINDOW_LONG_PTR_INDEX = WINDOW_LONG_PTR_INDEX(0);

    pub struct Embedder {
        parent: isize,
        state: Mutex<State>,
    }

    #[derive(Default)]
    struct State {
        /// Created on first use, then reused for every Engine.
        host: Option<isize>,
        area: Option<ScreenArea>,
        scale: f64,
        engine: Option<Embedded>,
    }

    /// An Engine window in the host, with what it was like before.
    struct Embedded {
        window: isize,
        /// Whether the GUI owns the Session; an adopted Session's window is
        /// given back as its own window when the GUI lets go of it.
        owned: bool,
        style: i32,
        ex_style: i32,
        rect: RECT,
    }

    fn hwnd(handle: isize) -> HWND {
        HWND(handle as *mut _)
    }

    impl Embedder {
        pub const SUPPORTED: bool = true;

        /// An embedder for the GUI window `parent` (its `HWND`).
        pub fn new(parent: isize) -> Embedder {
            Embedder {
                parent,
                state: Mutex::new(State {
                    scale: 1.0,
                    ..State::default()
                }),
            }
        }

        /// Moves the Engine window `window` into the screen area and gives
        /// it keyboard focus. Any window embedded before is released first.
        /// `false` if `window` is gone, or isn't `engine_pid`'s (a window
        /// handle can be reused once its window is destroyed).
        pub fn embed(&self, window: u64, engine_pid: Option<u32>, owned: bool) -> bool {
            self.release();
            let window = window as isize;
            let target = hwnd(window);
            unsafe {
                if !IsWindow(Some(target)).as_bool() {
                    return false;
                }
                if let Some(pid) = engine_pid {
                    let mut owner = 0;
                    GetWindowThreadProcessId(target, Some(&mut owner));
                    if owner != pid {
                        return false;
                    }
                }
            }
            let mut state = self.state.lock().unwrap();
            let Some(host) = self.host(&mut state) else {
                return false;
            };
            unsafe {
                let style = GetWindowLongW(target, GWL_STYLE);
                let ex_style = GetWindowLongW(target, GWL_EXSTYLE);
                let mut rect = RECT::default();
                let _ = GetWindowRect(target, &mut rect);
                // A hidden App's window is minimized; a child can't be.
                if IsIconic(target).as_bool() {
                    let _ = ShowWindow(target, SW_RESTORE);
                }
                // Where the page is while it is still its own window, before
                // a native frame goes with the restyling below.
                let own = page_offset(target);
                remember_own_offset(hwnd(host), own);
                let frame = WS_POPUP
                    | WS_CAPTION
                    | WS_THICKFRAME
                    | WS_SYSMENU
                    | WS_MINIMIZEBOX
                    | WS_MAXIMIZEBOX;
                let child = (WINDOW_STYLE(style as u32) & !frame) | WS_CHILD | WS_VISIBLE;
                SetWindowLongW(target, GWL_STYLE, child.0 as i32);
                let ex_frame = WS_EX_APPWINDOW
                    | WS_EX_WINDOWEDGE
                    | WS_EX_CLIENTEDGE
                    | WS_EX_DLGMODALFRAME
                    | WS_EX_TOPMOST;
                let child_ex = WINDOW_EX_STYLE(ex_style as u32) & !ex_frame;
                SetWindowLongW(target, GWL_EXSTYLE, child_ex.0 as i32);
                if SetParent(target, Some(hwnd(host))).is_err() {
                    SetWindowLongW(target, GWL_STYLE, style);
                    SetWindowLongW(target, GWL_EXSTYLE, ex_style);
                    return false;
                }
                state.engine = Some(Embedded {
                    window,
                    owned,
                    style,
                    ex_style,
                    rect,
                });
                place(host, &state);
                fill(hwnd(host));
                SetTimer(Some(hwnd(host)), REFIT_TIMER, REFIT_EVERY_MS, None);
                focus_child(hwnd(host));
            }
            true
        }

        /// Lets go of the embedded Engine window, if any. An adopted
        /// Session's window becomes its own window again, where it was; an
        /// owned Session's is left to close with its Engine.
        pub fn release(&self) {
            let mut state = self.state.lock().unwrap();
            let Some(engine) = state.engine.take() else {
                return;
            };
            let host = state.host.expect("an embedded window has a host");
            unsafe {
                let target = hwnd(engine.window);
                let ours = IsWindow(Some(target)).as_bool()
                    && GetParent(target).is_ok_and(|p| p == hwnd(host));
                if ours && !engine.owned {
                    let _ = ShowWindow(target, SW_HIDE);
                    let _ = SetParent(target, None);
                    SetWindowLongW(target, GWL_STYLE, engine.style);
                    SetWindowLongW(target, GWL_EXSTYLE, engine.ex_style);
                    let r = engine.rect;
                    let _ = SetWindowPos(
                        target,
                        None,
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        SWP_NOZORDER | SWP_FRAMECHANGED | SWP_SHOWWINDOW,
                    );
                }
                let _ = KillTimer(Some(hwnd(host)), REFIT_TIMER);
                let _ = ShowWindow(hwnd(host), SW_HIDE);
            }
        }

        /// Whether an Engine window is in the host. Safe on any thread.
        pub fn is_embedded(&self) -> bool {
            self.state.lock().unwrap().engine.is_some()
        }

        /// Where the screen area is now (`None` while the GUI shows none),
        /// at `scale` physical pixels per CSS px.
        pub fn set_area(&self, area: Option<ScreenArea>, scale: f64) {
            let mut state = self.state.lock().unwrap();
            state.area = area;
            state.scale = scale;
            if let Some(host) = state.host {
                place(host, &state);
            }
        }

        /// The GUI window's new DPI scale, e.g. after moving to another monitor.
        pub fn set_scale(&self, scale: f64) {
            let area = self.state.lock().unwrap().area;
            self.set_area(area, scale);
        }

        fn host(&self, state: &mut State) -> Option<isize> {
            if let Some(host) = state.host {
                return Some(host);
            }
            let host = unsafe { create_host(hwnd(self.parent)) }?;
            state.host = Some(host);
            Some(host)
        }
    }

    impl Drop for Embedder {
        fn drop(&mut self) {
            self.release();
        }
    }

    /// Shows the host over the TV screen's place in the screen area, above
    /// the GUI's webview, while it holds an Engine window and the GUI shows
    /// a screen area.
    fn place(host: isize, state: &State) {
        let host = hwnd(host);
        let area = state
            .area
            .map(|a| a.to_physical(state.scale).fit_tv_screen());
        unsafe {
            match (area, &state.engine) {
                (Some(b), Some(_)) if b.width > 0 && b.height > 0 => {
                    let _ = SetWindowPos(
                        host,
                        Some(HWND_TOP),
                        b.x,
                        b.y,
                        b.width,
                        b.height,
                        SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                }
                _ => {
                    let _ = ShowWindow(host, SW_HIDE);
                }
            }
        }
    }

    /// Places the host's Engine window so that the page fills the host.
    /// Chromium draws its own title bar and info bars (the "Chrome for
    /// Testing" notice) inside its window, so the window is made larger by
    /// those and moved up and left past them, where the host clips them.
    unsafe fn fill(host: HWND) {
        unsafe {
            let Ok(engine) = GetWindow(host, GW_CHILD) else {
                return;
            };
            let mut client = RECT::default();
            let _ = GetClientRect(host, &mut client);
            // Chromium's window can be without one for a while, e.g. while
            // it is too small to draw in: the last one measured still holds.
            let (left, top) = match page_offset(engine) {
                Some(offset) => {
                    remember_offset(host, offset);
                    offset
                }
                None => remembered_offset(host),
            };
            // The Session fits the App to the window's size less the frame
            // it had as its own window (`tizen_sim_core::screen`), so the
            // window keeps that frame around the host, even where it is a
            // native frame that has gone (old Engines). The frame is as wide
            // on the right and bottom as on the left; only the top has more,
            // the title and info bars.
            let (own_left, own_top) = remembered_own_offset(host).unwrap_or((left, top));
            let _ = SetWindowPos(
                engine,
                None,
                -left,
                -top,
                client.right + 2 * own_left,
                client.bottom + own_top + own_left,
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED | SWP_SHOWWINDOW,
            );
        }
    }

    /// Where the page's top-left corner is in the Engine window. `None`
    /// while Chromium has no window for the page. Only the corner counts:
    /// Chromium resizes the page's window some time after its own.
    unsafe fn page_offset(engine: HWND) -> Option<(i32, i32)> {
        unsafe {
            let page = page_window(engine)?;
            let (mut outer, mut inner) = (RECT::default(), RECT::default());
            GetWindowRect(engine, &mut outer).ok()?;
            GetWindowRect(page, &mut inner).ok()?;
            let offset = (inner.left - outer.left, inner.top - outer.top);
            (offset.0 >= 0 && offset.1 >= 0).then_some(offset)
        }
    }

    /// The host keeps where the page was in the Engine window while that was
    /// its own window, `None` if it had no page window then.
    unsafe fn remember_own_offset(host: HWND, offset: Option<(i32, i32)>) {
        let packed = offset.map_or(0, |(left, top)| {
            (1 << 32) | ((top as u16 as isize) << 16) | (left as u16 as isize)
        });
        unsafe { SetWindowLongPtrW(host, OWN_OFFSET, packed) };
    }

    unsafe fn remembered_own_offset(host: HWND) -> Option<(i32, i32)> {
        let packed = unsafe { GetWindowLongPtrW(host, OWN_OFFSET) };
        (packed >> 32 != 0).then_some(((packed & 0xffff) as i32, ((packed >> 16) & 0xffff) as i32))
    }

    /// The host keeps the last page offset in its user data.
    unsafe fn remember_offset(host: HWND, (left, top): (i32, i32)) {
        let packed = ((top as u16 as isize) << 16) | (left as u16 as isize);
        unsafe { SetWindowLongPtrW(host, GWLP_USERDATA, packed) };
    }

    unsafe fn remembered_offset(host: HWND) -> (i32, i32) {
        let packed = unsafe { GetWindowLongPtrW(host, GWLP_USERDATA) };
        ((packed & 0xffff) as i32, ((packed >> 16) & 0xffff) as i32)
    }

    /// The window Chromium draws the page in (kept on Windows for
    /// accessibility, Engines old and new alike).
    unsafe fn page_window(engine: HWND) -> Option<HWND> {
        unsafe extern "system" fn visit(window: HWND, found: LPARAM) -> BOOL {
            let mut class = [0u16; 64];
            let n = unsafe { GetClassNameW(window, &mut class) } as usize;
            if String::from_utf16_lossy(&class[..n]) == "Chrome_RenderWidgetHostHWND"
                && unsafe { IsWindowVisible(window) }.as_bool()
            {
                unsafe { *(found.0 as *mut Option<HWND>) = Some(window) };
                return false.into();
            }
            true.into()
        }
        let mut found: Option<HWND> = None;
        unsafe {
            let _ = EnumChildWindows(
                Some(engine),
                Some(visit),
                LPARAM(&mut found as *mut _ as isize),
            );
        }
        found
    }

    /// Whether the page's corner is no longer the host's, e.g. because an
    /// info bar closed or Chromium only now made the page's window.
    unsafe fn misfit(host: HWND) -> bool {
        unsafe {
            let Some(page) = GetWindow(host, GW_CHILD).ok().and_then(|e| page_window(e)) else {
                return false;
            };
            let (mut outer, mut inner) = (RECT::default(), RECT::default());
            let _ = GetWindowRect(host, &mut outer);
            let _ = GetWindowRect(page, &mut inner);
            (outer.left, outer.top) != (inner.left, inner.top)
        }
    }

    /// Gives the Engine window keyboard focus. It runs on another thread of
    /// another process, so this thread's input is attached to its for the
    /// call.
    unsafe fn focus_child(host: HWND) {
        unsafe {
            let Ok(child) = GetWindow(host, GW_CHILD) else {
                return;
            };
            let ours = GetCurrentThreadId();
            let theirs = GetWindowThreadProcessId(child, None);
            let attached = ours != theirs && AttachThreadInput(ours, theirs, true).as_bool();
            let _ = SetFocus(Some(child));
            if attached {
                let _ = AttachThreadInput(ours, theirs, false);
            }
        }
    }

    unsafe fn create_host(parent: HWND) -> Option<isize> {
        unsafe {
            let instance = GetModuleHandleW(None).ok()?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(host_proc),
                hInstance: instance.into(),
                lpszClassName: HOST_CLASS,
                hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                cbWndExtra: std::mem::size_of::<isize>() as i32,
                ..Default::default()
            };
            // Fails harmlessly once the class exists.
            RegisterClassW(&class);
            let host = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                HOST_CLASS,
                w!("TV screen"),
                WS_CHILD | WS_CLIPCHILDREN,
                0,
                0,
                0,
                0,
                Some(parent),
                None,
                Some(instance.into()),
                None,
            )
            .ok()?;
            Some(host.0 as isize)
        }
    }

    unsafe extern "system" fn host_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe {
            match message {
                WM_SIZE => {
                    fill(window);
                    LRESULT(0)
                }
                // Chromium lays its window out on its own thread, in its
                // own time: check now and then that the page still fits.
                WM_TIMER if wparam.0 == REFIT_TIMER => {
                    if misfit(window) {
                        fill(window);
                    }
                    LRESULT(0)
                }
                // A click anywhere in the Engine window: Windows activates
                // the GUI's window, and the Engine must get the keys.
                WM_PARENTNOTIFY
                    if matches!(
                        (wparam.0 & 0xffff) as u32,
                        WM_LBUTTONDOWN | WM_MBUTTONDOWN | WM_RBUTTONDOWN | WM_XBUTTONDOWN
                    ) =>
                {
                    focus_child(window);
                    LRESULT(0)
                }
                WM_SETFOCUS | WM_MOUSEACTIVATE => {
                    focus_child(window);
                    DefWindowProcW(window, message, wparam, lparam)
                }
                _ => DefWindowProcW(window, message, wparam, lparam),
            }
        }
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    //! The Embedder on real windows of this process: stand-ins for the
    //! GUI's window and the Engine's.

    use super::*;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::MapWindowPoints;
    use windows::Win32::UI::WindowsAndMessaging::*;
    use windows::core::{PCWSTR, w};

    struct Window(HWND);

    impl Drop for Window {
        fn drop(&mut self) {
            unsafe {
                let _ = DestroyWindow(self.0);
            }
        }
    }

    fn top_level(title: PCWSTR, x: i32) -> Window {
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                title,
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                x,
                100,
                800,
                600,
                None,
                None,
                None,
                None,
            )
        };
        Window(window.unwrap())
    }

    impl Window {
        fn handle(&self) -> u64 {
            self.0.0 as u64
        }

        fn parent_handle(&self) -> isize {
            self.0.0 as isize
        }

        fn style(&self) -> WINDOW_STYLE {
            WINDOW_STYLE(unsafe { GetWindowLongW(self.0, GWL_STYLE) } as u32)
        }

        fn rect(&self) -> RECT {
            let mut rect = RECT::default();
            unsafe { GetWindowRect(self.0, &mut rect).unwrap() };
            rect
        }

        fn is_top_level(&self) -> bool {
            unsafe { GetParent(self.0) }.is_err()
        }
    }

    fn client_size(window: HWND) -> (i32, i32) {
        let mut rect = RECT::default();
        unsafe { GetClientRect(window, &mut rect).unwrap() };
        (rect.right, rect.bottom)
    }

    /// The window `window` is a child of, and its box in that parent's
    /// client area as (left, top, right, bottom).
    fn placement(window: HWND) -> (HWND, (i32, i32, i32, i32)) {
        unsafe {
            let parent = GetParent(window).unwrap();
            let mut rect = RECT::default();
            GetWindowRect(window, &mut rect).unwrap();
            let mut points = [
                POINT {
                    x: rect.left,
                    y: rect.top,
                },
                POINT {
                    x: rect.right,
                    y: rect.bottom,
                },
            ];
            MapWindowPoints(None, Some(parent), &mut points);
            (parent, (points[0].x, points[0].y, points[1].x, points[1].y))
        }
    }

    const AREA: ScreenArea = ScreenArea {
        x: 200.0,
        y: 40.0,
        width: 480.0,
        height: 270.0,
    };

    #[test]
    fn an_embedded_window_fills_the_screen_area_without_its_frame() {
        let gui = top_level(w!("gui"), 100);
        let engine = top_level(w!("engine"), 300);
        let embedder = Embedder::new(gui.parent_handle());
        embedder.set_area(Some(AREA), 1.5);

        assert!(embedder.embed(engine.handle(), Some(std::process::id()), true));

        let (host, in_host) = placement(engine.0);
        let (parent, area) = placement(host);
        assert_eq!(parent, gui.0);
        assert_eq!(area, (300, 60, 300 + 720, 60 + 405));
        assert_eq!((in_host.0, in_host.1), (0, 0));
        assert_eq!(client_size(engine.0), (720, 405));
        assert!(engine.style().contains(WS_CHILD));
        assert!(!engine.style().contains(WS_CAPTION));
        assert!(!engine.style().contains(WS_THICKFRAME));
    }

    #[test]
    fn the_engine_window_is_centered_at_16_9_in_a_wider_area() {
        let gui = top_level(w!("gui"), 100);
        let engine = top_level(w!("engine"), 300);
        let embedder = Embedder::new(gui.parent_handle());
        embedder.set_area(
            Some(ScreenArea {
                width: 1000.0,
                ..AREA
            }),
            1.0,
        );

        assert!(embedder.embed(engine.handle(), None, true));

        let (host, _) = placement(engine.0);
        let (_, area) = placement(host);
        assert_eq!(area, (200 + 260, 40, 200 + 260 + 480, 40 + 270));
        assert_eq!(client_size(engine.0), (480, 270));
    }

    fn screen_rect(window: HWND) -> RECT {
        let mut rect = RECT::default();
        unsafe { GetWindowRect(window, &mut rect).unwrap() };
        rect
    }

    /// Where the stand-in Chromium window draws the page: below the title
    /// bar and the "Chrome for Testing" info bar it draws itself.
    const PAGE_TOP: i32 = 70;

    /// A stand-in for a Chromium window: the page goes in a child window
    /// laid out below its own title bar and info bars, following its size.
    fn chromium_window(x: i32) -> (Window, HWND) {
        unsafe extern "system" fn frame(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
            unsafe {
                if m == WM_SIZE
                    && let Ok(page) = GetWindow(h, GW_CHILD)
                {
                    let (width, height) = ((l.0 & 0xffff) as i32, ((l.0 >> 16) & 0xffff) as i32);
                    let _ = MoveWindow(page, 0, PAGE_TOP, width, height - PAGE_TOP, false);
                }
                DefWindowProcW(h, m, w, l)
            }
        }
        unsafe extern "system" fn page(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
            unsafe { DefWindowProcW(h, m, w, l) }
        }
        unsafe {
            for (name, proc) in [
                (w!("FakeChromiumFrame"), frame as _),
                (w!("Chrome_RenderWidgetHostHWND"), page as _),
            ] {
                RegisterClassW(&WNDCLASSW {
                    lpfnWndProc: Some(proc),
                    lpszClassName: name,
                    ..Default::default()
                });
            }
            let window = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("FakeChromiumFrame"),
                w!("engine"),
                WS_POPUP | WS_VISIBLE,
                x,
                100,
                800,
                600,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let content = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("Chrome_RenderWidgetHostHWND"),
                w!("page"),
                WS_CHILD | WS_VISIBLE,
                0,
                PAGE_TOP,
                800,
                600 - PAGE_TOP,
                Some(window),
                None,
                None,
                None,
            )
            .unwrap();
            (Window(window), content)
        }
    }

    #[test]
    fn only_the_page_fills_the_screen_area_not_the_engines_title_bar() {
        let gui = top_level(w!("gui"), 100);
        let (engine, page) = chromium_window(300);
        let embedder = Embedder::new(gui.parent_handle());
        embedder.set_area(Some(AREA), 1.0);

        assert!(embedder.embed(engine.handle(), None, true));

        let (host, _) = placement(engine.0);
        assert_eq!(screen_rect(page), screen_rect(host));
    }

    /// A stand-in for an old Engine's window, with a native frame: the page
    /// fills the client area below the native title bar.
    fn natively_framed_chromium_window(x: i32) -> (Window, HWND) {
        unsafe extern "system" fn frame(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
            unsafe {
                if m == WM_SIZE
                    && let Ok(page) = GetWindow(h, GW_CHILD)
                {
                    let (width, height) = ((l.0 & 0xffff) as i32, ((l.0 >> 16) & 0xffff) as i32);
                    let _ = MoveWindow(page, 0, 0, width, height, false);
                }
                DefWindowProcW(h, m, w, l)
            }
        }
        unsafe {
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(frame),
                lpszClassName: w!("FakeNativeChromiumFrame"),
                ..Default::default()
            });
            let window = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("FakeNativeChromiumFrame"),
                w!("engine"),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                x,
                100,
                800,
                600,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let (width, height) = client_size(window);
            // The page's window class is registered by `chromium_window`.
            let _ = chromium_window(0);
            let content = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("Chrome_RenderWidgetHostHWND"),
                w!("page"),
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                width,
                height,
                Some(window),
                None,
                None,
                None,
            )
            .unwrap();
            (Window(window), content)
        }
    }

    fn size(rect: RECT) -> (i32, i32) {
        (rect.right - rect.left, rect.bottom - rect.top)
    }

    #[test]
    fn a_native_frame_the_engine_window_loses_still_counts_in_its_size() {
        // The Session fits the App to the Engine window's size less its
        // frame, as measured while it was its own window
        // (`tizen_sim_core::screen`); a native frame goes when embedded.
        let gui = top_level(w!("gui"), 100);
        let (engine, page) = natively_framed_chromium_window(300);
        let (window, content) = (size(screen_rect(engine.0)), size(screen_rect(page)));
        let frame = (window.0 - content.0, window.1 - content.1);
        assert!(frame.1 > 0, "no native frame to lose");
        let embedder = Embedder::new(gui.parent_handle());
        embedder.set_area(Some(AREA), 1.0);

        assert!(embedder.embed(engine.handle(), None, true));

        let (host, _) = placement(engine.0);
        let window = size(screen_rect(engine.0));
        assert_eq!((window.0 - frame.0, window.1 - frame.1), client_size(host));
        let (page, host) = (screen_rect(page), screen_rect(host));
        assert_eq!((page.left, page.top), (host.left, host.top));
    }

    #[test]
    fn the_engine_window_follows_the_screen_area_and_the_dpi() {
        let gui = top_level(w!("gui"), 100);
        let engine = top_level(w!("engine"), 300);
        let embedder = Embedder::new(gui.parent_handle());
        embedder.set_area(Some(AREA), 1.0);
        assert!(embedder.embed(engine.handle(), None, true));

        embedder.set_area(
            Some(ScreenArea {
                width: 640.0,
                height: 360.0,
                ..AREA
            }),
            1.0,
        );
        assert_eq!(client_size(engine.0), (640, 360));

        embedder.set_scale(2.0);
        assert_eq!(client_size(engine.0), (1280, 720));
        let (host, _) = placement(engine.0);
        let (_, area) = placement(host);
        assert_eq!((area.0, area.1), (400, 80));
    }

    #[test]
    fn without_a_screen_area_the_engine_window_is_hidden() {
        let gui = top_level(w!("gui"), 100);
        let engine = top_level(w!("engine"), 300);
        let embedder = Embedder::new(gui.parent_handle());
        assert!(embedder.embed(engine.handle(), None, true));
        let (host, _) = placement(engine.0);
        let host_shown = || unsafe { IsWindowVisible(host) }.as_bool();
        assert!(!host_shown());

        embedder.set_area(Some(AREA), 1.0);
        assert!(host_shown());

        embedder.set_area(None, 1.0);
        assert!(!host_shown());
    }

    #[test]
    fn an_adopted_window_is_given_back_as_it_was() {
        let gui = top_level(w!("gui"), 100);
        let engine = top_level(w!("engine"), 300);
        let (style, rect) = (engine.style(), engine.rect());
        let embedder = Embedder::new(gui.parent_handle());
        embedder.set_area(Some(AREA), 1.0);
        assert!(embedder.embed(engine.handle(), None, false));

        embedder.release();

        assert!(engine.is_top_level());
        assert_eq!(engine.style(), style);
        assert_eq!(engine.rect(), rect);
    }

    #[test]
    fn an_owned_window_is_left_to_close_with_its_engine() {
        let gui = top_level(w!("gui"), 100);
        let engine = top_level(w!("engine"), 300);
        let embedder = Embedder::new(gui.parent_handle());
        embedder.set_area(Some(AREA), 1.0);
        assert!(embedder.embed(engine.handle(), None, true));

        embedder.release();

        // No flash of a top-level window just before the Engine closes.
        assert!(!engine.is_top_level());
    }

    #[test]
    fn embedding_another_window_releases_the_first() {
        let gui = top_level(w!("gui"), 100);
        let first = top_level(w!("first"), 300);
        let second = top_level(w!("second"), 400);
        let embedder = Embedder::new(gui.parent_handle());
        embedder.set_area(Some(AREA), 1.0);
        assert!(embedder.embed(first.handle(), None, false));

        assert!(embedder.embed(second.handle(), None, false));

        assert!(first.is_top_level());
        assert!(second.style().contains(WS_CHILD));
    }

    #[test]
    fn a_window_that_is_gone_or_not_the_engines_is_not_embedded() {
        let gui = top_level(w!("gui"), 100);
        let embedder = Embedder::new(gui.parent_handle());
        let engine = top_level(w!("engine"), 300);

        assert!(!embedder.embed(engine.handle(), Some(std::process::id() + 1), false));
        assert!(engine.is_top_level());

        let gone = engine.handle();
        drop(engine);
        assert!(!embedder.embed(gone, None, false));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_screen_area_is_scaled_to_physical_pixels() {
        let area = ScreenArea {
            x: 240.0,
            y: 49.0,
            width: 960.0,
            height: 540.0,
        };
        assert_eq!(
            area.to_physical(1.0),
            PhysicalBox {
                x: 240,
                y: 49,
                width: 960,
                height: 540
            }
        );
        assert_eq!(
            area.to_physical(1.5),
            PhysicalBox {
                x: 360,
                y: 74,
                width: 1440,
                height: 810
            }
        );
    }

    #[test]
    fn the_tv_screen_is_the_largest_16_9_box_centered_in_the_area() {
        let wide = PhysicalBox {
            x: 10,
            y: 20,
            width: 1000,
            height: 360,
        };
        assert_eq!(
            wide.fit_tv_screen(),
            PhysicalBox {
                x: 190,
                y: 20,
                width: 640,
                height: 360
            }
        );
        let tall = PhysicalBox {
            x: 0,
            y: 0,
            width: 640,
            height: 600,
        };
        assert_eq!(
            tall.fit_tv_screen(),
            PhysicalBox {
                x: 0,
                y: 120,
                width: 640,
                height: 360
            }
        );
    }

    #[test]
    fn a_tv_screen_that_almost_fits_rounds_to_the_area() {
        // 563 * 16 / 9 = 1000.9, so the whole width.
        let area = PhysicalBox {
            x: 0,
            y: 0,
            width: 1001,
            height: 563,
        };
        assert_eq!(area.fit_tv_screen(), area);
        let empty = PhysicalBox {
            x: 5,
            y: 5,
            width: 0,
            height: 300,
        };
        assert_eq!(
            empty.fit_tv_screen(),
            PhysicalBox {
                x: 5,
                y: 155,
                width: 0,
                height: 0
            }
        );
    }

    #[test]
    fn fractional_areas_round_their_edges() {
        let area = ScreenArea {
            x: 10.4,
            y: 0.0,
            width: 100.3,
            height: 0.2,
        };
        // Left edge 13, right edge round(138.375) = 138.
        assert_eq!(
            area.to_physical(1.25),
            PhysicalBox {
                x: 13,
                y: 0,
                width: 125,
                height: 0
            }
        );
    }
}
