use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::{AnyWindowHandle, AppContext, Entity, HeadlessAppContext, Size, px};
use image::{Rgba, RgbaImage};
use time::macros::datetime;
use tuclaw_core::model::ChannelId;
use tuclaw_core::v3::{AgentId, Client, MockTransport, Pace, Scenario, SurfaceId};
use tuclaw_desktop::audio::RodioSpeaker;
use tuclaw_desktop::icon::Icons;
use tuclaw_desktop::link::Source;
use tuclaw_desktop::local;
use tuclaw_desktop::shell::Shell;
use tuclaw_desktop::state::AppState;

const WIDTH: f32 = 1280.;
const HEIGHT: f32 = 820.;
const CHANNEL_TOLERANCE: u8 = 24;
const PIXEL_BUDGET: f64 = 0.002;

struct Stage {
    state: Entity<AppState>,
    mock: MockTransport,
    cx: HeadlessAppContext,
}

impl Stage {
    fn new() -> Stage {
        let platform = gpui_platform::current_platform(true);
        let mut cx = HeadlessAppContext::with_platform(
            platform.text_system(),
            Arc::new(Icons),
            gpui_platform::current_headless_renderer,
        );
        cx.update(gpui_kit::init);
        let mock = MockTransport::new(Scenario::default(), Pace::Stepped);
        let client = Client::mock(&mock);
        let state = cx.update(|cx| {
            cx.new(|_| AppState::new(client, Source::Mock, Box::new(RodioSpeaker::default())))
        });
        cx.update(|cx| state.update(cx, |state, cx| state.start(cx)));
        cx.run_until_parked();
        mock.pump_control();
        cx.run_until_parked();
        Stage { state, mock, cx }
    }

    fn channel(&mut self, name: &str) -> ChannelId {
        let mut found = None;
        self.cx.update(|cx| {
            for channel in self.state.read(cx).channels() {
                if channel.name == name {
                    found = Some(channel.id);
                }
            }
        });
        found.unwrap_or_else(|| panic!("no channel named {name}"))
    }

    fn select(&mut self, name: &str) {
        let channel = self.channel(name);
        let state = self.state.clone();
        self.cx
            .update(|cx| state.update(cx, |state, cx| state.select(channel, cx)));
        self.settle();
    }

    fn update(&mut self, f: impl FnOnce(&mut AppState, &mut gpui::Context<AppState>)) {
        let state = self.state.clone();
        self.cx.update(|cx| state.update(cx, f));
        self.settle();
    }

    fn deliver(&mut self) {
        while self.mock.step() {}
        self.settle();
    }

    fn settle(&mut self) {
        self.cx.run_until_parked();
    }

    fn open(&mut self) -> AnyWindowHandle {
        let state = self.state.clone();
        let window = self
            .cx
            .open_window(
                Size {
                    width: px(WIDTH),
                    height: px(HEIGHT),
                },
                move |window, cx| cx.new(|cx| Shell::new(state, window, cx)),
            )
            .expect("the window opens");
        self.settle();
        window.into()
    }

    fn wait(&mut self, duration: Duration) {
        self.cx.advance_clock(duration);
        self.settle();
    }

    fn shot(&mut self, window: AnyWindowHandle) -> RgbaImage {
        self.cx
            .update_window(window, |_, window, _cx| window.refresh())
            .expect("the window is open");
        self.settle();
        self.cx
            .capture_screenshot(window)
            .expect("the frame renders")
    }

    fn counts(&mut self, name: &str) -> (usize, usize, bool) {
        let channel = self.channel(name);
        let mut counts = None;
        self.cx.update(|cx| {
            for candidate in self.state.read(cx).channels() {
                if candidate.id == channel {
                    counts = Some((candidate.unread, candidate.replies, candidate.marked));
                }
            }
        });
        counts.expect("the channel exists")
    }
}

type Scene = fn() -> Result<Vec<(&'static str, RgbaImage)>, String>;

fn away_from_general() -> Result<Vec<(&'static str, RgbaImage)>, String> {
    let mut stage = Stage::new();
    stage.select("General");
    let window = stage.open();
    stage.update(|state, cx| state.set_window_active(false, cx));
    stage.mock.agent_posts(
        SurfaceId(1),
        AgentId(1),
        "The air will be clean tonight, PM2.5 stays under 15.",
    );
    stage.mock.automation_posts(
        SurfaceId(1),
        AgentId(2),
        "Weekly digest: two new releases are out.",
    );
    stage
        .mock
        .agent_posts(SurfaceId(3), AgentId(2), "The living room lights are off.");
    stage.deliver();
    stage.update(|state, cx| state.set_window_active(true, cx));
    let unread = stage.shot(window);
    let (general, replies, _marked) = stage.counts("General");
    if (general, replies) != (2, 1) {
        return Err(format!(
            "General before reading: {general} unread, {replies} replies, expected 2 and 1"
        ));
    }
    stage.wait(Duration::from_secs(3));
    let read = stage.shot(window);
    let (general, replies, _marked) = stage.counts("General");
    if (general, replies) != (0, 0) {
        return Err(format!(
            "General after three seconds on screen: {general} unread, {replies} replies, expected none"
        ));
    }
    Ok(vec![("general-unread", unread), ("general-read", read)])
}

fn scrolled_up() -> Result<Vec<(&'static str, RgbaImage)>, String> {
    let mut stage = Stage::new();
    stage.select("General");
    let window = stage.open();
    stage.update(|state, _cx| state.set_following(false));
    stage
        .mock
        .agent_posts(SurfaceId(1), AgentId(1), "Done, the file is on the NAS.");
    stage.deliver();
    let shot = stage.shot(window);
    let (general, replies, _marked) = stage.counts("General");
    if (general, replies) != (1, 1) {
        return Err(format!(
            "General scrolled up: {general} unread, {replies} replies, expected 1 and 1"
        ));
    }
    Ok(vec![("general-pill", shot)])
}

fn automations() -> Result<Vec<(&'static str, RgbaImage)>, String> {
    let mut stage = Stage::new();
    stage.select("Magnet Feed");
    let window = stage.open();
    stage.update(|state, cx| state.open_automations(cx));
    Ok(vec![("magnet-automations", stage.shot(window))])
}

fn quoted_reply() -> Result<Vec<(&'static str, RgbaImage)>, String> {
    let mut stage = Stage::new();
    stage.select("General");
    let window = stage.open();
    stage.mock.agent_posts(
        SurfaceId(1),
        AgentId(1),
        "Steven answered on Friday:\n\n> Also, everything urgent for this week is done. If you want something else in work, mark those tickets with the urgent flag.\n\nNothing to repeat; one note about the flags is enough.",
    );
    stage.deliver();
    Ok(vec![("general-quote", stage.shot(window))])
}

fn marked_unread() -> Result<Vec<(&'static str, RgbaImage)>, String> {
    let mut stage = Stage::new();
    stage.select("General");
    let window = stage.open();
    let home = stage.channel("Smart Home");
    stage.update(|state, cx| state.mark_unread(home, cx));
    let shot = stage.shot(window);
    let (_unread, _replies, marked) = stage.counts("Smart Home");
    if !marked {
        return Err("Smart Home is not marked unread".to_string());
    }
    Ok(vec![("sidebar-marked", shot)])
}

const SCENES: [(&str, Scene); 5] = [
    ("away from General", away_from_general),
    ("scrolled up", scrolled_up),
    ("automations", automations),
    ("marked unread", marked_unread),
    ("quoted reply", quoted_reply),
];

fn baselines() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("visual")
}

fn output() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("target")
        .join("visual")
}

fn compare(name: &str, actual: &RgbaImage, update: bool) -> Result<(), String> {
    let baseline = baselines().join(format!("{name}.png"));
    if update {
        actual
            .save(&baseline)
            .map_err(|error| format!("{name}: cannot write the baseline: {error}"))?;
        return Ok(());
    }
    let expected = image::open(&baseline)
        .map_err(|error| {
            format!(
                "{name}: no baseline at {} ({error}); run with UPDATE_BASELINE=1",
                baseline.display()
            )
        })?
        .to_rgba8();
    if expected.dimensions() != actual.dimensions() {
        save_failure(name, actual, None);
        return Err(format!(
            "{name}: {:?} instead of {:?}",
            actual.dimensions(),
            expected.dimensions()
        ));
    }
    let mut diff = RgbaImage::new(actual.width(), actual.height());
    let mut differing = 0usize;
    for (x, y, pixel) in actual.enumerate_pixels() {
        let other = expected.get_pixel(x, y);
        let mut far = false;
        for channel in 0..4 {
            if pixel[channel].abs_diff(other[channel]) > CHANNEL_TOLERANCE {
                far = true;
            }
        }
        if far {
            differing += 1;
            diff.put_pixel(x, y, Rgba([220, 40, 40, 255]));
        } else {
            let Rgba([r, g, b, _]) = *pixel;
            let grey = ((u16::from(r) + u16::from(g) + u16::from(b)) / 3) as u8;
            diff.put_pixel(x, y, Rgba([grey, grey, grey, 60]));
        }
    }
    let total = f64::from(actual.width()) * f64::from(actual.height());
    let share = differing as f64 / total;
    if share > PIXEL_BUDGET {
        save_failure(name, actual, Some(&diff));
        return Err(format!(
            "{name}: {differing} pixels ({:.2}%) differ from the baseline; see {}",
            share * 100.,
            output().display()
        ));
    }
    Ok(())
}

fn save_failure(name: &str, actual: &RgbaImage, diff: Option<&RgbaImage>) {
    let directory = output();
    std::fs::create_dir_all(&directory).ok();
    actual.save(directory.join(format!("{name}.png"))).ok();
    if let Some(diff) = diff {
        diff.save(directory.join(format!("{name}.diff.png"))).ok();
    }
}

fn main() {
    unsafe { std::env::set_var("TZ", "UTC") };
    local::fix_now(datetime!(2026-10-03 16:00 UTC));
    let update = std::env::var("UPDATE_BASELINE").is_ok_and(|value| value == "1");
    let filter = std::env::args().skip(1).find(|arg| !arg.starts_with('-'));
    std::fs::create_dir_all(baselines()).expect("the baseline directory exists");
    let mut failures = Vec::new();
    let mut ran = 0;
    for (title, scene) in SCENES {
        if filter
            .as_deref()
            .is_some_and(|filter| !title.contains(filter))
        {
            continue;
        }
        ran += 1;
        match scene() {
            Ok(shots) => {
                for (name, shot) in shots {
                    if let Err(failure) = compare(name, &shot, update) {
                        failures.push(failure);
                    }
                }
            }
            Err(failure) => failures.push(format!("{title}: {failure}")),
        }
    }
    if failures.is_empty() {
        println!("visual: {ran} scenes passed");
        return;
    }
    for failure in &failures {
        eprintln!("visual: {failure}");
    }
    std::process::exit(1);
}
