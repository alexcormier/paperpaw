//! `paperpaw-dev`: checks PaperPaw plugins (see the library's docs).

fn main() -> std::process::ExitCode {
    paperpaw_dev::cli::main(std::env::args().skip(1), None)
}
