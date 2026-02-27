# Benchmark Baseline (bd-3az)

Date: 2026-02-27  
Host: local dev machine (`cargo bench`, criterion/plotters backend)  
Profile: `release` (`cargo bench`)  
Sampling config used for baseline capture:

```bash
criterion defaults (warmup=3s, sample_size=100)
```

## Commands

```bash
CARGO_TARGET_DIR=target_codex cargo bench --bench assertion_throughput --bench pipeline_throughput --bench normalization
```

## Key baselines

### `assertion_throughput`

- `assertions/sheet_exists`: `126.44–144.52 µs`
- `assertions/cell_eq`: `156.70–164.72 µs`
- `assertions/text_contains`: `214.50–262.46 ns`
- `assertions/text_regex`: `467.19–475.23 µs`
- `assertion_batches/batch_evaluation/10` throughput: `3.98–4.06 Kelem/s`

### `pipeline_throughput`

- `single_record/simple`: `4.55–4.61 µs`
- `single_record/complex`: `4.55–4.57 µs`
- `batch_processing/records/10` throughput: `218.12–220.05 Kelem/s`
- `batch_processing/records/100` throughput: `212.93–219.20 Kelem/s`
- `registry_scaling/fingerprints/50` throughput: `10.35–10.73 Melem/s`
- `manifest_patterns/small_files_many`: `90.65–91.32 µs`
- `error_handling/success_path`: `4.58–4.66 µs`
- `error_handling/failure_path`: `4.54–4.60 µs`

### `normalization`

- `markdown_normalization/simple`: `1.97–2.06 ms`
- `markdown_normalization/medium`: `1.84–1.89 ms`
- `markdown_normalization/complex`: `1.87–1.97 ms`
- `markdown_structure/table_heavy`: `927.61 µs–1.04 ms`
- `large_documents/normalization/50x` throughput: `8.52–8.61 MiB/s`
- `large_documents/structure_extraction/50x` throughput: `13.88–14.00 MiB/s`
- `parsing_approaches/regex_based`: `1.73–1.75 ms`
- `parsing_approaches/manual_parsing`: `204.76–205.64 µs`
