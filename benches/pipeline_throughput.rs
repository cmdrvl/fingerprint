use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use fingerprint::document::Document;
use fingerprint::pipeline::enricher::enrich_record_with_fingerprints;
use fingerprint::registry::builtin::register_builtins;
use fingerprint::registry::{AssertionResult, Fingerprint, FingerprintRegistry, FingerprintResult};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;

fn fixture(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

// Mock fingerprint for benchmarking
struct BenchmarkFingerprint {
    id: String,
    format: String,
    processing_complexity: ProcessingComplexity,
}

#[derive(Clone)]
enum ProcessingComplexity {
    Simple,  // Just filename check
    Medium,  // Cell/structural checks
    Complex, // Text processing with regex
}

impl BenchmarkFingerprint {
    fn new(id: &str, format: &str, complexity: ProcessingComplexity) -> Self {
        Self {
            id: id.to_owned(),
            format: format.to_owned(),
            processing_complexity: complexity,
        }
    }
}

impl Fingerprint for BenchmarkFingerprint {
    fn id(&self) -> &str {
        &self.id
    }

    fn format(&self) -> &str {
        &self.format
    }

    fn fingerprint(&self, _doc: &Document) -> FingerprintResult {
        // Simulate different processing complexities
        match self.processing_complexity {
            ProcessingComplexity::Simple => {
                // Simple processing: just return success
                FingerprintResult {
                    matched: true,
                    reason: None,
                    assertions: vec![AssertionResult {
                        name: "filename_check".to_owned(),
                        passed: true,
                        detail: Some("Simple processing".to_owned()),
                        context: None,
                    }],
                    extracted: Some(HashMap::from([("simple_data".to_owned(), json!("value"))])),
                    content_hash: Some("blake3:simple".to_owned()),
                }
            }
            ProcessingComplexity::Medium => {
                // Medium processing: simulate cell access
                std::thread::sleep(std::time::Duration::from_micros(10));
                FingerprintResult {
                    matched: true,
                    reason: None,
                    assertions: vec![
                        AssertionResult {
                            name: "sheet_check".to_owned(),
                            passed: true,
                            detail: Some("Sheet exists".to_owned()),
                            context: None,
                        },
                        AssertionResult {
                            name: "cell_check".to_owned(),
                            passed: true,
                            detail: Some("Cell value matches".to_owned()),
                            context: None,
                        },
                    ],
                    extracted: Some(HashMap::from([(
                        "medium_data".to_owned(),
                        json!({"cells": ["A1", "B1"]}),
                    )])),
                    content_hash: Some("blake3:medium".to_owned()),
                }
            }
            ProcessingComplexity::Complex => {
                // Complex processing: simulate text analysis
                std::thread::sleep(std::time::Duration::from_micros(50));
                let text = "Sample text content for processing and analysis";
                let word_count = text.split_whitespace().count();

                FingerprintResult {
                    matched: true,
                    reason: None,
                    assertions: vec![
                        AssertionResult {
                            name: "text_analysis".to_owned(),
                            passed: true,
                            detail: Some(format!("Analyzed {} words", word_count)),
                            context: None,
                        },
                        AssertionResult {
                            name: "pattern_match".to_owned(),
                            passed: true,
                            detail: Some("Found expected patterns".to_owned()),
                            context: None,
                        },
                    ],
                    extracted: Some(HashMap::from([
                        ("text_stats".to_owned(), json!({"word_count": word_count})),
                        (
                            "patterns".to_owned(),
                            json!(["sample", "content", "analysis"]),
                        ),
                    ])),
                    content_hash: Some("blake3:complex".to_owned()),
                }
            }
        }
    }
}

fn bench_single_record_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_record");

    // Test different fingerprint complexities
    for (complexity_name, complexity) in [
        ("simple", ProcessingComplexity::Simple),
        ("medium", ProcessingComplexity::Medium),
        ("complex", ProcessingComplexity::Complex),
    ] {
        let mut registry = FingerprintRegistry::new();
        registry.register(Box::new(BenchmarkFingerprint::new(
            &format!("bench-{}.v1", complexity_name),
            "csv",
            complexity,
        )));

        let record = json!({
            "path": fixture("tests/fixtures/files/sample.csv").display().to_string(),
            "hash": "blake3:bench"
        });

        let fingerprint_ids = vec![format!("bench-{}.v1", complexity_name)];

        group.bench_function(complexity_name.to_string(), |b| {
            b.iter(|| {
                black_box(enrich_record_with_fingerprints(
                    black_box(&record),
                    black_box(&registry),
                    black_box(&fingerprint_ids),
                ))
            });
        });
    }

    group.finish();
}

fn bench_batch_record_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_processing");

    // Create registry with mixed complexity fingerprints
    let mut registry = FingerprintRegistry::new();
    registry.register(Box::new(BenchmarkFingerprint::new(
        "simple.v1",
        "csv",
        ProcessingComplexity::Simple,
    )));
    registry.register(Box::new(BenchmarkFingerprint::new(
        "medium.v1",
        "xlsx",
        ProcessingComplexity::Medium,
    )));
    registry.register(Box::new(BenchmarkFingerprint::new(
        "complex.v1",
        "pdf",
        ProcessingComplexity::Complex,
    )));

    let fingerprint_ids = vec![
        "simple.v1".to_string(),
        "medium.v1".to_string(),
        "complex.v1".to_string(),
    ];

    // Test different batch sizes
    for batch_size in [1usize, 5, 10, 25, 50, 100] {
        let records: Vec<Value> = (0..batch_size)
            .map(|i| {
                json!({
                    "path": format!("test_file_{}.csv", i),
                    "hash": format!("blake3:hash{}", i)
                })
            })
            .collect();

        group.throughput(Throughput::Elements(batch_size as u64));
        group.bench_with_input(
            BenchmarkId::new("records", batch_size),
            &batch_size,
            |b, _| {
                b.iter(|| {
                    for record in &records {
                        black_box(enrich_record_with_fingerprints(
                            black_box(record),
                            black_box(&registry),
                            black_box(&fingerprint_ids),
                        ));
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_fingerprint_registry_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("registry_scaling");

    let record = json!({
        "path": fixture("tests/fixtures/files/sample.csv").display().to_string(),
        "hash": "blake3:scaling_test"
    });

    // Test with different numbers of fingerprints in registry
    for fp_count in [1usize, 5, 10, 25, 50] {
        let mut registry = FingerprintRegistry::new();
        let mut fingerprint_ids = Vec::new();

        // Add fingerprints to registry
        for i in 0..fp_count {
            let complexity = match i % 3 {
                0 => ProcessingComplexity::Simple,
                1 => ProcessingComplexity::Medium,
                _ => ProcessingComplexity::Complex,
            };

            let fp_id = format!("scaling-{}.v1", i);
            registry.register(Box::new(BenchmarkFingerprint::new(
                &fp_id, "csv", complexity,
            )));
            fingerprint_ids.push(fp_id);
        }

        group.throughput(Throughput::Elements(fp_count as u64));
        group.bench_with_input(
            BenchmarkId::new("fingerprints", fp_count),
            &fp_count,
            |b, _| {
                b.iter(|| {
                    black_box(enrich_record_with_fingerprints(
                        black_box(&record),
                        black_box(&registry),
                        black_box(&fingerprint_ids),
                    ))
                });
            },
        );
    }

    group.finish();
}

fn bench_manifest_processing_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("manifest_patterns");

    let mut registry = FingerprintRegistry::new();
    registry.register(Box::new(BenchmarkFingerprint::new(
        "pattern-test.v1",
        "csv",
        ProcessingComplexity::Medium,
    )));

    let fingerprint_ids = vec!["pattern-test.v1".to_string()];

    // Test different manifest patterns

    // Small files pattern
    let small_files: Vec<Value> = (0..20)
        .map(|i| {
            json!({
                "path": format!("small_file_{}.csv", i),
                "hash": format!("blake3:small{}", i)
            })
        })
        .collect();

    group.bench_function("small_files_many", |b| {
        b.iter(|| {
            for record in &small_files {
                black_box(enrich_record_with_fingerprints(
                    black_box(record),
                    black_box(&registry),
                    black_box(&fingerprint_ids),
                ));
            }
        });
    });

    // Mixed file types pattern
    let mixed_files: Vec<Value> = (0..10)
        .flat_map(|i| {
            vec![
                json!({"path": format!("doc_{}.csv", i), "hash": format!("blake3:csv{}", i)}),
                json!({"path": format!("sheet_{}.xlsx", i), "hash": format!("blake3:xlsx{}", i)}),
                json!({"path": format!("report_{}.pdf", i), "hash": format!("blake3:pdf{}", i)}),
            ]
        })
        .collect();

    group.bench_function("mixed_formats", |b| {
        b.iter(|| {
            for record in &mixed_files {
                black_box(enrich_record_with_fingerprints(
                    black_box(record),
                    black_box(&registry),
                    black_box(&fingerprint_ids),
                ));
            }
        });
    });

    group.finish();
}

fn bench_error_handling_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("error_handling");

    // Registry with fingerprint that always matches
    let mut success_registry = FingerprintRegistry::new();
    success_registry.register(Box::new(BenchmarkFingerprint::new(
        "always-success.v1",
        "csv",
        ProcessingComplexity::Simple,
    )));

    // Fingerprint that simulates failures
    struct FailingFingerprint;
    impl Fingerprint for FailingFingerprint {
        fn id(&self) -> &str {
            "always-fail.v1"
        }
        fn format(&self) -> &str {
            "csv"
        }
        fn fingerprint(&self, _doc: &Document) -> FingerprintResult {
            FingerprintResult {
                matched: false,
                reason: Some("Simulated failure for benchmarking".to_owned()),
                assertions: vec![AssertionResult {
                    name: "failing_assertion".to_owned(),
                    passed: false,
                    detail: Some("This assertion always fails".to_owned()),
                    context: None,
                }],
                extracted: None,
                content_hash: None,
            }
        }
    }

    let mut failure_registry = FingerprintRegistry::new();
    failure_registry.register(Box::new(FailingFingerprint));

    let record = json!({
        "path": fixture("tests/fixtures/files/sample.csv").display().to_string(),
        "hash": "blake3:error_test"
    });

    // Compare success vs failure performance
    group.bench_function("success_path", |b| {
        let fingerprint_ids = vec!["always-success.v1".to_string()];
        b.iter(|| {
            black_box(enrich_record_with_fingerprints(
                black_box(&record),
                black_box(&success_registry),
                black_box(&fingerprint_ids),
            ))
        });
    });

    group.bench_function("failure_path", |b| {
        let fingerprint_ids = vec!["always-fail.v1".to_string()];
        b.iter(|| {
            black_box(enrich_record_with_fingerprints(
                black_box(&record),
                black_box(&failure_registry),
                black_box(&fingerprint_ids),
            ))
        });
    });

    group.finish();
}

fn bench_end_to_end_record_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("end_to_end_record");
    let mut registry = FingerprintRegistry::new();
    for builtin in register_builtins() {
        registry.register(builtin);
    }
    let fingerprint_ids = vec![
        "csv.v0".to_owned(),
        "xlsx.v0".to_owned(),
        "pdf.v0".to_owned(),
        "markdown.v0".to_owned(),
    ];

    let records = vec![
        (
            "csv",
            json!({
                "version": "hash.v0",
                "path": fixture("tests/fixtures/files/sample.csv").display().to_string(),
                "extension": ".csv",
                "bytes_hash": "sha256:bench-csv",
                "tool_versions": { "hash": "0.1.0" }
            }),
        ),
        (
            "xlsx",
            json!({
                "version": "hash.v0",
                "path": fixture("tests/fixtures/files/sample.xlsx").display().to_string(),
                "extension": ".xlsx",
                "bytes_hash": "sha256:bench-xlsx",
                "tool_versions": { "hash": "0.1.0" }
            }),
        ),
        (
            "pdf_with_text",
            json!({
                "version": "hash.v0",
                "path": fixture("tests/fixtures/files/sample.pdf").display().to_string(),
                "text_path": fixture("tests/fixtures/files/sample.md").display().to_string(),
                "extension": ".pdf",
                "bytes_hash": "sha256:bench-pdf",
                "tool_versions": { "hash": "0.1.0" }
            }),
        ),
        (
            "markdown",
            json!({
                "version": "hash.v0",
                "path": fixture("tests/fixtures/files/sample.md").display().to_string(),
                "extension": ".md",
                "bytes_hash": "sha256:bench-md",
                "tool_versions": { "hash": "0.1.0" }
            }),
        ),
    ];

    for (name, record) in records {
        group.throughput(Throughput::Elements(1));
        group.bench_function(name, |b| {
            b.iter(|| {
                black_box(enrich_record_with_fingerprints(
                    black_box(&record),
                    black_box(&registry),
                    black_box(&fingerprint_ids),
                ))
            });
        });
    }

    group.finish();
}

fn bench_end_to_end_batch_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("end_to_end_batch");
    let mut registry = FingerprintRegistry::new();
    for builtin in register_builtins() {
        registry.register(builtin);
    }
    let fingerprint_ids = vec![
        "csv.v0".to_owned(),
        "xlsx.v0".to_owned(),
        "pdf.v0".to_owned(),
        "markdown.v0".to_owned(),
    ];
    let templates = [
        json!({
            "version": "hash.v0",
            "path": fixture("tests/fixtures/files/sample.csv").display().to_string(),
            "extension": ".csv",
            "bytes_hash": "sha256:bench-csv",
            "tool_versions": { "hash": "0.1.0" }
        }),
        json!({
            "version": "hash.v0",
            "path": fixture("tests/fixtures/files/sample.xlsx").display().to_string(),
            "extension": ".xlsx",
            "bytes_hash": "sha256:bench-xlsx",
            "tool_versions": { "hash": "0.1.0" }
        }),
        json!({
            "version": "hash.v0",
            "path": fixture("tests/fixtures/files/sample.pdf").display().to_string(),
            "text_path": fixture("tests/fixtures/files/sample.md").display().to_string(),
            "extension": ".pdf",
            "bytes_hash": "sha256:bench-pdf",
            "tool_versions": { "hash": "0.1.0" }
        }),
        json!({
            "version": "hash.v0",
            "path": fixture("tests/fixtures/files/sample.md").display().to_string(),
            "extension": ".md",
            "bytes_hash": "sha256:bench-md",
            "tool_versions": { "hash": "0.1.0" }
        }),
    ];

    for batch_size in [10usize, 50, 100] {
        let records = templates
            .iter()
            .cloned()
            .cycle()
            .take(batch_size)
            .collect::<Vec<_>>();

        group.throughput(Throughput::Elements(batch_size as u64));
        group.bench_with_input(
            BenchmarkId::new("records", batch_size),
            &batch_size,
            |b, _| {
                b.iter(|| {
                    for record in &records {
                        black_box(enrich_record_with_fingerprints(
                            black_box(record),
                            black_box(&registry),
                            black_box(&fingerprint_ids),
                        ));
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_single_record_processing,
    bench_batch_record_processing,
    bench_fingerprint_registry_scaling,
    bench_manifest_processing_patterns,
    bench_error_handling_overhead,
    bench_end_to_end_record_throughput,
    bench_end_to_end_batch_throughput
);
criterion_main!(benches);
