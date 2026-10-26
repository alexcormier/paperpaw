//! Progress before the kernel's first frame. The web bundle is verified
//! and copied before plugins load, so its work must have a screen already.

use alloc::format;

use log::{info, warn};
use paperpaw_core::sdk::Canvas;
use paperpaw_graphics::Framebuffer;
use paperpaw_platform::{Instant, PixelFormat, Rect};
use paperpaw_x4::display::{Error, Planes, Refresh};

use crate::board::Panel;
use crate::native::bundle::Progress;

/// A few rows in memory, never a second whole frame.
const ROWS: u16 = 40;
/// Do not add a panel refresh for every copied block or small plugin.
const UPDATE_MS: u64 = 1500;

pub struct Startup<'a> {
    panel: &'a mut Panel,
    screen: Option<paperpaw_app::startup::Screen>,
    attempted: bool,
    last: Option<Instant>,
    copying: bool,
}

impl<'a> Startup<'a> {
    pub fn new(panel: &'a mut Panel) -> Self {
        Self {
            panel,
            screen: None,
            attempted: false,
            last: None,
            copying: false,
        }
    }

    pub fn update(&mut self, progress: Progress<'_>) {
        if !self.attempted {
            self.attempted = true;
            self.screen = match paperpaw_app::startup::Screen::new() {
                Ok(screen) => Some(screen),
                Err(error) => {
                    warn!("startup display: {error:?}");
                    None
                }
            };
        }
        let copying = matches!(progress, Progress::Copying { .. });
        if copying
            && self.copying
            && self
                .last
                .is_some_and(|last| crate::now().since(last) < UPDATE_MS)
        {
            return;
        }
        self.copying = copying;
        let (phase, detail, done, total) = match progress {
            Progress::Checking => ("Checking plugins", "From the web flasher".into(), 0, 0),
            Progress::Copying {
                plugin,
                index,
                total,
                done,
                bytes,
            } => (
                "Installing plugins",
                format!("{plugin} ({index}/{total})"),
                (index.saturating_sub(1) as u32) * 100 + done.saturating_mul(100) / bytes.max(1),
                total as u32 * 100,
            ),
            Progress::Starting => ("Starting PaperPaw", "Preparing your plugins".into(), 0, 0),
            Progress::Damaged => (
                "Plugin bundle is damaged",
                "Please flash your selected plugins again.".into(),
                0,
                0,
            ),
            Progress::Failed => (
                "Plugins could not be installed",
                "Check the SD card. Will retry next start.".into(),
                0,
                0,
            ),
        };
        if let Some(screen) = &self.screen {
            match draw(self.panel, self.last.is_none(), |canvas, rect| {
                screen.draw(canvas, rect, phase, &detail, done, total);
            }) {
                Ok(()) => self.last = Some(crate::now()),
                Err(error) => {
                    warn!("startup display: {error}");
                    self.screen = None;
                }
            }
        }
    }
}

/// Show feedback before the running kernel flushes its state and resets.
/// Reuse its font registry; only a small, fallible display band is needed.
pub fn restarting(panel: &mut Panel, kernel: &paperpaw_core::Kernel) {
    let tr = kernel.translator();
    let labels = [
        tr.tr("core.restarting.title"),
        tr.tr("core.restarting.text"),
        tr.tr("core.restarting.keep-on"),
    ];
    match draw(panel, true, |canvas, rect| {
        paperpaw_app::startup::restarting(kernel.fonts(), canvas, rect, labels);
    }) {
        Ok(()) => info!("restart display: shown"),
        Err(error) => warn!("restart display: {error}; continuing restart"),
    }
}

#[derive(Debug)]
enum Problem {
    Memory,
    Display(Error),
}

impl core::fmt::Display for Problem {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Memory => f.write_str("not enough memory for a display band"),
            Self::Display(error) => write!(f, "{error:?}"),
        }
    }
}

impl From<Error> for Problem {
    fn from(error: Error) -> Self {
        Self::Display(error)
    }
}

fn draw(
    panel: &mut Panel,
    first: bool,
    paint: impl Fn(&mut Canvas<'_>, Rect),
) -> Result<(), Problem> {
    let (width, height) = panel.size();
    let mut frame = Framebuffer::banded(width, height, ROWS, PixelFormat::Mono1)
        .map_err(|_| Problem::Memory)?;
    let mut send = |planes| -> Result<(), Error> {
        let mut top = 0;
        while top < height {
            crate::power::feed();
            frame.set_top(top);
            paint(&mut frame.canvas(), Rect::from_size(width, height));
            panel.write_rows(top, frame.view(), planes)?;
            top += ROWS;
        }
        Ok(())
    };
    send(if first { Planes::BOTH } else { Planes::NEW })?;
    panel.refresh(if first { Refresh::Half } else { Refresh::Fast }, true)?;
    // The old and new memories must agree after a fast refresh, as when
    // the kernel draws. Replay the small bands, rather than keep a frame.
    let mut top = 0;
    while top < height {
        crate::power::feed();
        frame.set_top(top);
        paint(&mut frame.canvas(), Rect::from_size(width, height));
        panel.write_rows(top, frame.view(), Planes::BOTH)?;
        top += ROWS;
    }
    Ok(())
}
