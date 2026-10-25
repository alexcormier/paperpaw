//! Hot code: [`hot!`](crate::hot), for code plugins (ADR 0030).

/// Marks functions as hot: the few small loops a plugin spends its time
/// in, such as what draws or emulates a frame. Where a device has memory
/// faster than its plugins' code, it runs them from there: the X4 copies
/// hot code into internal RAM, where a loop that misses the flash's cache
/// runs several times faster. Elsewhere it runs where it is, as without
/// the mark.
///
/// ```ignore
/// paperpaw_sdk::hot! {
///     fn step(cpu: &mut Cpu, memory: &mut [u8]) {
///         // run until the frame is drawn
///     }
/// }
/// ```
///
/// - **What to mark**: functions that run every frame and loop, not
///   their callers. Only functions: no statics or constants.
/// - **What it costs**: on the X4, internal RAM the heap would have had,
///   at most 16 KiB a plugin (`paperpaw_native::index::HOT_MAX`) and what
///   the firmware's arena has left. Hot code that does not fit runs from
///   flash, as without the mark. The plugin's load log says what it took.
/// - **Calls across the edge are fine**: hot code may call cold code and
///   the other way round, and read constants. Two functions that reach
///   each other otherwise than by a call or a load (a jump of short
///   reach, a difference of addresses) are both hot or neither;
///   `paperpaw-sign pack` names them where not.
///
/// For now only RISC-V moves hot code (the X4); elsewhere the mark
/// changes nothing in the build.
#[macro_export]
macro_rules! hot {
    ($($item:item)*) => {
        $(
            #[cfg_attr(
                all(target_os = "none", target_arch = "riscv32"),
                unsafe(link_section = ".text.paperpaw_hot")
            )]
            $item
        )*
    };
}
