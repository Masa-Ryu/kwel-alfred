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

macOS builds need both Darwin Rust targets, Xcode's `lipo`, `pkgbuild`, `productbuild`, and **resvg 0.45.1** (`cargo install resvg --version 0.45.1 --locked`). Windows packaging needs NSIS. Ubuntu packaging uses `dpkg-deb`. These are development tools; end users install prebuilt packages.

macOS builds all three executables for arm64/x86_64 and creates Universal binaries. Alfred packages retain sorted ZIP entries, fixed timestamps, explicit Unix modes, the single tracked SVG source and required empty Profile ID configuration. Installer inputs are selected explicitly and never include prefs, caches, tokens or user data.

## Versions and migration

The original `1.00` display version maps to SemVer **1.0.0**; new release tags are `v1.0.0` and later. `v1.00` is a historical spelling, not a new tag format. Do not create a second release for an existing distribution. `scripts/check-release-version.py` compares the tag, Cargo, distribution config and Workflow version. Release publication requires the tagged commit already be on main.

The GitHub repository was renamed in place to `Masa-Ryu/kwel-connect`; existing history, Issue #2 and PR #1 remain. Canonical URLs use the new name. The Alfred bundle ID `com.kwel.alfred.search` is intentionally stable for preference/import compatibility; the Native Host has the new canonical name and a legacy alias.

Issue #2 originally requested a research-only RFC. The implementation request supersedes that scope. The Go test baseline and generated v1 fixtures are retained as evidence before Go removal. Migration status, actual CI and remaining real-app checks are recorded in the implementation report.
