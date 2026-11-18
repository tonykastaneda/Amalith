//! Amalith's docking/undocking, prototyped on GPUI. No tools or real
//! panel bodies: just masters, groups and tabs moving between the two
//! rails and their own floating windows.

mod dock;
mod native;
mod shell;
mod ui;

use gpui::{App, AppContext, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};

fn main() {
    Application::new().run(|cx: &mut App| {
        let shell = cx.new(|_| shell::Shell::new());
        let bounds = Bounds::centered(None, size(px(1280.0), px(800.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Amalith — GPUI dock test".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| ui::MainView::new(shell.clone(), window, cx)),
        )
        .expect("open main window");
        cx.activate(true);
    });
}
