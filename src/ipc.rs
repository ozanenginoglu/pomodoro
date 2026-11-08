//! Tek örnek (single instance) ve komut satırı kontrolü.
//!
//! Çalışan uygulama yerel bir soket dinler (Linux: soyut ad alanında Unix
//! soketi, Windows: named pipe). `pomodoro toggle` gibi komutlar bu sokete bir
//! satır yazar ve tek satırlık yanıtı okur. Soyut ad alanı sayesinde uygulama
//! çökse bile geride "ölü" soket dosyası kalmaz.

use std::{
    io::{self, BufRead, BufReader, Write},
    sync::{Arc, Mutex},
    thread,
};

use interprocess::local_socket::{GenericNamespaced, ListenerOptions, prelude::*};

use crate::status::{self, Format, StatusInfo};

const SOCKET: &str = "pomodoro-widget.sock";

/// Soket adı. `POMODORO_SOCKET` ile değiştirilebilir (ör. testte ikinci bir örnek
/// çalıştırmak için).
fn socket_name() -> String {
    std::env::var("POMODORO_SOCKET").unwrap_or_else(|_| SOCKET.to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Pomodoro'yu başlat / duraklat.
    Toggle,
    /// Pomodoro'nun mevcut evresini baştan al.
    Reset,
    /// Pomodoro'da sonraki evreye geç.
    Skip,
    /// Pomodoro ↔ toplantı.
    Mode,
    /// Ayarlar panelini aç.
    Settings,
}

impl Command {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "toggle" => Command::Toggle,
            "reset" => Command::Reset,
            "skip" => Command::Skip,
            "mode" => Command::Mode,
            "settings" => Command::Settings,
            _ => return None,
        })
    }
}

/// Widget'ın paylaştığı, sunucu iş parçacığının okuduğu durum.
pub type Status = Arc<Mutex<StatusInfo>>;

/// Çalışan widget'a komut gönderir; yanıtı döndürür.
pub fn send(line: &str) -> io::Result<String> {
    let name = socket_name().to_ns_name::<GenericNamespaced>()?;
    let mut conn = BufReader::new(LocalSocketStream::connect(name)?);
    conn.get_mut().write_all(format!("{line}\n").as_bytes())?;
    let mut reply = String::new();
    conn.read_line(&mut reply)?;
    Ok(reply.trim_end_matches('\n').to_string())
}

pub enum Bind {
    /// Bu örnek sunucu oldu; komutlar bu kanaldan gelir.
    Listening(async_channel::Receiver<Command>, Status),
    /// Başka bir örnek zaten çalışıyor.
    AlreadyRunning,
}

/// Soketi dinlemeye başlar. Bağlantılar ayrı bir iş parçacığında işlenir.
pub fn listen() -> io::Result<Bind> {
    let name = socket_name().to_ns_name::<GenericNamespaced>()?;
    let listener = match ListenerOptions::new().name(name.clone()).create_sync() {
        Ok(listener) => listener,
        // Linux'ta `AddrInUse`, Windows'ta (ilk boru örneği bayrağı yüzünden)
        // `PermissionDenied` gelir. Platformdan bağımsız olmak için: soket
        // açılamıyor ama bağlanılabiliyorsa başka bir örnek çalışıyordur.
        Err(e) => {
            return match LocalSocketStream::connect(name) {
                Ok(_) => Ok(Bind::AlreadyRunning),
                Err(_) => Err(e),
            };
        }
    };
    let (tx, rx) = async_channel::unbounded();
    let status = Status::default();
    let shared = status.clone();

    thread::Builder::new().name("ipc".into()).spawn(move || {
        for conn in listener.incoming().filter_map(Result::ok) {
            let mut conn = BufReader::new(conn);
            let mut line = String::new();
            if conn.read_line(&mut line).is_err() {
                continue;
            }
            let line = line.trim();
            let reply = if let Some(rest) = line.strip_prefix("status") {
                match status_format(rest) {
                    Some(format) => {
                        let info = shared.lock().map(|s| s.clone()).unwrap_or_default();
                        status::format(&info, format)
                    }
                    None => crate::i18n::unknown_format(rest.trim()),
                }
            } else if let Some(command) = Command::parse(line) {
                match tx.send_blocking(command) {
                    Ok(()) => "ok".into(),
                    Err(_) => break, // uygulama kapanıyor
                }
            } else {
                crate::i18n::unknown_command(line)
            };
            let _ = conn.get_mut().write_all(format!("{reply}\n").as_bytes());
        }
    })?;

    Ok(Bind::Listening(rx, status))
}

/// `status` ya da `status waybar` gibi bir satırın biçim kısmı.
fn status_format(rest: &str) -> Option<Format> {
    match rest.trim() {
        "" => Some(Format::platform_default()),
        name => Format::parse(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_commands_only() {
        assert_eq!(Command::parse("toggle"), Some(Command::Toggle));
        assert_eq!(Command::parse("settings"), Some(Command::Settings));
        assert_eq!(Command::parse("status"), None, "status sunucu iş parçacığında yanıtlanır");
        assert_eq!(Command::parse("TOGGLE"), None);
    }

    #[test]
    fn status_format_defaults_per_platform() {
        assert_eq!(status_format(""), Some(Format::platform_default()));
        assert_eq!(status_format(" waybar"), Some(Format::Waybar));
        assert_eq!(status_format(" nope"), None);
    }
}
