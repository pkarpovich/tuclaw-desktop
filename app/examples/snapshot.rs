use std::path::PathBuf;
use std::sync::Arc;

use gpui::{AppContext, HeadlessAppContext, Size, px};
use tuclaw_core::v3::{Client, MockTransport, Pace, Scenario};
use tuclaw_desktop::audio::{PeakCache, RodioSpeaker};
use tuclaw_desktop::link::Source;
use tuclaw_desktop::runlog::Disclosure;
use tuclaw_desktop::shell::Shell;
use tuclaw_desktop::state::AppState;

fn main() {
    let channel = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Magnet Feed".to_string());
    let open_logs = std::env::args().any(|arg| arg == "--open-logs");
    let inspect = std::env::args().any(|arg| arg == "--inspect");
    let height = match std::env::var("SNAPSHOT_HEIGHT") {
        Ok(height) => height.parse::<f32>().expect("a height in points"),
        Err(_) => 820.,
    };
    let platform = gpui_platform::current_platform(true);
    let mut cx = HeadlessAppContext::with_platform(
        platform.text_system(),
        Arc::new(()),
        gpui_platform::current_headless_renderer,
    );
    cx.update(gpui_kit::init);
    let mock = match std::env::var("TUCLAW_MOCK_WORLD") {
        Ok(path) => {
            let seed = tuclaw_desktop::link::load_seed(std::path::Path::new(&path))
                .expect("the world loads");
            MockTransport::seeded(seed, Scenario::default(), Pace::Stepped)
        }
        Err(_) => MockTransport::new(Scenario::default(), Pace::Stepped),
    };
    let client = Client::mock(&mock);
    let cache = PeakCache::new(std::env::temp_dir().join("tuclaw-snapshot-peaks"));
    let state = cx.update(|cx| {
        cx.new(|_| {
            AppState::new(client, Source::Mock, Box::new(RodioSpeaker::default()))
                .with_peak_cache(cache)
        })
    });
    cx.update(|cx| state.update(cx, |state, cx| state.start(cx)));
    cx.run_until_parked();
    mock.pump_control();
    cx.run_until_parked();
    let mut found = None;
    cx.update(|cx| {
        for candidate in state.read(cx).channels() {
            if candidate.name == channel {
                found = Some(candidate.id);
            }
        }
    });
    let Some(selected) = found else {
        eprintln!("no channel named {channel}");
        std::process::exit(1);
    };
    cx.update(|cx| state.update(cx, |state, cx| state.select(selected, cx)));
    cx.run_until_parked();
    if open_logs {
        cx.update(|cx| {
            state.update(cx, |state, cx| {
                let mut ids = Vec::new();
                for message in state.messages() {
                    if message.run.is_some() {
                        ids.push(message.id);
                    }
                }
                for id in ids {
                    state.toggle(Disclosure::Log(id), cx);
                }
            })
        });
        cx.run_until_parked();
    }
    if inspect {
        cx.update(|cx| {
            state.update(cx, |state, cx| {
                let mut last = None;
                for message in state.messages() {
                    if message.run.is_some() {
                        last = Some(message.id);
                    }
                }
                if let Some(id) = last {
                    state.toggle(Disclosure::Inspect(id), cx);
                }
            })
        });
        cx.run_until_parked();
    }
    let built = state.clone();
    let window = cx
        .open_window(
            Size {
                width: px(1280.),
                height: px(height),
            },
            move |window, cx| cx.new(|cx| Shell::new(built, window, cx)),
        )
        .expect("the window opens");
    cx.run_until_parked();
    let image = cx
        .capture_screenshot(window.into())
        .expect("the frame renders");
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("target")
        .join("snapshots");
    std::fs::create_dir_all(&directory).expect("the directory exists");
    let path = directory.join(format!("{}.png", channel.replace(' ', "-")));
    image.save(&path).expect("the png is written");
    println!("{}", path.display());
}
