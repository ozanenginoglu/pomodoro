//! Linux (X11 + xmonad) özel kodu. Konumlandırma ve sürükleme xmonad
//! tarafından yapılır; durum satırı xmobar biçimindedir.

use std::cell::RefCell;

use gpui::{Point, Window, point, px};

use super::StatusInfo;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use x11rb::{protocol::xproto::ConnectionExt, rust_connection::RustConnection};

thread_local! {
    /// İmleç sorguları için ayrı, tembel açılan bir X bağlantısı.
    static X11: RefCell<Option<RustConnection>> = const { RefCell::new(None) };
}

/// İmlecin pencereye göre konumu (mantıksal piksel). İmleç başka bir
/// ekrandaysa veya sorgu başarısız olursa `None`.
pub fn pointer_position(window: &Window) -> Option<Point<gpui::Pixels>> {
    let RawWindowHandle::Xcb(handle) = HasWindowHandle::window_handle(window).ok()?.as_raw() else {
        return None;
    };
    let xid = handle.window.get();
    let scale = window.scale_factor();

    X11.with_borrow_mut(|conn| {
        if conn.is_none() {
            *conn = RustConnection::connect(None).ok().map(|(c, _)| c);
        }
        let reply = conn.as_ref()?.query_pointer(xid).ok().and_then(|c| c.reply().ok());
        let Some(reply) = reply else {
            // Bağlantı koptuysa bir sonraki çağrıda yeniden açılsın.
            *conn = None;
            return None;
        };
        reply
            .same_screen
            .then(|| point(px(reply.win_x as f32 / scale), px(reply.win_y as f32 / scale)))
    })
}

/// xmobar biçimli durum satırı. İkonlar xmobar'daki 4. ek yazı tipinden
/// (Font Awesome Solid) gelir: kronometre, kahve, kişiler.
pub fn format_status(info: &StatusInfo) -> String {
    const MUTED: &str = "#6272a4";
    const STOPWATCH: char = '\u{f2f2}';
    const MUG: char = '\u{f7b6}';
    const USERS: char = '\u{f0c0}';
    let icon = |c: char| format!("<fn=4>{c}</fn>");
    match info {
        StatusInfo::Idle => format!("<fc={MUTED}>{}</fc>", icon(STOPWATCH)),
        StatusInfo::Pomodoro { remaining, on_break, paused, color } => {
            let color = if *paused { MUTED } else { color };
            let glyph = if *on_break { MUG } else { STOPWATCH };
            format!("<fc={color}>{}  {remaining}</fc>", icon(glyph))
        }
        StatusInfo::Meeting { remaining, ends_at, color } => {
            format!("<fc={color}>{}  {remaining}</fc> <fc={MUTED}>→ {ends_at}</fc>", icon(USERS))
        }
    }
}
