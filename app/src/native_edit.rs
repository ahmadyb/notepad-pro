//! Optional native editor surface: a Win32 Rich Edit control (RICHEDIT50W,
//! the WordPad engine) parented inside the Slint window, exactly where the
//! Slint editor would sit.
//!
//! Why: the user asked for the behaviour of a real Windows text control.
//! Rich Edit gives it for free — unlimited Enter, native caret/selection/
//! clipboard/undo, horizontal + vertical scrollbars, bullet/number paragraph
//! formatting — while Rust stays the source of truth: every native edit
//! arrives through `EN_CHANGE`, is reconciled into the line model by the same
//! `apply_surface_text` path the Slint surface uses, and Rust mutations
//! (open, undo, replace-all, highlights) flow back with `WM_SETTEXT` +
//! char/para format messages.
//!
//! The Slint `TextInput` surface remains as an automatic fallback if the
//! attach ever fails (non-Windows builds always use it).
//!
//! All raw Win32 structs below are declared locally (`#[repr(C)]`) so the
//! module does not depend on RichEdit type paths in the `windows` crate.

#[cfg(not(windows))]
mod imp {
    use crate::callbacks::SharedState;
    use crate::ui::AppWindow;
    pub fn start_attach(_window: &AppWindow, _state: &SharedState) {}
    pub fn attached() -> bool {
        false
    }
    pub fn poll(_window: &AppWindow, _state: &SharedState) {}
}

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, OnceLock};

    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, TRUE, WPARAM};
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, LoadLibraryW};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, SetFocus, VIRTUAL_KEY};
    use windows::Win32::UI::WindowsAndMessaging::*;

    /// `NMHDR` declared locally: the notification header of `WM_NOTIFY`.
    #[repr(C)]
    struct Nmhdr {
        hwnd_from: usize,
        id_from: usize,
        code: isize,
    }

    use crate::callbacks::window_cb;
    use crate::callbacks::{lock, SharedState};
    use crate::sync;
    use crate::ui::AppWindow;
    use notepad_pro_core::types::line::ListType;

    // ── raw message / struct constants (richedit.h / winuser.h) ──────────
    const WM_USER: u32 = 0x0400;
    const EM_EXGETSEL: u32 = WM_USER + 52;
    const EM_EXLINEFROMCHAR: u32 = WM_USER + 54;
    const EM_EXSETSEL: u32 = WM_USER + 55;
    const EM_LINEINDEX: u32 = 0x00BB;
    const EM_SCROLLCARET: u32 = 0x00B7;
    const EM_SETBKGNDCOLOR: u32 = WM_USER + 67;
    const EM_SETCHARFORMAT: u32 = WM_USER + 68;
    const EM_SETEVENTMASK: u32 = WM_USER + 69;
    const EM_SETPARAFORMAT: u32 = WM_USER + 71;
    const EM_SETTARGETDEVICE: u32 = WM_USER + 75;

    const SCF_SELECTION: usize = 0x0001;
    const SCF_ALL: usize = 0x0004;
    const CFM_SIZE: u32 = 0x8000_0000;
    const CFM_COLOR: u32 = 0x4000_0000;
    const CFM_FACE: u32 = 0x2000_0000;
    const CFM_BACKCOLOR: u32 = 0x0400_0000;

    const PFM_STARTINDENT: u32 = 0x0001;
    const PFM_OFFSET: u32 = 0x0004;
    const PFM_NUMBERING: u32 = 0x0020;
    const PFN_BULLET: u16 = 0x0001;
    const PFN_ARABIC: u16 = 0x0002;

    const EN_CHANGE: u32 = 0x0300;
    const EN_SELCHANGE: isize = 0x0700;
    const VK_SHIFT: u16 = 0x10;
    const VK_CONTROL: u16 = 0x11;
    const VK_TAB: u16 = 0x09;
    const ENM_CHANGE: isize = 0x0001;
    const ENM_SELCHANGE: isize = 0x0008;

    const ES_MULTILINE: u32 = 0x0004;
    const ES_AUTOVSCROLL: u32 = 0x0040;
    const ES_AUTOHSCROLL: u32 = 0x0080;
    const ES_NOHIDESEL: u32 = 0x0100;
    const ES_SAVESEL: u32 = 0x8000;

    const UNCHECKED_PREFIX: &str = "☐ ";
    const CHECKED_PREFIX: &str = "☑ ";

    #[repr(C)]
    struct CharFormat2W {
        cb_size: u32,
        dw_mask: u32,
        dw_effects: u32,
        y_height: i32,
        y_offset: i32,
        cr_text_color: u32,
        b_char_set: u8,
        b_pitch_and_family: u8,
        sz_face_name: [u16; 32],
        w_weight: u16,
        s_spacing: i16,
        cr_back_color: u32,
        lcid: u32,
        dw_reserved: u32,
        s_style: i16,
        w_kerning: u16,
        b_underline_type: u8,
        b_animation: u8,
        b_rev_author: u8,
        b_reserved1: u8,
    }

    #[repr(C)]
    struct ParaFormat2W {
        cb_size: u32,
        dw_mask: u32,
        w_numbering: u16,
        w_reserved: u16,
        dx_start_indent: i32,
        dx_right_indent: i32,
        dx_offset: i32,
        w_alignment: u16,
        c_tab_count: u8,
        b_reserved: u8,
        rgx_tabs: [i32; 32],
        dy_space_before: i32,
        dy_space_after: i32,
        dy_line_spacing: i32,
        s_style: i16,
    }

    #[repr(C)]
    struct CharRange {
        cp_min: i32,
        cp_max: i32,
    }

    struct Native {
        /// Raw value of the child `HWND` (kept as `isize` so the static is
        /// `Send`/`Sync`).
        edit: isize,
        rect: (i32, i32, i32, i32),
        font_key: (String, i32),
        wrap: Option<bool>,
        theme: String,
        style_sig: String,
    }

    static NATIVE: OnceLock<Mutex<Option<Native>>> = OnceLock::new();
    static OLD_PARENT_PROC: std::sync::atomic::AtomicIsize =
        std::sync::atomic::AtomicIsize::new(0);
    static OLD_EDIT_PROC: std::sync::atomic::AtomicIsize =
        std::sync::atomic::AtomicIsize::new(0);
    static PENDING_EDIT: AtomicBool = AtomicBool::new(false);
    static PENDING_SEL: AtomicBool = AtomicBool::new(false);
    static SUPPRESS: AtomicBool = AtomicBool::new(false);

    // The subclass procs run on the UI thread; hand them the live handles.
    // `AppWindow` is !Send, so this must be thread-local, not a static.
    thread_local! {
        static CONTEXT: std::cell::RefCell<Option<(AppWindow, SharedState)>> =
            std::cell::RefCell::new(None);
    }

    fn slot() -> &'static Mutex<Option<Native>> {
        NATIVE.get_or_init(|| Mutex::new(None))
    }

    pub fn attached() -> bool {
        slot().lock().unwrap().is_some()
    }

    // ── attach ───────────────────────────────────────────────────────────

    extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let mut buf = [0u16; 64];
        let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
        if n > 0 {
            let t = String::from_utf16_lossy(&buf[..n as usize]);
            if t.starts_with("NotePad Pro") {
                unsafe { *(lparam.0 as *mut HWND) = hwnd };
                return BOOL(0);
            }
        }
        BOOL(1)
    }

    fn find_app_window() -> Option<HWND> {
        let mut found = HWND::default();
        unsafe {
            let _ = EnumWindows(Some(enum_cb), LPARAM(&mut found as *mut HWND as isize));
        }
        if found == HWND::default() {
            None
        } else {
            Some(found)
        }
    }

    extern "system" fn parent_subclass(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_COMMAND => {
                if (wparam.0 >> 16) as u32 == EN_CHANGE && !SUPPRESS.load(Ordering::SeqCst) {
                    PENDING_EDIT.store(true, Ordering::SeqCst);
                }
            }
            WM_NOTIFY => {
                let nm = lparam.0 as *const Nmhdr;
                if !nm.is_null() && unsafe { (*nm).code } == EN_SELCHANGE {
                    PENDING_SEL.store(true, Ordering::SeqCst);
                }
            }
            _ => {}
        }
        call_old(OLD_PARENT_PROC.load(Ordering::SeqCst), hwnd, msg, wparam, lparam)
    }

    fn call_old(prev: isize, hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        let proc: WNDPROC = unsafe { std::mem::transmute(prev) };
        unsafe { CallWindowProcW(proc, hwnd, msg, wparam, lparam) }
    }

    extern "system" fn edit_subclass(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // Ctrl chords and Tab: route to the app's shortcut table instead of
        // letting Rich Edit swallow them (Ctrl+S, Ctrl+O, Ctrl+Shift+D, ...).
        if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
            let ctrl_down = unsafe { GetKeyState(VK_CONTROL as i32) } < 0;
            let shift_down = unsafe { GetKeyState(VK_SHIFT as i32) } < 0;
            let vk = VIRTUAL_KEY(wparam.0 as u16);
            let handled = CONTEXT.with(|c| {
                let b = c.borrow();
                let (window, state) = b.as_ref()?;
                if ctrl_down {
                    if let Some(ch) = vkey_to_char(vk) {
                        let text = ch.to_string();
                        if window_cb::handle_shortcut(window, state, &text, true, shift_down) {
                            return Some(LRESULT(0));
                        }
                    }
                } else if vk == VIRTUAL_KEY(VK_TAB) {
                    lock(state).indent(!shift_down);
                    sync::sync_all(window, &lock(state));
                    return Some(LRESULT(0));
                }
                None
            });
            if let Some(r) = handled {
                return r;
            }
        }
        call_old(OLD_EDIT_PROC.load(Ordering::SeqCst), hwnd, msg, wparam, lparam)
    }

    fn vkey_to_char(vk: VIRTUAL_KEY) -> Option<char> {
        match vk.0 {
            0x41..=0x5A => Some(char::from((vk.0 - 0x41 + b'a' as u16) as u8)),
            0x30..=0x39 => Some(char::from(vk.0 as u8)),
            _ => None,
        }
    }

    fn try_attach() -> bool {
        let Some(parent) = find_app_window() else {
            return false;
        };
        unsafe {
            let _lib = LoadLibraryW(w!("Msftedit.dll"));
            let hinst = GetModuleHandleW(None).unwrap_or_default();
            let style = WINDOW_STYLE(
                WS_CHILD.0
                    | WS_VISIBLE.0
                    | WS_VSCROLL.0
                    | WS_HSCROLL.0
                    | ES_MULTILINE
                    | ES_AUTOVSCROLL
                    | ES_AUTOHSCROLL
                    | ES_NOHIDESEL
                    | ES_SAVESEL,
            );
            let Ok(edit) = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("RICHEDIT50W"),
                w!(""),
                style,
                0,
                0,
                100,
                100,
                parent,
                None,
                hinst,
                None,
            ) else {
                return false;
            };
            let _ = SendMessageW(
                edit,
                EM_SETEVENTMASK,
                WPARAM(0),
                LPARAM(ENM_CHANGE | ENM_SELCHANGE),
            );
            OLD_PARENT_PROC.store(
                SetWindowLongPtrW(parent, GWLP_WNDPROC, parent_subclass as usize as isize),
                Ordering::SeqCst,
            );
            OLD_EDIT_PROC.store(
                SetWindowLongPtrW(edit, GWLP_WNDPROC, edit_subclass as usize as isize),
                Ordering::SeqCst,
            );
            let _ = SetFocus(edit);
            *slot().lock().unwrap() = Some(Native {
                edit: edit.0 as isize,
                rect: (0, 0, 0, 0),
                font_key: (String::new(), 0),
                wrap: None,
                theme: String::new(),
                style_sig: String::new(),
            });
        }
        true
    }

    // ── text transfer ─────────────────────────────────────────────────────

    fn get_rich_text(edit: HWND) -> String {
        unsafe {
            let len =
                SendMessageW(edit, WM_GETTEXTLENGTH, WPARAM(0), LPARAM(0)).0.max(0) as usize;
            let mut buf = vec![0u16; len + 1];
            let n = SendMessageW(
                edit,
                WM_GETTEXT,
                WPARAM(len + 1),
                LPARAM(buf.as_mut_ptr() as isize),
            )
            .0
            .max(0) as usize;
            String::from_utf16_lossy(&buf[..n.min(len)])
        }
    }

    fn rich_lines_from_state(state: &crate::AppState) -> Vec<String> {
        state
            .doc()
            .lines
            .iter()
            .map(|l| match l.list_type {
                ListType::Check => {
                    if l.checked {
                        format!("{CHECKED_PREFIX}{}", l.text)
                    } else {
                        format!("{UNCHECKED_PREFIX}{}", l.text)
                    }
                }
                _ => l.text.clone(),
            })
            .collect()
    }

    fn strip_check_prefix(line: &str) -> &str {
        line.strip_prefix(UNCHECKED_PREFIX)
            .or_else(|| line.strip_prefix(CHECKED_PREFIX))
            .unwrap_or(line)
    }

    fn model_text_from_rich(rich: &str) -> String {
        rich.split("\r\n")
            .map(|l| strip_check_prefix(l).to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn set_rich_text(edit: HWND, text: &str) {
        let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
        SUPPRESS.store(true, Ordering::SeqCst);
        unsafe {
            let _ = SetWindowTextW(edit, PCWSTR(wide.as_ptr()));
        }
        SUPPRESS.store(false, Ordering::SeqCst);
    }

    fn utf16_offset(lines: &[String], line: usize, col: usize) -> i32 {
        let mut off = 0usize;
        for l in lines.iter().take(line) {
            off += l.encode_utf16().count() + 2;
        }
        if let Some(l) = lines.get(line) {
            off += l.chars().take(col).map(|c| c.len_utf16()).sum::<usize>();
        }
        off as i32
    }

    fn select_range(edit: HWND, from: i32, to: i32) {
        let mut cr = CharRange {
            cp_min: from,
            cp_max: to,
        };
        unsafe {
            let _ = SendMessageW(
                edit,
                EM_EXSETSEL,
                WPARAM(0),
                LPARAM(&mut cr as *mut _ as isize),
            );
        }
    }

    // ── styling ───────────────────────────────────────────────────────────

    fn colorref(hex: &str) -> u32 {
        let h = hex.trim_start_matches('#');
        let r = u32::from_str_radix(&h[0..2], 16).unwrap_or(255);
        let g = u32::from_str_radix(&h[2..4], 16).unwrap_or(255);
        let b = u32::from_str_radix(&h[4..6], 16).unwrap_or(255);
        r | (g << 8) | (b << 16)
    }

    fn theme_colours(theme: &str) -> (u32, u32) {
        // (editor bg, editor text) as COLORREF.
        if theme == "dark" {
            (colorref("#1a1d2e"), colorref("#e8eaf0"))
        } else {
            (colorref("#ffffff"), colorref("#1b1f2a"))
        }
    }

    fn apply_theme_and_font(n: &mut Native, window: &AppWindow, theme: &str) {
        let edit = HWND(n.edit as *mut _);
        let (bg, fg) = theme_colours(theme);
        unsafe {
            // wParam=1 keeps the current selection colour logic simple: the
            // message sets the control background directly.
            let _ = SendMessageW(edit, EM_SETBKGNDCOLOR, WPARAM(1), LPARAM(bg as isize));
            let mut cf = std::mem::zeroed::<CharFormat2W>();
            cf.cb_size = std::mem::size_of::<CharFormat2W>() as u32;
            cf.dw_mask = CFM_COLOR | CFM_BACKCOLOR | CFM_SIZE | CFM_FACE;
            cf.cr_text_color = fg;
            cf.cr_back_color = bg;
            let px = (window.get_base_font_size() * window.get_zoom()).round();
            cf.y_height = (px * 15.0) as i32; // twips: px * 72/96 * 20
            let face = window.get_font_family().to_string();
            let mut wide = [0u16; 32];
            for (i, c) in face.encode_utf16().take(31).enumerate() {
                wide[i] = c;
            }
            cf.sz_face_name = wide;
            let _ = SendMessageW(
                edit,
                EM_SETCHARFORMAT,
                WPARAM(SCF_ALL),
                LPARAM(&mut cf as *mut _ as isize),
            );
        }
        n.font_key = (
            window.get_font_family().to_string(),
            (window.get_base_font_size() * window.get_zoom()).round() as i32,
        );
    }

    fn style_signature(state: &crate::AppState) -> String {
        let mut sig = String::new();
        for l in &state.doc().lines {
            sig.push_str(&format!("{:?}/{}/{}/|", l.colour, l.indent, l.checked));
            sig.push(match l.list_type {
                ListType::None => '.',
                ListType::Bullet => '*',
                ListType::Number => '#',
                ListType::Check => 'x',
            });
        }
        sig
    }

    fn apply_line_styles(edit: HWND, state: &crate::AppState) {
        let rich = rich_lines_from_state(state);
        let (bg, _) = theme_colours(&state.settings.theme);
        // Reset every line's band colour to the background first.
        unsafe {
            let len =
                SendMessageW(edit, WM_GETTEXTLENGTH, WPARAM(0), LPARAM(0)).0.max(0) as i32;
            select_range(edit, 0, len);
            let mut cf = std::mem::zeroed::<CharFormat2W>();
            cf.cb_size = std::mem::size_of::<CharFormat2W>() as u32;
            cf.dw_mask = CFM_BACKCOLOR;
            cf.cr_back_color = bg;
            let _ = SendMessageW(
                edit,
                EM_SETCHARFORMAT,
                WPARAM(SCF_SELECTION),
                LPARAM(&mut cf as *mut _ as isize),
            );
        }
        let mut off = 0i32;
        for (i, rl) in rich.iter().enumerate() {
            let end = off + rl.encode_utf16().count() as i32;
            let Some(line) = state.doc().lines.get(i) else { break };
            // Highlight band colour.
            if line.colour.is_highlighted() {
                let (_, rgba) = state.palette.resolve(line.colour);
                unsafe {
                    select_range(edit, off, end.max(off));
                    let mut cf = std::mem::zeroed::<CharFormat2W>();
                    cf.cb_size = std::mem::size_of::<CharFormat2W>() as u32;
                    cf.dw_mask = CFM_BACKCOLOR;
                    cf.cr_back_color = colorref(&format!("#{:06x}", (rgba >> 8) & 0xff_ffff));
                    let _ = SendMessageW(
                        edit,
                        EM_SETCHARFORMAT,
                        WPARAM(SCF_SELECTION),
                        LPARAM(&mut cf as *mut _ as isize),
                    );
                }
            }
            // Bullet / number paragraph formatting + indent.
            let numbering = match line.list_type {
                ListType::Bullet => PFN_BULLET,
                ListType::Number => PFN_ARABIC,
                _ => 0,
            };
            unsafe {
                select_range(edit, off, end.max(off));
                let mut pf = std::mem::zeroed::<ParaFormat2W>();
                pf.cb_size = std::mem::size_of::<ParaFormat2W>() as u32;
                pf.dw_mask = PFM_NUMBERING | PFM_STARTINDENT | PFM_OFFSET;
                pf.w_numbering = numbering;
                pf.dx_start_indent = (10 + line.indent as i32 * 22) * 15;
                pf.dx_offset = if numbering != 0 { 22 * 15 } else { 0 };
                let _ = SendMessageW(
                    edit,
                    EM_SETPARAFORMAT,
                    WPARAM(SCF_SELECTION),
                    LPARAM(&mut pf as *mut _ as isize),
                );
            }
            off = end + 2; // "\r\n"
        }
    }

    // ── poll / sync ───────────────────────────────────────────────────────

    fn sync_native(window: &AppWindow, state_lock: &SharedState) {
        let mut guard = slot().lock().unwrap();
        let Some(n) = guard.as_mut() else { return };
        let edit = HWND(n.edit as *mut _);
        let state = lock(state_lock);

        // Geometry: sit exactly over the (hidden) Slint editor.
        let rect = (
            window.get_editor_gx() as i32,
            window.get_editor_gy() as i32,
            window.get_editor_gw() as i32,
            window.get_editor_gh() as i32,
        );
        if rect != n.rect && rect.2 > 8 && rect.3 > 8 {
            n.rect = rect;
            unsafe {
                let _ = MoveWindow(edit, rect.0, rect.1, rect.2, rect.3, TRUE);
            }
        }

        // Theme + font.
        let theme = state.settings.theme.clone();
        let font_key = (
            window.get_font_family().to_string(),
            (window.get_base_font_size() * window.get_zoom()).round() as i32,
        );
        if theme != n.theme || font_key != n.font_key {
            n.theme = theme.clone();
            n.font_key = font_key;
            apply_theme_and_font(n, window, &theme);
        }

        // Wrap: EM_SETTARGETDEVICE(1) disables word wrap.
        let wrap = state.settings.word_wrap;
        if n.wrap != Some(wrap) {
            n.wrap = Some(wrap);
            unsafe {
                let _ = SendMessageW(
                    edit,
                    EM_SETTARGETDEVICE,
                    WPARAM(if wrap { 0 } else { 1 }),
                    LPARAM(0),
                );
            }
        }

        // Text: push only when the model differs from what Rich Edit shows.
        let want = rich_lines_from_state(&state).join("\r\n");
        let have = get_rich_text(edit);
        if have != want {
            let off = utf16_offset(
                &rich_lines_from_state(&state),
                state.cursor.line,
                state.cursor.col,
            );
            set_rich_text(edit, &want);
            unsafe {
                let mut cr = CharRange {
                    cp_min: off,
                    cp_max: off,
                };
                let _ = SendMessageW(
                    edit,
                    EM_EXSETSEL,
                    WPARAM(0),
                    LPARAM(&mut cr as *mut _ as isize),
                );
                let _ = SendMessageW(edit, EM_SCROLLCARET, WPARAM(0), LPARAM(0));
            }
        }

        // Highlights / list formatting: restyle when the signature changes.
        let sig = style_signature(&state);
        if sig != n.style_sig {
            n.style_sig = sig;
            apply_line_styles(edit, &state);
        }
    }

    fn on_pending_edit(window: &AppWindow, state: &SharedState) {
        let text = {
            let guard = slot().lock().unwrap();
            let Some(n) = guard.as_ref() else { return };
            get_rich_text(HWND(n.edit as *mut _))
        };
        let model = model_text_from_rich(&text);
        window_cb::apply_surface_text(window, state, &model);
    }

    fn on_pending_sel(window: &AppWindow, state: &SharedState) {
        let (line, col16) = {
            let guard = slot().lock().unwrap();
            let Some(n) = guard.as_ref() else { return };
            let edit = HWND(n.edit as *mut _);
            unsafe {
                let mut cr = CharRange {
                    cp_min: 0,
                    cp_max: 0,
                };
                let _ = SendMessageW(
                    edit,
                    EM_EXGETSEL,
                    WPARAM(0),
                    LPARAM(&mut cr as *mut _ as isize),
                );
                let cp = cr.cp_min.max(0) as usize;
                let line = SendMessageW(edit, EM_EXLINEFROMCHAR, WPARAM(0), LPARAM(cp as isize))
                    .0
                    .max(0) as usize;
                let idx =
                    SendMessageW(edit, EM_LINEINDEX, WPARAM(line), LPARAM(0)).0.max(0) as usize;
                (line, cp.saturating_sub(idx))
            }
        };
        {
            let mut st = lock(state);
            st.cursor.line = line.min(st.doc().line_count().saturating_sub(1));
            let len = st
                .doc()
                .lines
                .get(st.cursor.line)
                .map(|l| {
                    // Convert the UTF-16 column to a character column over the
                    // rich (possibly prefixed) line, sans prefix.
                    l.text.chars().count()
                })
                .unwrap_or(0);
            st.cursor.col = col16.min(len);
        }
        sync::sync_status(window, &lock(state));
    }

    pub fn poll(window: &AppWindow, state: &SharedState) {
        if PENDING_EDIT.swap(false, Ordering::SeqCst) {
            on_pending_edit(window, state);
        }
        if PENDING_SEL.swap(false, Ordering::SeqCst) {
            on_pending_sel(window, state);
        }
        if attached() {
            sync_native(window, state);
        }
    }

    pub fn start_attach(window: &AppWindow, state: &SharedState) {
        CONTEXT.with(|c| *c.borrow_mut() = Some((window.clone(), state.clone())));
        let w = window.clone();
        let s = state.clone();
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(100),
            move || {
                if !attached() {
                    if try_attach() {
                        w.set_native_editor(true);
                        sync::sync_all(&w, &lock(&s));
                        poll(&w, &s);
                    }
                } else {
                    poll(&w, &s);
                }
            },
        );
        std::mem::forget(timer);
    }
}

#[cfg(windows)]
pub use imp::{attached, poll, start_attach};
#[cfg(not(windows))]
pub use imp::{attached, start_attach};
