//! Digests PaperPaw needs to work with other programs and to name files:
//! [`md5`] (KOReader's fingerprint of a book file, its sync keys),
//! [`sha1`] (Calibre's and others' needs) and [`fnv1a64`] (short, stable
//! ids in names of files). None of them is meant for security. Also
//! bytes written as [`hex`] digits, as digests are.

#![no_std]

extern crate alloc;

mod fnv;
pub mod hex;
pub mod md5;
pub mod sha1;

pub use fnv::fnv1a64;
