#![forbid(unsafe_code)]

pub mod cli;
pub mod compile;
pub mod document;
pub mod dsl;
pub mod infer;
pub mod output;
pub mod pipeline;
pub mod progress;
pub mod refusal;
pub mod registry;
pub mod witness;

/// Run the fingerprint CLI. Returns an exit code (0, 1, or 2).
pub fn run() -> u8 {
    use clap::Parser;
    // Parse CLI args (handles --version and --help via clap, then exits)
    let _cli = cli::Cli::parse();
    todo!()
}
