# Test Coverage Parity Review (PRM-FP-0005)

Date: 2026-02-25
Repo: `fingerprint`
Baseline repo: `rvl`

## Scope

Audit `fingerprint` test-coverage posture against the `rvl` model:
- unit tests
- integration tests
- smoke tests
- golden tests
- witness command coverage
- CI enforcement for test execution

Then map every identified gap to executable beads.

## Evidence Snapshot

`rvl` baseline (current local state):
- Unit tests in `src/`: 213 (`rg "#[test]" src`)
- Integration tests in `tests/`: 111 tests across 20 files
- Fixture corpus files in `tests/fixtures/`: 120
- CI workflow includes fmt/clippy/test/build and binary smoke checks

`fingerprint` current state:
- Unit tests in `src/`: 0 (no `src/` yet)
- Integration tests in `tests/`: 0
- Fixture corpus files in `tests/fixtures/`: 0
- Workflows in `.github/workflows/`: 0

## Gap Matrix

| Area | `rvl` Baseline | `fingerprint` Gap | Execution Bead(s) |
|---|---|---|---|
| Unit tests (core internals) | 213 unit tests in `src/*` | No unit tests or runtime modules yet | `bd-1zd` |
| Integration tests (pipeline behavior) | 111 integration tests in `tests/*.rs` | No run-mode integration coverage | `bd-18y` |
| Smoke CLI behavior | CI smoke-checks built binary (`--version`, `--help`) | No CLI smoke suite or workflow hooks | `bd-kmi`, `bd-2p6` |
| Golden output determinism | Golden output tests + fixtures in `tests/output_golden.rs` | No golden-output determinism checks | `bd-2ka` |
| Witness command coverage | Dedicated witness query/last/schema tests | No witness command tests yet | `bd-kmi`, `bd-2ka` |
| Fixture corpus breadth | 120 fixture files across regression/corpus/witness | No fixture corpus for csv/xlsx/pdf and failure paths | `bd-9in` |
| CI enforcement of coverage suites | CI has explicit test job; release build smoke | No CI gates for unit/integration/smoke/golden suites | `bd-3j6`, `bd-2p6` |

## Bead Mapping Completeness

All identified gaps now map to executable beads:
- `bd-1zd` unit assertion/refusal tests
- `bd-18y` run-mode integration coverage
- `bd-kmi` CLI + witness smoke coverage
- `bd-2ka` golden determinism coverage
- `bd-9in` fixture corpus build-out
- `bd-3j6` CI coverage enforcement
- `bd-2p6` CI workflow baseline parity

No uncovered test-coverage gap remains after this mapping pass.

## Recommended Execution Order

1. `bd-9in` fixture corpus first (unblocks realistic tests)
2. `bd-1zd` + `bd-18y` in parallel once fixtures land
3. `bd-kmi` + `bd-2ka` next
4. `bd-3j6` after core suites are present and stable

## Definition of Done for PRM-FP-0005

- A written parity matrix exists with concrete baseline evidence.
- Every missing area is linked to at least one executable bead.
- Downstream test implementation beads are dependency-linked.
