# Development

## Workspace

| Crate | Responsibility |
| --- | --- |
| `kwel-protocol` | Native v1 and local v1 types, validation and framing |
| `kwel-native-host` | bounded async sessions, secure IPC, OS registration and installers |
| `kwel-client` | language-neutral JSON CLI and public protocol client |
| `kwel-alfred` | official Alfred JSON, connection retry and Chrome activation |

Use the Rust toolchain pinned by `rust-toolchain.toml`. Cargo.lock is committed. Tokio is used for bounded asynchronous Named Pipe/Unix socket I/O, cancellation and timeouts; no unbounded per-connection thread model is used. Windows FFI is isolated. Product executables have no Go dependency.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo audit --deny warnings
python3 scripts/build.py package
python3 scripts/build.py verify
```

Tests use synthetic IDs and isolated directories. `scripts/test-installer.py` additionally exercises actual package install/upgrade/removal only on disposable GitHub-hosted runners; it refuses ordinary local environments. Do not run installer tests against a real user home.

macOS builds need both Darwin Rust targets, Xcode's `lipo`, `pkgbuild`, `productbuild`, the pinned LLVM tools component, and **resvg 0.45.1** (`cargo install resvg --version 0.45.1 --locked`). Windows packaging needs NSIS 3.11. Ubuntu packaging uses `dpkg-deb`. Install the audit tool with `cargo install cargo-audit --version 0.22.2 --locked`. These are development tools; end users install prebuilt packages.

macOS builds all three executables for arm64/x86_64 and creates Universal binaries. Alfred packages retain sorted ZIP entries, fixed timestamps, explicit Unix modes, the single tracked SVG source and required empty Profile ID configuration. Installer inputs are selected explicitly and never include prefs, caches, tokens or user data.

## Versions and migration

The original `1.00` Workflow display version maps to SemVer **1.0.0**; new release tags are `v1.0.0` and later. No repository tags or Releases existed when this migration was inspected; no historical tag is renamed or deleted. `scripts/check-release-version.py` compares the tag, Cargo, distribution config and Workflow version. Release publication requires the tagged commit already be on main and refuses an existing Release.

The GitHub repository was renamed in place to `Masa-Ryu/kwel-connect`; existing history, Issue #2 and PR #1 remain. Canonical URLs use the new name. The Alfred bundle ID `com.kwel.alfred.search` is intentionally stable for preference/import compatibility; the Native Host has the new canonical name and a legacy alias.

Issue #2 originally requested a research-only RFC. The implementation request supersedes that scope. The original Go CI baseline passed static analysis and race tests before migration. Rust's protocol/security/registration/installer tests passed on all five requested hosted runners before Go removal. The retained fixtures compare 81 original Go validation/normalization cases, including UTF-16 limits, framing, URL authority/escaping and IPv6 zones. No Go toolchain is used by the final build or tests. Final CI evidence and remaining real-app checks are recorded in PR #1 and Issue #2.
