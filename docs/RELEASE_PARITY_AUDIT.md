# Release Infra Parity Audit vs `rvl` (PRM-FP-0001 / `bd-2fj`)

## Scope

This audit covers repository automation parity only (CI/release/tap pipeline), not runtime behavior.

## Side-by-side parity matrix

| Area | `rvl` baseline expectation | `fingerprint` current state | Classification | Gap bead(s) / rationale |
|---|---|---|---|---|
| `ci.yml` | Gated CI (fmt, clippy, tests, smoke) on PR/push | `.github/workflows/ci.yml` runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, and tag smoke build checks (`--version`, `--help`) | **Keep** | No open parity gap; behavior is aligned and should be retained |
| `release.yml` | Tag-driven release build matrix + publish release assets | `.github/workflows/release.yml` derives version from `Cargo.toml`, verifies expected `v<version>` tag exists, builds target matrix archives, and publishes artifacts to GitHub Releases | **Adapt** | Implemented in `bd-w5b`; keep matrix/publish flow, adapt as signing/provenance steps land |
| Checksums / signing | Release artifacts include checksums and signed attestable artifacts | Not yet emitted in current workflow | **Defer** | `bd-poy` (Release attestations and signing parity) explicitly adds SHA256SUMS + signing artifacts |
| SBOM / provenance | Supply-chain metadata artifacts shipped with release | Not yet emitted in current workflow | **Defer** | `bd-poy` explicitly adds CycloneDX SBOM + provenance artifacts |
| Homebrew update path | Release automation updates tap formula from published artifacts + checksums | No formula/tap automation wired yet | **Defer** | `bd-3o0` (Homebrew release path parity) is dedicated to this gap after `bd-poy` |

## Gap coverage check

Open release-infra parity gaps are fully mapped to explicit beads:

- Checksums/signing → `bd-poy`
- SBOM/provenance → `bd-poy`
- Homebrew update path → `bd-3o0`

No unmatched release-infra parity gaps remain from this audit scope.
