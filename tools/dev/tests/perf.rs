//! What `perf` looks for, it finds: a test plugin with a screen and a
//! widget that read while they draw (as screens once did, see
//! `crates/paperpaw-app/tests/draw.rs`), a screen too large for the X4's
//! heap and one that leaks a little each time it opens.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::borrow::Cow;
use std::sync::Mutex;

use paperpaw_core::KernelBuilder;
use paperpaw_dev::cli::Context;
use paperpaw_dev::perf;
use paperpaw_dev::report::{Level, Report};
use paperpaw_dev::target::Target;
use paperpaw_platform::Rect;
use paperpaw_sdk::widgets::{Widget, WidgetSpec};
use paperpaw_sdk::{
    Canvas, Cx, Env, Handled, Id, Manifest, Plugin, RegisterError, Registrar, SDK_VERSION, Screen,
    ScreenSpec,
};

/// Reads a file whenever it is drawn.
struct Reads;

impl Screen for Reads {
    fn draw(&mut self, _: &mut Canvas<'_>, env: &Env<'_>) {
        let _ = paperpaw_sdk::read_file(env.storage, "notes.txt", 1024);
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

/// Keeps 200 KiB while it is open: more than the X4 has free.
struct Big(#[allow(dead_code)] Vec<u8>);

impl Screen for Big {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

/// What the leaking screen keeps, never let go.
static LEAKED: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());

struct Leaks;

impl Screen for Leaks {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

/// A widget reading a file whenever it is drawn.
struct ReadsWidget;

impl Widget for ReadsWidget {
    fn live(&self) -> bool {
        false
    }

    fn height(&self, _: &Env<'_>, _: u16) -> u16 {
        1
    }

    fn draw(&self, _: &mut Canvas<'_>, env: &Env<'_>, _: Rect) {
        let _ = paperpaw_sdk::read_file(env.storage, "notes.txt", 1024);
    }
}

struct Probe;

impl Plugin for Probe {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: Id::new_static("perf-probe"),
            name: Cow::Borrowed("Perf probe"),
            description: Cow::Borrowed("Screens that break the X4's budgets."),
            version: SDK_VERSION,
            sdk: SDK_VERSION,
            uses: paperpaw_sdk::Uses::NOTHING,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec {
            id: Id::new_static("perf-probe.reads"),
            title: Cow::Borrowed("perf-probe.reads"),
            factory: Box::new(|_, _| Box::new(Reads)),
        })?;
        r.add_screen(ScreenSpec {
            id: Id::new_static("perf-probe.big"),
            title: Cow::Borrowed("perf-probe.big"),
            factory: Box::new(|_, _| Box::new(Big(vec![1; 200 * 1024]))),
        })?;
        r.add_screen(ScreenSpec {
            id: Id::new_static("perf-probe.leaks"),
            title: Cow::Borrowed("perf-probe.leaks"),
            factory: Box::new(|_, _| {
                LEAKED.lock().unwrap().push(vec![1; 1024]);
                Box::new(Leaks)
            }),
        })?;
        r.add_widget(WidgetSpec::new(
            Id::new_static("perf-probe.widget"),
            "perf-probe.widget",
            |_| Box::new(ReadsWidget),
        ))
    }
}

fn add(builder: KernelBuilder) -> KernelBuilder {
    builder.plugin(Probe)
}

#[test]
fn reads_while_drawing_large_screens_and_leaks_are_found() {
    let out = std::env::temp_dir().join(format!("paperpaw-dev-perf-{}", std::process::id()));
    let mut context = Context::new(
        Target {
            plugins: vec!["perf-probe".into()],
            add: Some(add),
            ..Target::default()
        },
        out,
    );
    context.quick = true;
    let mut report = Report::default();
    perf::run(&context, &mut report).unwrap();
    let found = |check: &str, place: &str, level: Level| {
        report
            .findings
            .iter()
            .filter(|f| f.check == check && f.place == place && f.level == level)
            .map(|f| f.message.clone())
            .collect::<Vec<_>>()
    };
    let text = report.text(Level::Info);

    // The file is not there: reading it asks its size, in vain.
    let asked = |m: &String| m.contains("drawn again, band 1: size notes.txt");
    let reads = found("perf.draw-io", "perf-probe.reads", Level::Error);
    assert!(reads.iter().any(asked), "{text}");
    let widget = found("perf.draw-io", "perf-probe.widget", Level::Error);
    assert!(widget.iter().any(asked), "{text}");
    assert!(
        !found("perf.heap", "perf-probe.big", Level::Error).is_empty(),
        "{text}"
    );
    assert!(
        !found("perf.leak", "perf-probe.leaks", Level::Warning).is_empty(),
        "{text}"
    );
    // Nothing else is wrong with them.
    for place in ["perf-probe.big", "perf-probe.leaks"] {
        assert!(
            found("perf.draw-io", place, Level::Error).is_empty(),
            "{text}"
        );
    }
    assert!(
        found("perf.heap", "perf-probe.reads", Level::Error).is_empty(),
        "{text}"
    );
    assert!(
        found("perf.leak", "perf-probe.reads", Level::Warning).is_empty(),
        "{text}"
    );
    assert!(context.out.join("perf/index.html").is_file());
    assert_eq!(report.pages[0].title, "Budgets on the X4");
    let _ = std::fs::remove_dir_all(&context.out);
}
