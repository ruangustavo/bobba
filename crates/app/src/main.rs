use bobba::app::BobbaApp;
use bobba::backend::{Backend, DirectTarget};
use gpui_kit::component::{Theme, ThemeMode, TitleBar};
use gpui_kit::*;

actions!(bobba, [Quit]);

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
            cx.on_action(|_: &Quit, cx: &mut App| cx.quit());
            let (ui, events) = smol::channel::unbounded();
            let backend = Backend::spawn(ui);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(None, size(px(1180.), px(760.)), cx)),
                ..TitleBar::window_options()
            };
            let opened = gpui_kit::open_window(options, cx, |window, cx| {
                window.set_window_title("Bobba");
                Theme::change(ThemeMode::Dark, Some(window), cx);
                let app = cx.new(|cx| BobbaApp::new(backend, events, window, cx));
                if let Some(target) = DirectTarget::from_args(std::env::args()) {
                    app.update(cx, |app, cx| app.connect_to(target, window, cx));
                }
                app
            });
            if opened.is_err() {
                cx.quit();
            }
        });
}
