//! The frame test (`samples/frame-test`, ADR 0030) as a native plugin
//! file, to measure its work as hot code (from internal RAM on the X4)
//! and as ordinary code: `cargo x4 --example frame_test`, then pack it
//! as the sample, into `plugins/frame-test/code.ppn`.

#![no_std]
#![no_main]

paperpaw_sdk::plugin!(paperpaw_sample_frame_test::FrameTest);
