//! paperpaw-dev for this plugin: PaperPaw's standard app with the plugin
//! added, run on emulated devices and checked.
//!
//! ```sh
//! cargo run --example paperpaw-dev -- check --plugin my-plugin --quick
//! cargo run --example paperpaw-dev -- gallery --plugin my-plugin
//! cargo run --example paperpaw-dev -- --help
//! ```
//!
//! The report is in `target/paperpaw-dev/report.html`.

fn main() -> std::process::ExitCode {
    // The plugin as the app gets it; more plugins with more `.plugin(...)`.
    paperpaw_dev::main(|app| app.plugin(my_plugin::MyPlugin))
}
