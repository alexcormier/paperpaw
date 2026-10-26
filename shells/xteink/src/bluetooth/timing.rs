//! Remote-test-only stage timing. No device addresses, keys or report payloads.
use embassy_time::Instant;

pub(super) struct Stage {
    started: Instant,
    link: u32,
    name: &'static str,
}

impl Stage {
    pub(super) fn start(link: u32, name: &'static str) -> Self {
        let started = Instant::now();
        log::info!(
            "bluetooth timing: link {link} {name} start at_ms={}",
            started.as_millis()
        );
        Self {
            started,
            link,
            name,
        }
    }

    pub(super) fn finish(self, outcome: &str) {
        log::info!(
            "bluetooth timing: link {} {} {} elapsed_ms={}",
            self.link,
            self.name,
            outcome,
            self.started.elapsed().as_millis()
        );
    }
}
