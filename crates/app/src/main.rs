#![cfg_attr(windows, windows_subsystem = "windows")]

use std::ffi::OsString;
use std::process::ExitCode;

use bobba::backend::{Backend, DirectTarget, UiSink};
use bobba::scenes::SCENES;
use bobba::state::App;
use bobba::view::chrome::TITLEBAR_HEIGHT;
use bobba_intercept::hosts;
use rune::{AppSpec, Titlebar, Waker};

const WINDOW: (f32, f32) = (760.0, 540.0);

fn setup(s: &mut App, wake: Waker) {
    if bobba::scenes::is_headless() {
        return;
    }
    s.feed.serve(wake.clone());
    s.store_feed.serve(wake.clone());
    let (sink, events) = UiSink::new(wake);
    s.attach(Backend::spawn(sink), events);
    if let Some(target) = DirectTarget::from_args(std::env::args()) {
        s.connect_to(target);
    }
}

fn main() -> ExitCode {
    let arguments: Vec<OsString> = std::env::args_os().collect();
    if let Some((command, rest)) = arguments.get(1..).and_then(<[OsString]>::split_first)
        && command == hosts::HELPER_COMMAND
    {
        return hosts::serve(rest);
    }
    rune::App::new(AppSpec {
        min_size: Some((680.0, 480.0)),
        scenes: SCENES,
        setup: Some(setup),
        poll: Some(bobba::poll),
        on_key: Some(bobba::on_key),
        ..AppSpec::new("Bobba", WINDOW, App::new, bobba::view::root)
    })
    .titlebar(Titlebar::new(TITLEBAR_HEIGHT))
    .run();
    ExitCode::SUCCESS
}
