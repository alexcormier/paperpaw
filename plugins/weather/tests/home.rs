//! The weather widgets on the home screen of the standard app on an X4,
//! with the network answered here (no real requests): what they show,
//! that they draw, and when the weather is asked for.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{HttpResponse, Io, Kernel};
use paperpaw_platform::{
    Color, DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
    SystemStatus, WallClock,
};
use paperpaw_sdk::MemoryStorage;

/// An X4 as the emulator's profile has it: buttons, no touch.
const DEVICE: DeviceInfo = DeviceInfo {
    id: "test-x4",
    name: "Test X4",
    display: DisplayInfo {
        width: 480,
        height: 800,
        dpi: 220,
        format: PixelFormat::Mono1,
        rotation: Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: false,
        pen: false,
        keys: &["back", "confirm", "left", "right", "up", "down", "power"],
    },
    memory: MemoryClass::Small,
};

const KEPT: &str = ".paperpaw/plugins/weather/weather.txt";
/// When the kept weather was fetched (2026-10-14, 10:30 UTC).
const FETCHED: i64 = 1_791_973_800;
const PLACE: &str = "[weather]\nplace = Berlin\ncoordinates = 52.52,13.41\n";

struct App {
    kernel: Kernel,
    now: u64,
}

impl App {
    /// The app with the weather widgets on its first page, `config`
    /// added, the weather kept if `kept`, the time `minutes` after the
    /// weather was fetched.
    fn new(config: &str, kept: bool, minutes: i64) -> Self {
        let mut storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
        if kept {
            storage = storage.with(KEPT, include_str!("../testdata/weather.txt.v1"));
        }
        let config = format!(
            "[core]\nlanguage = en\n[help]\nshow = false\n[home]\npage-1 = weather.now 0/0 4x2, weather.forecast 0/2 4x2\n{config}"
        );
        let builder = Kernel::builder(DEVICE)
            .storage(storage)
            .network(true)
            .user_config(config);
        let mut kernel = paperpaw_app::install(builder, paperpaw_app::Fonts::Packs)
            .build()
            .unwrap();
        let clock = WallClock {
            at: Instant(0),
            unix_millis: (FETCHED + minutes * 60) * 1000,
            utc_offset_minutes: Some(120),
        };
        let status = SystemStatus {
            clock: Some(clock),
            battery: None,
        };
        let _ = kernel.set_status(Instant(0), status);
        let _ = kernel.start(Instant(0));
        Self { kernel, now: 0 }
    }

    /// The requests made, by their addresses.
    fn requests(&mut self) -> Vec<(u32, String)> {
        self.kernel
            .take_io()
            .into_iter()
            .filter_map(|io| match io {
                Io::Http(call) => Some((call.id, call.url)),
                _ => None,
            })
            .collect()
    }

    /// The texts on the screen, drawn anew a minute later (when widgets
    /// read what they show again).
    fn texts(&mut self) -> Vec<String> {
        self.now += 61_000;
        let _ = self.kernel.tick(Instant(self.now));
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|part| part.label)
            .collect()
    }

    /// How many pixels of the screen are drawn dark, between `top` and
    /// `bottom`.
    fn ink(&mut self, top: u16, bottom: u16) -> usize {
        let mut dark = 0;
        self.kernel.render(&mut |y, view| {
            for row in 0..view.height {
                if !(top..bottom).contains(&(y + row)) {
                    continue;
                }
                dark += (0..view.width)
                    .filter(|&x| view.pixel(x, row) == Color::BLACK)
                    .count();
            }
        });
        dark
    }
}

#[test]
fn the_widgets_show_the_weather_kept() {
    let mut app = App::new(PLACE, true, 10);
    // Fresh: nothing asked for.
    assert!(app.requests().is_empty());
    // The symbols and digits draw.
    let ink = app.ink(40, 760);
    assert!(ink > 2000, "{ink}");
    let texts = app.texts();
    for shown in [
        "Berlin",
        "12°",
        "Partly cloudy",
        "H 15° · L 6°",
        "Today",
        "Thu",
        "-1°",
    ] {
        assert!(texts.iter().any(|t| t == shown), "{shown} in {texts:?}");
    }
}

#[test]
fn without_a_place_nothing_is_sent() {
    let mut app = App::new("", true, 600);
    assert!(app.requests().is_empty());
    let texts = app.texts();
    assert!(
        texts.iter().any(|t| t == "Choose a place for the weather"),
        "{texts:?}"
    );
    assert!(app.requests().is_empty());
}

#[test]
fn old_weather_is_fetched_and_shown() {
    let mut app = App::new(
        &format!("{PLACE}[weather]\ntemperature = weather.unit.fahrenheit\n"),
        true,
        3 * 24 * 60,
    );
    let texts = app.texts();
    assert!(
        texts.iter().any(|t| t == "Berlin · 3 days ago"),
        "{texts:?}"
    );
    // Only the coordinates go out.
    let requests = app.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    let (id, url) = &requests[0];
    assert!(
        url.starts_with("https://api.open-meteo.com/v1/forecast?latitude=52.52&longitude=13.41&")
    );
    assert!(!url.contains("Berlin"));
    // The answer is kept and shown, in the unit chosen.
    let body = include_str!("../testdata/forecast.json")
        .replace("\"temperature_2m\":12.4", "\"temperature_2m\":21.5");
    let response = HttpResponse {
        id: *id,
        status: 200,
        body: body.into_bytes(),
        ..HttpResponse::default()
    };
    let _ = app.kernel.http_response(Instant(app.now), response);
    let kept = paperpaw_sdk::read_file(app.kernel.storage(), KEPT, 4096).unwrap();
    assert!(String::from_utf8(kept).unwrap().contains("now 215 "));
    let texts = app.texts();
    assert!(texts.iter().any(|t| t == "71°"), "{texts:?}");
    assert!(!texts.iter().any(|t| t.contains("ago")), "{texts:?}");
    // Not again while it is fresh.
    assert!(app.requests().is_empty());
}

#[test]
fn a_failed_fetch_keeps_what_was_kept() {
    let mut app = App::new(PLACE, true, 5 * 60);
    let requests = app.requests();
    assert_eq!(requests.len(), 1);
    let failed = HttpResponse::failed(
        requests[0].0,
        paperpaw_sdk::HttpError::NoNetwork,
        "no network",
    );
    let _ = app.kernel.http_response(Instant(app.now), failed);
    let texts = app.texts();
    assert!(texts.iter().any(|t| t == "12°"), "{texts:?}");
    assert!(
        texts.iter().any(|t| t == "Berlin · 5 hours ago"),
        "{texts:?}"
    );
    // Not tried again at once.
    assert!(app.requests().is_empty());
}
