use clap::{Args, Parser, Subcommand};
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
    #[arg(long = "fp", alias = "fingerprint", value_name = "ID")]
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

        /// Pre-extracted markdown/text for PDF content inference
        #[arg(long = "text-path")]
        text_path: Option<PathBuf>,

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
    Query {
        #[command(flatten)]
        filters: WitnessFilters,

        /// Emit a JSON array instead of JSONL
        #[arg(long)]
        json: bool,
    },
    /// Show last witness record
    Last {
        #[command(flatten)]
        filters: WitnessFilters,

        /// Emit JSON output
        #[arg(long)]
        json: bool,
    },
    /// Count witness records
    Count {
        #[command(flatten)]
        filters: WitnessFilters,

        /// Emit JSON output
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Args, Clone, Default, PartialEq, Eq)]
pub struct WitnessFilters {
    /// Restrict matches to a specific tool
    #[arg(long)]
    pub tool: Option<String>,

    /// Only include records at or after this RFC3339 timestamp
    #[arg(long)]
    pub since: Option<String>,

    /// Only include records at or before this RFC3339 timestamp
    #[arg(long)]
    pub until: Option<String>,

    /// Only include records with this outcome
    #[arg(long)]
    pub outcome: Option<String>,

    /// Only include records whose inputs include this hash
    #[arg(long = "input-hash")]
    pub input_hash: Option<String>,
}

impl WitnessFilters {
    pub fn is_active(&self) -> bool {
        self.tool.is_some()
            || self.since.is_some()
            || self.until.is_some()
            || self.outcome.is_some()
            || self.input_hash.is_some()
    }
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

        let command = cli.command;
        assert!(matches!(command, Some(Command::Compile { .. })));
        if let Some(Command::Compile {
            yaml,
            out,
            check,
            schema,
        }) = command
        {
            assert_eq!(yaml, Some(PathBuf::from("argus-model.fp.yaml")));
            assert_eq!(out, Some(PathBuf::from("out-dir")));
            assert!(check);
            assert!(!schema);
        }
    }

    #[test]
    fn parses_compile_schema_subcommand_without_yaml() {
        let cli = Cli::parse_from(["fingerprint", "compile", "--schema"]);

        let command = cli.command;
        assert!(matches!(command, Some(Command::Compile { .. })));
        if let Some(Command::Compile {
            yaml,
            out,
            check,
            schema,
        }) = command
        {
            assert_eq!(yaml, None);
            assert_eq!(out, None);
            assert!(!check);
            assert!(schema);
        }
    }

    #[test]
    fn parses_witness_subcommands() {
        let query = Cli::parse_from([
            "fingerprint",
            "witness",
            "query",
            "--tool",
            "fingerprint",
            "--since",
            "2026-01-01T00:00:00Z",
            "--until",
            "2026-01-31T23:59:59Z",
            "--outcome",
            "ALL_MATCHED",
            "--input-hash",
            "blake3:abc",
            "--json",
        ]);
        let last = Cli::parse_from([
            "fingerprint",
            "witness",
            "last",
            "--tool",
            "fingerprint",
            "--json",
        ]);
        let count = Cli::parse_from([
            "fingerprint",
            "witness",
            "count",
            "--since",
            "2026-02-01T00:00:00Z",
        ]);

        let query_command = query.command;
        assert!(matches!(
            query_command,
            Some(Command::Witness {
                action: WitnessAction::Query { .. }
            })
        ));
        if let Some(Command::Witness {
            action: WitnessAction::Query { filters, json },
        }) = query_command
        {
            assert_eq!(filters.tool.as_deref(), Some("fingerprint"));
            assert_eq!(filters.since.as_deref(), Some("2026-01-01T00:00:00Z"));
            assert_eq!(filters.until.as_deref(), Some("2026-01-31T23:59:59Z"));
            assert_eq!(filters.outcome.as_deref(), Some("ALL_MATCHED"));
            assert_eq!(filters.input_hash.as_deref(), Some("blake3:abc"));
            assert!(json);
        }

        let last_command = last.command;
        assert!(matches!(
            last_command,
            Some(Command::Witness {
                action: WitnessAction::Last { .. }
            })
        ));
        if let Some(Command::Witness {
            action: WitnessAction::Last { filters, json },
        }) = last_command
        {
            assert_eq!(filters.tool.as_deref(), Some("fingerprint"));
            assert!(json);
        }

        let count_command = count.command;
        assert!(matches!(
            count_command,
            Some(Command::Witness {
                action: WitnessAction::Count { .. }
            })
        ));
        if let Some(Command::Witness {
            action: WitnessAction::Count { filters, json },
        }) = count_command
        {
            assert_eq!(filters.since.as_deref(), Some("2026-02-01T00:00:00Z"));
            assert!(!json);
        }
    }

    #[test]
    fn parses_fingerprint_alias_for_fp_flag() {
        let cli = Cli::parse_from([
            "fingerprint",
            "--fingerprint",
            "argus-model.v1",
            "--fingerprint",
            "csv.v0",
        ]);

        assert_eq!(
            cli.fingerprints,
            vec!["argus-model.v1".to_owned(), "csv.v0".to_owned()]
        );
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
            "--text-path",
            "appraisal.extracted.md",
            "--fields",
            "fields.yaml",
            "--id",
            "cbre-appraisal.v1",
            "--out",
            "cbre.fp.yaml",
        ]);

        let infer_command = infer.command;
        assert!(matches!(infer_command, Some(Command::Infer { .. })));
        if let Some(Command::Infer {
            dir,
            format,
            id,
            min_confidence,
            no_extract,
            out,
        }) = infer_command
        {
            assert_eq!(dir, PathBuf::from("fixtures"));
            assert_eq!(format, "xlsx");
            assert_eq!(id, "argus-model.v1");
            assert_eq!(min_confidence, 0.95);
            assert!(no_extract);
            assert_eq!(out, Some(PathBuf::from("out.fp.yaml")));
        }

        let infer_schema_command = infer_schema.command;
        assert!(matches!(
            infer_schema_command,
            Some(Command::InferSchema { .. })
        ));
        if let Some(Command::InferSchema {
            doc,
            text_path,
            fields,
            id,
            out,
        }) = infer_schema_command
        {
            assert_eq!(doc, PathBuf::from("appraisal.md"));
            assert_eq!(text_path, Some(PathBuf::from("appraisal.extracted.md")));
            assert_eq!(fields, PathBuf::from("fields.yaml"));
            assert_eq!(id.as_deref(), Some("cbre-appraisal.v1"));
            assert_eq!(out, Some(PathBuf::from("cbre.fp.yaml")));
        }
    }
}
