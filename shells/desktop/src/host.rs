//! What the emulated device takes from the computer it runs on: the time
//! that passes and random numbers. On a computer with an operating system
//! they come from it; in a browser (`wasm32`), where neither is reachable
//! from Rust alone, the shell hands them in ([`set_micros`],
//! [`set_random`]).

/// Random bytes into the buffer; `false` where there are none.
pub type Random = fn(&mut [u8]) -> bool;

/// Microseconds since the emulator started: for the panel's animations,
/// the time plugins take and the frames traced.
#[cfg(not(target_arch = "wasm32"))]
pub fn micros() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_micros() as u64
}

/// Microseconds since the emulator started, as the shell last said
/// ([`set_micros`]).
#[cfg(target_arch = "wasm32")]
pub fn micros() -> u64 {
    MICROS.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(target_arch = "wasm32")]
static MICROS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Tells the time that has passed since the emulator started, where the
/// operating system cannot (a browser's `performance.now()`); it never
/// goes back.
#[cfg(target_arch = "wasm32")]
pub fn set_micros(micros: u64) {
    MICROS.fetch_max(micros, std::sync::atomic::Ordering::Relaxed);
}

/// Random bytes from the operating system, or from the source the shell
/// set ([`set_random`]).
pub fn random(buf: &mut [u8]) -> bool {
    match RANDOM.get() {
        Some(random) => random(buf),
        None => system_random(buf),
    }
}

static RANDOM: std::sync::OnceLock<Random> = std::sync::OnceLock::new();

/// Takes random numbers from `random` (a browser's
/// `crypto.getRandomValues`), where the operating system gives none.
/// Only the first call counts.
pub fn set_random(random: Random) {
    let _ = RANDOM.set(random);
}

#[cfg(not(target_arch = "wasm32"))]
fn system_random(buf: &mut [u8]) -> bool {
    getrandom::fill(buf).is_ok()
}

#[cfg(target_arch = "wasm32")]
fn system_random(_: &mut [u8]) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_goes_on_and_random_bytes_come() {
        let first = micros();
        let mut buf = [0u8; 32];
        assert!(random(&mut buf));
        assert!(buf.iter().any(|&b| b != 0));
        assert!(micros() >= first);
    }
}
