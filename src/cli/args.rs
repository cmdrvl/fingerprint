use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
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

    /// Include assertion failure context and evaluate all assertions
    #[arg(long)]
    pub diagnose: bool,

    /// Print operator.json and exit
    #[arg(long)]
    pub describe: bool,

    /// Print JSON Schema and exit
    #[arg(long)]
    pub schema: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Compile DSL fingerprint to Rust crate
    Compile {
        /// DSL fingerprint file (.fp.yaml)
        #[arg(value_name = "YAML", required_unless_present = "schema")]
        yaml: Option<PathBuf>,

        /// Output directory for generated crate
        #[arg(long, requires = "yaml")]
        out: Option<PathBuf>,

        /// Validate only, don't generate code
        #[arg(long, requires = "yaml")]
        check: bool,

        /// Print JSON Schema for .fp.yaml and exit
        #[arg(long, conflicts_with_all = ["yaml", "out", "check"])]
        schema: bool,
    },
    /// Query the witness ledger
    Witness {
        #[command(subcommand)]
        action: WitnessAction,
    },
    /// Infer fingerprint definition from example documents
    Infer {
        /// Directory of example documents
        dir: PathBuf,

        /// Expected format
        #[arg(long, value_name = "FMT")]
        format: String,

        /// Fingerprint ID
        #[arg(long, value_name = "ID")]
        id: String,

        /// Minimum confidence threshold for inferred assertions
        #[arg(long = "min-confidence", default_value_t = 0.9)]
        min_confidence: f64,

        /// Disable extract/content_hash suggestions
        #[arg(long)]
        no_extract: bool,

        /// Output .fp.yaml path (default: stdout)
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Infer fingerprint from a document + field values
    InferSchema {
        /// Example document
        #[arg(long)]
        doc: PathBuf,

        /// Field definitions YAML
        #[arg(long)]
        fields: PathBuf,

        /// Fingerprint ID
        #[arg(long)]
        id: Option<String>,

        /// Output .fp.yaml path (default: stdout)
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
pub enum WitnessAction {
    /// Query witness records
    Query,
    /// Show last witness record
    Last,
    /// Count witness records
    Count,
}

#[cfg(test)]
mod tests {
    use super::{Cli, Command, WitnessAction};
    use clap::Parser;
    use std::path::PathBuf;

    #[test]
    fn parses_run_mode_flags() {
        let cli = Cli::parse_from([
            "fingerprint",
            "--fp",
            "argus-model.v1",
            "--fp",
            "csv.v0",
            "--jobs",
            "4",
            "--no-witness",
            "--progress",
            "--diagnose",
        ]);

        assert!(cli.command.is_none());
        assert_eq!(
            cli.fingerprints,
            vec!["argus-model.v1".to_owned(), "csv.v0".to_owned()]
        );
        assert_eq!(cli.jobs, Some(4));
        assert!(cli.no_witness);
        assert!(cli.progress);
        assert!(cli.diagnose);
    }

    #[test]
    fn parses_compile_subcommand() {
        let cli = Cli::parse_from([
            "fingerprint",
            "compile",
            "argus-model.fp.yaml",
            "--out",
            "out-dir",
            "--check",
        ]);

        match cli.command {
            Some(Command::Compile {
                yaml,
                out,
                check,
                schema,
            }) => {
                assert_eq!(yaml, Some(PathBuf::from("argus-model.fp.yaml")));
                assert_eq!(out, Some(PathBuf::from("out-dir")));
                assert!(check);
                assert!(!schema);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_compile_schema_subcommand_without_yaml() {
        let cli = Cli::parse_from(["fingerprint", "compile", "--schema"]);

        match cli.command {
            Some(Command::Compile {
                yaml,
                out,
                check,
                schema,
            }) => {
                assert_eq!(yaml, None);
                assert_eq!(out, None);
                assert!(!check);
                assert!(schema);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_witness_subcommands() {
        let query = Cli::parse_from(["fingerprint", "witness", "query"]);
        let last = Cli::parse_from(["fingerprint", "witness", "last"]);
        let count = Cli::parse_from(["fingerprint", "witness", "count"]);

        match query.command {
            Some(Command::Witness {
                action: WitnessAction::Query,
            }) => {}
            other => panic!("unexpected witness query command: {other:?}"),
        }
        match last.command {
            Some(Command::Witness {
                action: WitnessAction::Last,
            }) => {}
            other => panic!("unexpected witness last command: {other:?}"),
        }
        match count.command {
            Some(Command::Witness {
                action: WitnessAction::Count,
            }) => {}
            other => panic!("unexpected witness count command: {other:?}"),
        }
    }

    #[test]
    fn parses_infer_and_infer_schema_subcommands() {
        let infer = Cli::parse_from([
            "fingerprint",
            "infer",
            "fixtures",
            "--format",
            "xlsx",
            "--id",
            "argus-model.v1",
            "--min-confidence",
            "0.95",
            "--no-extract",
            "--out",
            "out.fp.yaml",
        ]);
        let infer_schema = Cli::parse_from([
            "fingerprint",
            "infer-schema",
            "--doc",
            "appraisal.md",
            "--fields",
            "fields.yaml",
            "--id",
            "cbre-appraisal.v1",
            "--out",
            "cbre.fp.yaml",
        ]);

        match infer.command {
            Some(Command::Infer {
                dir,
                format,
                id,
                min_confidence,
                no_extract,
                out,
            }) => {
                assert_eq!(dir, PathBuf::from("fixtures"));
                assert_eq!(format, "xlsx");
                assert_eq!(id, "argus-model.v1");
                assert_eq!(min_confidence, 0.95);
                assert!(no_extract);
                assert_eq!(out, Some(PathBuf::from("out.fp.yaml")));
            }
            other => panic!("unexpected infer command: {other:?}"),
        }

        match infer_schema.command {
            Some(Command::InferSchema {
                doc,
                fields,
                id,
                out,
            }) => {
                assert_eq!(doc, PathBuf::from("appraisal.md"));
                assert_eq!(fields, PathBuf::from("fields.yaml"));
                assert_eq!(id.as_deref(), Some("cbre-appraisal.v1"));
                assert_eq!(out, Some(PathBuf::from("cbre.fp.yaml")));
            }
            other => panic!("unexpected infer-schema command: {other:?}"),
        }
    }
}
