//! The interlinear sample (`samples/interlinear`, ADR 0032) as a native
//! plugin file: `cargo x4 --example interlinear` (or `cargo +esp x4pro
//! --example interlinear`), then pack it as the sample, into
//! `plugins/interlinear/code.ppn`.

#![no_std]
#![no_main]

paperpaw_sdk::plugin!(paperpaw_sample_interlinear::Interlinear);
