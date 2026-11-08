//! Pomodoro — sade, daire biçimli zamanlayıcı widget'ı.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod alert;
mod assets;
mod config;
mod ipc;
mod meeting;
mod platform;
mod pomodoro;
mod timer;
mod ui;

use std::process::ExitCode;

use gpui::{App, AppContext, Bounds, WindowBounds, px, size};
use gpui_platform::application;

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    if let Some(arg) = std::env::args().nth(1) {
        return run_command(&arg);
    }

    let (commands, status) = match ipc::listen() {
        Ok(ipc::Bind::Listening(commands, status)) => (commands, status),
        Ok(ipc::Bind::AlreadyRunning) => {
            eprintln!("Pomodoro zaten çalışıyor.");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            // Soket açılamasa da widget çalışsın; yalnızca komutlar çalışmaz.
            log::warn!("komut soketi açılamadı: {e}");
            (async_channel::unbounded().1, ipc::Status::default())
        }
    };

    application().with_assets(assets::Assets).run(move |cx: &mut App| {
        ui::widget::bind_keys(cx);
        ui::settings::bind_keys(cx);

        let config = cx.new(|_| config::Config::load());
        let side = config.read(cx).appearance.size.pixels();
        let bounds = Bounds::centered(None, size(px(side), px(side)), cx);
        let options = platform::widget_window_options(WindowBounds::Windowed(bounds));

        cx.open_window(options, |window, cx| {
            cx.new(|cx| ui::widget::Widget::new(config, commands, status, window, cx))
        })
        .expect("widget penceresi açılamadı");
        cx.activate(true);
    });
    ExitCode::SUCCESS
}

/// `pomodoro <komut>`: çalışan widget'a gönderir.
fn run_command(arg: &str) -> ExitCode {
    match arg {
        "-h" | "--help" | "help" => {
            print!("{}", ipc::USAGE);
            return ExitCode::SUCCESS;
        }
        "-V" | "--version" => {
            println!("pomodoro {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        _ => {}
    }
    match ipc::send(arg) {
        Ok(reply) if arg == "status" => {
            println!("{reply}");
            ExitCode::SUCCESS
        }
        Ok(reply) if reply == "ok" => ExitCode::SUCCESS,
        Ok(reply) => {
            eprintln!("{reply}\n\n{}", ipc::USAGE);
            ExitCode::from(2)
        }
        // Widget kapalı: durum boş (xmobar'da hiçbir şey görünmez).
        Err(_) if arg == "status" => ExitCode::SUCCESS,
        Err(_) => {
            eprintln!("Pomodoro çalışmıyor.");
            ExitCode::from(1)
        }
    }
}
