# GitHub publication and release builds

Goal: publish the existing mde source under jarymor-ux/mde, with macOS/Linux
instructions and downloadable builds after pushes to main (also accept master).

Design: keep the Rust application unchanged. Use native GitHub-hosted runners
for macOS ARM64, macOS x86_64, and Linux x86_64, with the committed Cargo.lock.
Run formatting, Clippy, tests, and optimized builds before publishing archives.
A separate job with contents:write creates an immutable prerelease per push;
build jobs have read-only repository access. Pull requests build without releasing.
Use main as the initial branch. Default repository visibility is private unless
the user requests public. Exclude target and local agent/runtime state.

Alternatives: artifacts alone expire and are harder to download; moving a single
release tag loses history. Immutable GitHub prereleases preserve each build.
macOS packages are ad-hoc signed, not notarized; Linux builds use Ubuntu 24.04.

## Tasks

- [x] Document source builds, binary installation, dependencies, and release behavior
      in README.md; add the MIT license already declared by Cargo.toml.
- [x] Add .github/workflows/release.yml: main/master push, PR, manual dispatch;
      three platform builds, archive/checksums, gated release job.
- [x] Validate existing application (fmt, Clippy, tests, release build), shell syntax,
      workflow schema with actionlint, and review publication contents.
- [ ] Initialize Git main, create the authorized GitHub repository, push, inspect
      the first Actions run, fix actual failures, and verify release assets.

Verification focus: runner CPU architecture, Linux native dependencies, locked Rust
MSRV, executable permissions and .app metadata, no publication on pull requests,
and no local state or build products in source control. No runtime behavior changes
are planned, so existing application tests and actual pipeline execution are the
relevant regression checks; do not add implementation-mirroring tests.

Local evidence: fmt, Clippy, 58 tests and release build pass; actionlint 1.7.12
passes; macOS package signature verification and Info.plist validation pass.
