# Bluetooth shell identity regressions

This standalone, unpublished host-test crate executes the Pro firmware's actual
identity module against its vendored `trouble-host`, with public cryptographic
fixtures. It is separate so embedded/host dependency features and vendored
formatting do not enter the main workspace. No radio or device is needed.

From the repository root (replace the target with your host on other systems):

```sh
cargo test --manifest-path tools/bluetooth-tests/Cargo.toml --locked --target x86_64-unknown-linux-gnu
cargo +1.88 test --manifest-path tools/bluetooth-tests/Cargo.toml --locked --target x86_64-unknown-linux-gnu
cargo clippy --manifest-path tools/bluetooth-tests/Cargo.toml --locked --all-targets --target x86_64-unknown-linux-gnu -- -D warnings
cargo fmt --manifest-path tools/bluetooth-tests/Cargo.toml --check
```

CI runs these tests on stable and the supported minimum Rust. The lockfile must
be refreshed when workspace package versions change. The main workspace's
format check remains unchanged; this crate's check formats only its own source
and the included shell module, without traversing the vendored dependency.
