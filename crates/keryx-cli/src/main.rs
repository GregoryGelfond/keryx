//! The `keryx` binary — a shim over the library. All capability lives in `keryx_cli` (the CLI
//! is a satellite, architecture §3), so the driver is nameable by the test suite and the exit
//! contract is a type, not a raw process code.
#![forbid(unsafe_code)]

fn main() -> keryx_cli::exit::Exit {
    keryx_cli::run()
}
