// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod cli;
#[allow(dead_code)]
const _ART: () = ();
mod core;
mod ui;

use gpui_kit::*;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|a| a == "--cli").unwrap_or(false) {
        if let Err(e) = cli::run(&args[1..]) {
            eprintln!("error: {e:#}");
            std::process::exit(1);
        }
        return;
    }
    // Start the I/O runtime before the UI.
    std::sync::LazyLock::force(&core::net::RT);

    gpui_kit::application().with_assets(gpui_kit::assets::AllAssets).run(|cx: &mut App| {
        gpui_kit::init(cx);
        ui::theme::apply(cx);
        let bounds = Bounds::centered(None, size(px(1220.), px(800.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(900.), px(600.))),
            titlebar: Some(TitlebarOptions { title: Some("Lodestone".into()), ..Default::default() }),
            app_id: Some("dev.p1e2r.lodestone".into()),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| ui::LodestoneApp::new(window, cx))).expect("open window");
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
