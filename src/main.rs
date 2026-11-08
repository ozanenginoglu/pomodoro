//! Pomodoro — sade, daire biçimli zamanlayıcı widget'ı.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod alert;
mod assets;
mod config;
mod i18n;
mod ipc;
mod meeting;
mod platform;
mod pomodoro;
mod status;
mod timer;
mod ui;

use std::process::ExitCode;

use gpui::{App, AppContext};
use gpui_platform::application;

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    // Komut satırı mesajları da ayarlardaki dilde olsun.
    let config = config::Config::load();
    i18n::set(config.language);

    if let Some(arg) = std::env::args().nth(1) {
        return run_command(&arg);
    }

    let (commands, status) = match ipc::listen() {
        Ok(ipc::Bind::Listening(commands, status)) => (commands, status),
        Ok(ipc::Bind::AlreadyRunning) => {
            eprintln!("{}", i18n::t().already_running);
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

        let config = cx.new(|_| config);
        let appearance = config.read(cx).appearance.clone();
        // Layer-shell denemesi başarısız olursa kapanış ikinci kez çağrılır.
        platform::open_widget(cx, &appearance, move |window, cx| {
            let (config, commands, status) = (config.clone(), commands.clone(), status.clone());
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
            print!("{}", i18n::t().usage);
            return ExitCode::SUCCESS;
        }
        "-V" | "--version" => {
            println!("pomodoro {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        "status" => return print_status(),
        _ => {}
    }
    match ipc::send(arg) {
        Ok(reply) if reply == "ok" => ExitCode::SUCCESS,
        Ok(reply) => {
            eprintln!("{reply}\n\n{}", i18n::t().usage);
            ExitCode::from(2)
        }
        Err(_) => {
            eprintln!("{}", i18n::t().not_running);
            ExitCode::from(1)
        }
    }
}

/// `pomodoro status [--format xmobar|waybar|plain]`
fn print_status() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let format = match args.as_slice() {
        [] => status::Format::platform_default(),
        [flag, value] if flag == "--format" || flag == "-f" => match status::Format::parse(value) {
            Some(format) => format,
            None => {
                eprintln!("{}", i18n::unknown_format(value));
                return ExitCode::from(2);
            }
        },
        [arg] if arg.starts_with("--format=") => {
            match status::Format::parse(&arg["--format=".len()..]) {
                Some(format) => format,
                None => {
                    eprintln!("{}", i18n::unknown_format(arg));
                    return ExitCode::from(2);
                }
            }
        }
        _ => {
            eprint!("{}", i18n::t().usage);
            return ExitCode::from(2);
        }
    };
    match ipc::send(&format!("status {}", format.name())) {
        Ok(reply) => println!("{reply}"),
        // Widget kapalı: xmobar'da hiçbir şey görünmesin; waybar modülü gizlensin.
        Err(_) => println!("{}", status::closed(format)),
    }
    ExitCode::SUCCESS
}
