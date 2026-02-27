use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use fingerprint::document::{CsvDocument, Document, TextDocument, XlsxDocument};
use fingerprint::dsl::assertions::{Assertion, evaluate_assertion};
use std::path::{Path, PathBuf};

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn text_document(content: &str) -> Document {
    Document::Text(TextDocument {
        path: PathBuf::from("benchmark.txt"),
        content: content.to_owned(),
        lines: content.lines().map(str::to_owned).collect(),
    })
}

fn bench_assertion_throughput(c: &mut Criterion) {
    let xlsx_doc = Document::Xlsx(XlsxDocument {
        path: fixture("tests/fixtures/files/sample.xlsx"),
    });
    let csv_doc = Document::Csv(CsvDocument {
        path: fixture("tests/fixtures/files/sample.csv"),
    });
    let text_doc = text_document("Invoice: INV-2023-001\nAmount: $1,234.56\nDue Date: 2023-12-31");

    let mut group = c.benchmark_group("assertions");

    group.bench_function("sheet_exists", |b| {
        let assertion = Assertion::SheetExists("Assumptions".to_owned());
        b.iter(|| {
            black_box(evaluate_assertion(
                black_box(&xlsx_doc),
                black_box(&assertion),
            ))
        });
    });

    group.bench_function("cell_eq", |b| {
        let assertion = Assertion::CellEq {
            sheet: "Assumptions".to_owned(),
            cell: "A1".to_owned(),
            value: "Tenant".to_owned(),
        };
        b.iter(|| {
            black_box(evaluate_assertion(
                black_box(&xlsx_doc),
                black_box(&assertion),
            ))
        });
    });

    group.bench_function("range_non_null", |b| {
        let assertion = Assertion::RangeNonNull {
            sheet: "Assumptions".to_owned(),
            range: "A1:B3".to_owned(),
        };
        b.iter(|| {
            black_box(evaluate_assertion(
                black_box(&xlsx_doc),
                black_box(&assertion),
            ))
        });
    });

    group.bench_function("csv_sheet_exists", |b| {
        let assertion = Assertion::SheetExists("Sheet1".to_owned());
        b.iter(|| {
            black_box(evaluate_assertion(
                black_box(&csv_doc),
                black_box(&assertion),
            ))
        });
    });

    group.bench_function("text_contains", |b| {
        let assertion = Assertion::TextContains("invoice".to_owned());
        b.iter(|| {
            black_box(evaluate_assertion(
                black_box(&text_doc),
                black_box(&assertion),
            ))
        });
    });

    group.bench_function("text_regex", |b| {
        let assertion = Assertion::TextRegex {
            pattern: r"INV-\d{4}-\d{3}".to_owned(),
        };
        b.iter(|| {
            black_box(evaluate_assertion(
                black_box(&text_doc),
                black_box(&assertion),
            ))
        });
    });

    group.bench_function("text_near", |b| {
        let assertion = Assertion::TextNear {
            anchor: "Invoice".to_owned(),
            pattern: "Amount".to_owned(),
            within_chars: 100,
        };
        b.iter(|| {
            black_box(evaluate_assertion(
                black_box(&text_doc),
                black_box(&assertion),
            ))
        });
    });

    group.finish();
}

fn bench_assertion_batch_throughput(c: &mut Criterion) {
    let xlsx_doc = Document::Xlsx(XlsxDocument {
        path: fixture("tests/fixtures/files/sample.xlsx"),
    });
    let assertions = [
        Assertion::FilenameRegex {
            pattern: r".*\.xlsx$".to_owned(),
        },
        Assertion::SheetExists("Assumptions".to_owned()),
        Assertion::SheetMinRows {
            sheet: "Assumptions".to_owned(),
            min_rows: 1,
        },
        Assertion::CellEq {
            sheet: "Assumptions".to_owned(),
            cell: "A1".to_owned(),
            value: "Tenant".to_owned(),
        },
        Assertion::RangeNonNull {
            sheet: "Assumptions".to_owned(),
            range: "A1:B2".to_owned(),
        },
    ];

    let mut group = c.benchmark_group("assertion_batches");
    for batch_size in [1usize, 5, 10, 20] {
        let batch_assertions = assertions
            .iter()
            .cycle()
            .take(batch_size)
            .collect::<Vec<_>>();
        group.throughput(Throughput::Elements(batch_size as u64));
        group.bench_with_input(
            BenchmarkId::new("batch_evaluation", batch_size),
            &batch_size,
            |b, _| {
                b.iter(|| {
                    for assertion in &batch_assertions {
                        black_box(
                            evaluate_assertion(black_box(&xlsx_doc), black_box(assertion)).unwrap(),
                        );
                    }
                });
            },
        );
    }
    group.finish();
}

fn bench_large_corpus_assertions(c: &mut Criterion) {
    let small_text = "Short text content";
    let medium_text = "Medium length text content ".repeat(10);
    let large_text = "Large text content with lots of repeated patterns ".repeat(100);
    let mut group = c.benchmark_group("corpus_size_scaling");

    for (size_name, text) in [
        ("small", small_text),
        ("medium", medium_text.as_str()),
        ("large", large_text.as_str()),
    ] {
        let text_doc = text_document(text);
        group.throughput(Throughput::Bytes(text.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("text_contains", size_name),
            &size_name,
            |b, _| {
                let assertion = Assertion::TextContains("content".to_owned());
                b.iter(|| {
                    black_box(evaluate_assertion(
                        black_box(&text_doc),
                        black_box(&assertion),
                    ))
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("text_regex", size_name),
            &size_name,
            |b, _| {
                let assertion = Assertion::TextRegex {
                    pattern: r"text.*content".to_owned(),
                };
                b.iter(|| {
                    black_box(evaluate_assertion(
                        black_box(&text_doc),
                        black_box(&assertion),
                    ))
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_assertion_throughput,
    bench_assertion_batch_throughput,
    bench_large_corpus_assertions
);
criterion_main!(benches);
