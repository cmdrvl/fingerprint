# Release Runbook

This document provides a step-by-step checklist for releasing new versions of `fingerprint`.

## Overview

The `fingerprint` release process involves:
1. Pre-release validation and preparation
2. Version bumping and tagging
3. CI/CD pipeline execution and verification
4. Distribution channel updates
5. Post-release validation and monitoring

## Pre-Release Checklist

### Code Quality & Testing
- [ ] All tests pass locally: `cargo test --all-features`
- [ ] Integration tests pass: `cargo test --tests`
- [ ] Smoke tests pass with built binary
- [ ] Golden output determinism tests pass
- [ ] No failing or ignored tests without documented justification
- [ ] Code coverage meets minimum thresholds
- [ ] Clippy passes with no warnings: `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] Formatting is correct: `cargo fmt --all -- --check`

### Documentation & Compatibility
- [ ] CHANGELOG.md updated with release notes for this version
- [ ] README.md updated if CLI interface or major features changed
- [ ] All public APIs documented
- [ ] Breaking changes clearly documented with migration guide
- [ ] Compatibility matrix updated if platform support changed

### Dependencies & Security
- [ ] Dependencies audited: `cargo audit`
- [ ] No known security vulnerabilities in dependency tree
- [ ] Unused dependencies removed
- [ ] License compatibility verified for new dependencies
- [ ] SBOM (Software Bill of Materials) generated if required

### Platform Support
- [ ] Builds successfully on all target platforms:
  - [ ] `x86_64-unknown-linux-gnu`
  - [ ] `x86_64-apple-darwin`
  - [ ] `aarch64-apple-darwin`
  - [ ] `x86_64-pc-windows-msvc`
- [ ] Cross-compilation tests pass
- [ ] Platform-specific features work correctly

## Version Bump Process

### Determine Version Type
Follow [Semantic Versioning](https://semver.org/):
- **MAJOR** (`1.0.0 -> 2.0.0`): Breaking changes to public API
- **MINOR** (`1.0.0 -> 1.1.0`): New features, backward-compatible
- **PATCH** (`1.0.0 -> 1.0.1`): Bug fixes, backward-compatible

### Update Version Numbers
- [ ] Update version in `Cargo.toml`
- [ ] Update version in `Cargo.lock`: `cargo update -p fingerprint`
- [ ] Update version in installation scripts if referenced
- [ ] Update version in documentation if hardcoded

### Create Release Commit
```bash
# Update CHANGELOG.md with final release notes
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "release: v{VERSION}"
git tag -a v{VERSION} -m "Release v{VERSION}"
```

## CI Verification

### Pre-Push Validation
- [ ] Local build succeeds: `cargo build --release`
- [ ] Local test suite passes: `cargo test --release`
- [ ] Binary size is reasonable (check against previous releases)
- [ ] Built binary works with smoke test: `./target/release/fingerprint --version`

### Push and Monitor CI
```bash
git push origin main
git push origin v{VERSION}
```

Monitor CI pipeline:
- [ ] All platform builds succeed
- [ ] All test suites pass on CI
- [ ] Binary artifacts are generated correctly
- [ ] Release assets are uploaded to GitHub Releases
- [ ] Docker images built and pushed (if applicable)

### CI Pipeline Components
- [ ] **Build Matrix**: All platforms build successfully
- [ ] **Test Matrix**: All test suites pass on all platforms
- [ ] **Security Scan**: No vulnerabilities detected
- [ ] **Performance Regression**: No significant performance degradation
- [ ] **Documentation**: Docs build and deploy successfully

## Homebrew Tap Verification

### Automatic Tap Update
The Homebrew tap should update automatically via CI:
- [ ] Homebrew formula updated with new version and SHA256
- [ ] Formula syntax validated
- [ ] Test installation works: `brew install cmd-rvl/tap/fingerprint`
- [ ] Smoke test from Homebrew installation works

### Manual Verification (if needed)
```bash
# Test Homebrew installation
brew uninstall fingerprint  # if previously installed
brew install cmd-rvl/tap/fingerprint
fingerprint --version  # should show new version
fingerprint infer tests/fixtures/files --format xlsx --id test  # smoke test
```

### Homebrew Tap Troubleshooting
If automatic update fails:
- [ ] Check tap repository for failed automation
- [ ] Verify SHA256 checksums match released assets
- [ ] Test formula locally: `brew install --build-from-source`
- [ ] Manual PR to tap repository if automation is broken

## Distribution Channels

### GitHub Releases
- [ ] Release created with proper title and description
- [ ] Release notes include highlights and breaking changes
- [ ] Binary assets attached for all supported platforms
- [ ] Source code archive attached
- [ ] Checksums file provided for verification

### Cargo Registry (crates.io)
```bash
# Publish to crates.io
cargo publish --dry-run  # verify package contents
cargo publish            # actual publish
```
- [ ] Package published successfully to crates.io
- [ ] Documentation built and available on docs.rs
- [ ] Installation works: `cargo install fingerprint --version {VERSION}`

### Docker Registry (if applicable)
- [ ] Docker images built and tagged correctly
- [ ] Images pushed to registry
- [ ] Multi-architecture manifests created
- [ ] Image vulnerability scan passes

## Post-Release Validation

### Installation Testing
Test installation from each distribution channel:
- [ ] **Homebrew**: `brew install cmd-rvl/tap/fingerprint`
- [ ] **Cargo**: `cargo install fingerprint --version {VERSION}`
- [ ] **GitHub Releases**: Download and extract binary
- [ ] **Docker**: `docker run fingerprint:v{VERSION} --version`

### Functional Testing
- [ ] Basic CLI functionality works
- [ ] Core features work with real data
- [ ] Performance is as expected
- [ ] No obvious regressions from previous version

### Monitoring & Feedback
- [ ] Monitor GitHub Issues for new bug reports
- [ ] Monitor installation metrics if available
- [ ] Monitor performance metrics in production usage
- [ ] Review community feedback and usage patterns

## Rollback Plan

### Criteria for Rollback
Rollback if any of these occur within 24 hours of release:
- Critical security vulnerability discovered
- Data corruption or loss reported
- Widespread installation failures
- Critical functional regression affecting core workflows

### Rollback Process
1. **Immediate Actions**:
   - [ ] Create GitHub Issue documenting the problem
   - [ ] Notify team and stakeholders
   - [ ] Assess impact and decide on rollback vs. hotfix

2. **GitHub Releases**:
   - [ ] Mark problematic release as "pre-release"
   - [ ] Update release notes with warning
   - [ ] Promote previous stable release as "latest"

3. **Homebrew Tap**:
   - [ ] Revert Homebrew formula to previous version
   - [ ] Test rollback installation works

4. **Cargo Registry**:
   - [ ] Yank problematic version: `cargo yank --version {VERSION}`
   - [ ] Consider publishing hotfix version instead

5. **Communication**:
   - [ ] Update CHANGELOG.md with rollback notice
   - [ ] Post announcement in relevant channels
   - [ ] Document lessons learned

### Recovery Process
After rollback:
- [ ] Identify and fix root cause
- [ ] Add tests to prevent regression
- [ ] Plan hotfix or next release
- [ ] Update release process to prevent similar issues

## Emergency Procedures

### Security Vulnerability
If a security issue is discovered:
1. [ ] **Do not** discuss publicly until patched
2. [ ] Create private security advisory on GitHub
3. [ ] Develop and test fix privately
4. [ ] Coordinate disclosure timeline
5. [ ] Release security patch ASAP
6. [ ] Notify users and downstream dependents

### Critical Bug Hotfix
For critical bugs requiring immediate attention:
1. [ ] Create hotfix branch from release tag
2. [ ] Apply minimal fix
3. [ ] Test thoroughly but expedite process
4. [ ] Release as patch version (increment patch number)
5. [ ] Backport fix to main branch

## Release History

| Version | Date | Type | Notes |
|---------|------|------|-------|
| v0.1.0  | TBD  | Initial | Initial release |

## Tools & Resources

### Required Tools
- `cargo` - Rust toolchain
- `git` - Version control
- `gh` - GitHub CLI (for releases)
- `brew` - Homebrew (for testing)

### Useful Commands
```bash
# Check current version
grep '^version' Cargo.toml

# Generate checksums for release assets
shasum -a 256 fingerprint-*

# Test cross-compilation
cargo build --target x86_64-unknown-linux-gnu --release

# Check binary size
ls -lh target/release/fingerprint

# Verify binary works
./target/release/fingerprint --help
```

### References
- [Semantic Versioning](https://semver.org/)
- [Rust Release Best Practices](https://doc.rust-lang.org/cargo/reference/publishing.html)
- [GitHub Releases Documentation](https://docs.github.com/en/repositories/releasing-projects-on-github)
- [Homebrew Formula Development](https://docs.brew.sh/Formula-Cookbook)

---

**Note**: This runbook should be updated as the release process evolves. Test the process regularly and incorporate lessons learned from each release.