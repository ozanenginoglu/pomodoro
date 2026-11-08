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

const SOCKET: &str = "pomodoro-widget.sock";

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

pub const USAGE: &str = "\
Kullanım: pomodoro [KOMUT]

Komut verilmezse widget'ı açar (zaten açıksa bir şey yapmaz).

Komutlar (çalışan widget'a gönderilir):
  toggle     Pomodoro'yu başlat / duraklat
  reset      Mevcut evreyi baştan al
  skip       Sonraki evreye geç
  mode       Pomodoro ↔ toplantı
  settings   Ayarlar panelini aç
  status     Durumu xmobar biçiminde yazdır (widget kapalıysa boş)
";

/// Widget'ın paylaştığı, sunucu iş parçacığının okuduğu durum satırı.
pub type Status = Arc<Mutex<String>>;

/// Çalışan widget'a komut gönderir; yanıtı döndürür.
pub fn send(line: &str) -> io::Result<String> {
    let name = SOCKET.to_ns_name::<GenericNamespaced>()?;
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
    let name = SOCKET.to_ns_name::<GenericNamespaced>()?;
    let listener = match ListenerOptions::new().name(name).create_sync() {
        Ok(listener) => listener,
        Err(e) if e.kind() == io::ErrorKind::AddrInUse => return Ok(Bind::AlreadyRunning),
        Err(e) => return Err(e),
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
            let reply = if line == "status" {
                shared.lock().map(|s| s.clone()).unwrap_or_default()
            } else if let Some(command) = Command::parse(line) {
                match tx.send_blocking(command) {
                    Ok(()) => "ok".into(),
                    Err(_) => break, // uygulama kapanıyor
                }
            } else {
                format!("bilinmeyen komut: {line}")
            };
            let _ = conn.get_mut().write_all(format!("{reply}\n").as_bytes());
        }
    })?;

    Ok(Bind::Listening(rx, status))
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
}
