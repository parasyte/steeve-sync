#![cfg_attr(not(any(test, debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::all)]

use image::error::ImageError;
use log::{error, info, SetLoggerError};
use steeve_sync::{
    logger::{Logger, MemLogger},
    Error as SteeveError, Steeve,
};
use tao::{
    error::OsError,
    event_loop::EventLoop,
    menu::{ContextMenu, MenuId, MenuItemAttributes},
    system_tray::{BadIcon, Icon as TrayIcon, SystemTray, SystemTrayBuilder},
    window::{Icon, Theme, Window, WindowBuilder},
};
use thiserror::Error;
use time::error::IndeterminateOffset;

#[cfg(target_os = "windows")]
use tao::platform::windows::IconExtWindows;

/// All the ways in which Steeve-Sync can fail.
#[derive(Debug, Error)]
enum AppError {
    #[error("Logger error")]
    Logger(#[from] SetLoggerError),

    #[error("Steeve error: {0}")]
    Steeve(#[from] SteeveError),

    #[error("OS error: {0}")]
    OsError(#[from] OsError),

    #[error("OS time error")]
    Time(#[from] IndeterminateOffset),

    #[error("Image decoder: {0}")]
    Image(#[from] ImageError),

    #[error("Bad Icon: {0}")]
    Icon(#[from] BadIcon),
}

/// The primary application
struct App {
    options: MenuId,
    quit: MenuId,
    black_icon: Vec<u8>,
    white_icon: Vec<u8>,
    window: Window,
    menu: Option<SystemTray>,
}

fn init_logger() -> Result<(Logger, Logger), AppError> {
    use simplelog::*;
    use time::UtcOffset;

    let info_logger = Logger::default();
    let info_memlogger = MemLogger::new(100, info_logger.clone());

    let debug_logger = Logger::default();
    let debug_memlogger = MemLogger::new(1000, debug_logger.clone());

    let config = ConfigBuilder::new()
        .add_filter_allow_str("steeve_sync")
        .set_time_offset(UtcOffset::current_local_offset()?)
        .set_time_format_custom(format_description!(
            "[year]-[month]-[day] [hour repr:24]:[minute]:[second].[subsecond digits:3]"
        ))
        .build();

    CombinedLogger::init(vec![
        TermLogger::new(
            LevelFilter::Info,
            config.clone(),
            TerminalMode::Mixed,
            ColorChoice::Auto,
        ),
        WriteLogger::new(LevelFilter::Debug, config.clone(), debug_memlogger),
        WriteLogger::new(LevelFilter::Info, config, info_memlogger),
    ])?;

    Ok((debug_logger, info_logger))
}

fn create_app(event_loop: &EventLoop<()>) -> Result<App, AppError> {
    let mut menu = ContextMenu::new();

    let options = menu.add_item(MenuItemAttributes::new("Options...")).id();
    let quit = menu.add_item(MenuItemAttributes::new("Quit")).id();

    let black_icon = read_icon(include_bytes!("../assets/steeve-sync-black.ico"))?;
    let white_icon = read_icon(include_bytes!("../assets/steeve-sync-white.ico"))?;

    let window = WindowBuilder::new()
        .with_title("Steeve-Sync")
        .with_visible(false)
        .build(event_loop)?;

    let icon = if window.theme() == Theme::Dark {
        white_icon.clone()
    } else {
        black_icon.clone()
    };
    let icon = TrayIcon::from_rgba(icon, 256, 256)?;
    let menu = Some(SystemTrayBuilder::new(icon, Some(menu)).build(event_loop)?);

    #[cfg(target_os = "windows")]
    window.set_window_icon(Icon::from_resource(icon_res, None).ok());

    Ok(App {
        options,
        quit,
        black_icon,
        white_icon,
        window,
        menu,
    })
}

fn run() -> Result<(), AppError> {
    use tao::{
        event::{Event, TrayEvent, WindowEvent},
        event_loop::ControlFlow,
    };

    // TODO: Use the loggers to show logs in the GUI
    let (_debug_logger, _info_logger) = init_logger()?;

    info!("Welcome, miners!");

    // TODO: Make this configurable
    let max_backups = 10;
    let mut steeve = Steeve::new(max_backups)?;

    info!("Steeve is waiting for bugs to kill...");

    // XXX: This must be the last use of the question-mark operator in the function.
    // Otherwise Obj-C panics on macOS from `msgbox` and then `tao` catches the panic and hides the
    // reason for the failure.
    let event_loop = EventLoop::new();
    let mut app = create_app(&event_loop)?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            // Quit events
            Event::MenuEvent { menu_id, .. } if menu_id == app.quit => {
                // Remove tray icon from system
                app.menu.take();

                // Stop watching the FS
                let _ = steeve.stop();

                info!("See you next mission!");
                *control_flow = ControlFlow::Exit;
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                app.window.set_visible(false);
            }

            // Theme events
            Event::WindowEvent {
                event: WindowEvent::ThemeChanged(theme),
                ..
            } => {
                if let Some(menu) = app.menu.as_mut() {
                    let icon = if theme == Theme::Dark {
                        app.white_icon.clone()
                    } else {
                        app.black_icon.clone()
                    };
                    let window_icon = Icon::from_rgba(icon.clone(), 256, 256).ok();
                    app.window.set_window_icon(window_icon);
                    let tray_icon = TrayIcon::from_rgba(icon, 256, 256).expect("TrayIcon");
                    menu.set_icon(tray_icon);
                }
            }

            // Menu events
            Event::TrayEvent {
                event: TrayEvent::LeftClick,
                ..
            } => {
                app.window.set_visible(true);
                app.window.set_focus();
            }
            Event::MenuEvent { menu_id, .. } if menu_id == app.options => {
                app.window.set_visible(true);
                app.window.set_focus();
            }

            _ => (),
        }
    });
}

fn read_icon(bytes: &[u8]) -> Result<Vec<u8>, AppError> {
    use image::{codecs::ico::IcoDecoder, ImageDecoder};
    use std::io::Cursor;

    let decoder = IcoDecoder::new(Cursor::new(bytes))?;
    let mut buf = vec![0; decoder.total_bytes().try_into().unwrap()];
    decoder.read_image(&mut buf)?;

    Ok(buf)
}

fn main() {
    use msgbox::IconType;

    if let Err(err) = run() {
        error!("Error: {err}");

        // Show error in message box.
        if let Err(err) = msgbox::create("Error", &err.to_string(), IconType::Error) {
            error!("Could not create message box: `{err}`")
        }
    }
}
