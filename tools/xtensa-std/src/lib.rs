//! Nothing of its own: building this crate with `-Zbuild-std` builds
//! `core`, `alloc` and `compiler_builtins` for the ESP32-S3, which
//! `build.sh` packs (see `README.md`).

#![no_std]
