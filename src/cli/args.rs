use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "fingerprint", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// JSONL manifest file (default: stdin)
    #[arg(value_name = "INPUT")]
    pub input: Option<PathBuf>,

    /// Fingerprint ID to test (repeatable; evaluated in CLI order, first match wins)
    #[arg(long = "fp", value_name = "ID")]
    pub fingerprints: Vec<String>,

    /// List available fingerprints and exit
    #[arg(long)]
    pub list: bool,

    /// Number of parallel workers (default: CPU count)
    #[arg(long)]
    pub jobs: Option<usize>,

    /// Suppress witness ledger recording
    #[arg(long)]
    pub no_witness: bool,

    /// Emit progress to stderr
    #[arg(long)]
    pub progress: bool,

    /// Print operator.json and exit
    #[arg(long)]
    pub describe: bool,

    /// Print JSON Schema and exit
    #[arg(long)]
    pub schema: bool,
}

#[derive(Subcommand)]
pub enum Command {
    /// Compile DSL fingerprint to Rust crate
    Compile {
        /// DSL fingerprint file (.fp.yaml)
        yaml: PathBuf,

        /// Output directory for generated crate
        #[arg(long)]
        out: Option<PathBuf>,

        /// Validate only, don't generate code
        #[arg(long)]
        check: bool,

        /// Print DSL JSON Schema and exit
        #[arg(long)]
        schema: bool,
    },
    /// Query the witness ledger
    Witness {
        #[command(subcommand)]
        action: WitnessAction,
    },
    /// Infer a fingerprint definition from example files
    Infer {
        /// Directory of example files to observe
        #[arg(value_name = "DIR")]
        dir: PathBuf,

        /// Output file for generated .fp.yaml (default: stdout)
        #[arg(long)]
        out: Option<PathBuf>,

        /// Expected format (xlsx, csv, pdf)
        #[arg(long)]
        format: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum WitnessAction {
    /// Query witness records
    Query,
    /// Show last witness record
    Last,
    /// Count witness records
    Count,
}
