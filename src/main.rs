#![forbid(unsafe_code)]

//! OpeNitro — Single-binary GPU-accelerated Acer Nitro & Predator controller for Linux.

use gpui_kit::{
    AppContext as _, Bounds, QuitMode, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
use openitro::backend::cli::maybe_run_cli;
use openitro::backend::privilege::run_ec_helper_loop;
use openitro::ui::app::OpeNitroApp;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--ec-helper") {
        let code = run_ec_helper_loop();
        std::process::exit(code);
    }

    if let Some(code) = maybe_run_cli(&args) {
        std::process::exit(code);
    }

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(|cx| {
            gpui_kit::init(cx);

            cx.on_window_closed(|cx, _window_id| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let options = WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("OpeNitro — Linux System Controller".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Default::default(),
                    size: size(px(1140.0), px(740.0)),
                })),
                window_min_size: Some(size(px(360.0), px(420.0))),
                ..Default::default()
            };

            let _ = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| OpeNitroApp::new(window, cx))
            })
            .expect("failed to open OpeNitro window");

            cx.activate(true);
        });
}
