use bobba::backend::{Backend, DirectTarget, UiSink};
use bobba::scenes::SCENES;
use bobba::state::App;
use bobba::view::chrome::TITLEBAR_HEIGHT;
use rune::{AppSpec, Titlebar, Waker};

const WINDOW: (f32, f32) = (1180.0, 760.0);

fn setup(s: &mut App, wake: Waker) {
    if bobba::scenes::is_headless() {
        return;
    }
    let (sink, events) = UiSink::new(wake);
    s.attach(Backend::spawn(sink), events);
    if let Some(target) = DirectTarget::from_args(std::env::args()) {
        s.connect_to(target);
    }
}

fn main() {
    rune::App::new(AppSpec {
        min_size: Some((900.0, 560.0)),
        scenes: SCENES,
        setup: Some(setup),
        poll: Some(bobba::poll),
        ..AppSpec::new("Bobba", WINDOW, App::new, bobba::view::root)
    })
    .titlebar(Titlebar::new(TITLEBAR_HEIGHT))
    .run();
}
