use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::{AnyWindowHandle, AppContext, Entity, HeadlessAppContext, Size, px};
use image::{Rgba, RgbaImage};
use time::macros::datetime;
use tuclaw_core::model::{AgentId, ChannelId};
use tuclaw_core::v3::{Client, MockTransport, Pace, Scenario};
use tuclaw_desktop::audio::RodioSpeaker;
use tuclaw_desktop::chrome::Hold;
use tuclaw_desktop::icon::Icons;
use tuclaw_desktop::link::Source;
use tuclaw_desktop::local;
use tuclaw_desktop::state::AppState;
use tuclaw_desktop::testing::FakeRecorder;
use tuclaw_ios::navigator::Tab;
use tuclaw_ios::phone::Phone;

const WIDTH: f32 = 402.;
const HEIGHT: f32 = 874.;
const CHANNEL_TOLERANCE: u8 = 24;
const PIXEL_BUDGET: f64 = 0.002;

struct Stage {
    state: Entity<AppState>,
    phone: Option<Entity<Phone>>,
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
        Stage {
            state,
            phone: None,
            cx,
        }
    }

    fn open(&mut self) -> AnyWindowHandle {
        let state = self.state.clone();
        let mut phone = None;
        let window = self
            .cx
            .open_window(
                Size {
                    width: px(WIDTH),
                    height: px(HEIGHT),
                },
                |window, cx| {
                    let built = cx.new(|cx| Phone::new(state, window, cx));
                    phone = Some(built.clone());
                    built
                },
            )
            .expect("the window opens");
        self.phone = phone;
        self.settle();
        window.into()
    }

    fn phone(&self) -> Entity<Phone> {
        self.phone.clone().expect("the phone is open")
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

    fn open_channel(&mut self, name: &str) {
        let channel = self.channel(name);
        let phone = self.phone();
        self.cx.update(|cx| {
            let navigator = phone.read(cx).navigator().clone();
            navigator.update(cx, |navigator, cx| navigator.open_channel(channel, cx));
        });
        self.settle();
    }

    fn switch(&mut self, tab: Tab) {
        let phone = self.phone();
        self.cx.update(|cx| {
            let navigator = phone.read(cx).navigator().clone();
            navigator.update(cx, |navigator, cx| navigator.switch(tab, cx));
        });
        self.settle();
    }

    fn update(&mut self, f: impl FnOnce(&mut AppState, &mut gpui::Context<AppState>)) {
        let state = self.state.clone();
        self.cx.update(|cx| state.update(cx, f));
        self.settle();
    }

    fn settle(&mut self) {
        self.cx.run_until_parked();
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
}

type Scene = fn() -> Result<Vec<(&'static str, RgbaImage)>, String>;

fn home() -> Result<Vec<(&'static str, RgbaImage)>, String> {
    let mut stage = Stage::new();
    let window = stage.open();
    let general = stage.channel("General");
    let mut previewed = false;
    stage.cx.update(|cx| {
        previewed = stage.state.read(cx).preview(general).is_some();
    });
    if !previewed {
        return Err("General has no preview on Home".to_string());
    }
    Ok(vec![("phone-home", stage.shot(window))])
}

fn conversation() -> Result<Vec<(&'static str, RgbaImage)>, String> {
    let mut stage = Stage::new();
    let window = stage.open();
    stage.open_channel("Magnet Feed");
    let mut shown = None;
    stage
        .cx
        .update(|cx| shown = stage.state.read(cx).selected());
    if shown != Some(stage.channel("Magnet Feed")) {
        return Err("Magnet Feed is not the open conversation".to_string());
    }
    Ok(vec![("phone-conversation", stage.shot(window))])
}

fn held_voice() -> Result<Vec<(&'static str, RgbaImage)>, String> {
    let mut stage = Stage::new();
    let window = stage.open();
    stage.update(|state, _cx| state.set_recorder(Box::new(FakeRecorder::default())));
    stage.open_channel("General");
    let phone = stage.phone();
    stage.cx.update(|cx| {
        let talk = phone.read(cx).talk().clone();
        talk.update(cx, |talk, cx| talk.hold(Hold::Pressed, cx));
    });
    stage.wait(Duration::from_secs(2));
    Ok(vec![("phone-held-voice", stage.shot(window))])
}

fn agent_settings() -> Result<Vec<(&'static str, RgbaImage)>, String> {
    let mut stage = Stage::new();
    let window = stage.open();
    stage.switch(Tab::Agents);
    stage.update(|state, cx| state.open_settings(AgentId(1), cx));
    Ok(vec![("phone-agent-settings", stage.shot(window))])
}

fn automations() -> Result<Vec<(&'static str, RgbaImage)>, String> {
    let mut stage = Stage::new();
    let window = stage.open();
    stage.switch(Tab::Automations);
    let mut tasks = 0;
    stage
        .cx
        .update(|cx| tasks = stage.state.read(cx).tasks().len());
    if tasks == 0 {
        return Err("the Automations tab loaded no tasks".to_string());
    }
    Ok(vec![("phone-automations", stage.shot(window))])
}

const SCENES: [(&str, Scene); 5] = [
    ("home", home),
    ("conversation", conversation),
    ("held voice", held_voice),
    ("agent settings", agent_settings),
    ("automations", automations),
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
        println!("phone visual: {ran} scenes passed");
        return;
    }
    for failure in &failures {
        eprintln!("phone visual: {failure}");
    }
    std::process::exit(1);
}
