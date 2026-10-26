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
python3 -m unittest discover -s scripts/tests -v
```

Tests use synthetic IDs and isolated directories. `scripts/test-installer.py` additionally exercises actual package install/upgrade/removal only on disposable GitHub-hosted runners; it refuses ordinary local environments. Do not run installer tests against a real user home.

macOS builds need both Darwin Rust targets, Xcode's `lipo`, `pkgbuild`, `productbuild`, the pinned LLVM tools component, and **resvg 0.45.1** (`cargo install resvg --version 0.45.1 --locked`). Windows packaging needs NSIS 3.11. Ubuntu packaging uses `dpkg-deb`. Install the audit tool with `cargo install cargo-audit --version 0.22.2 --locked`. These are development tools; end users install prebuilt packages.

macOS builds all three executables for arm64/x86_64 and creates Universal binaries. Windows release builds explicitly target x64 MSVC with static CRT; package verification rejects references to separately installed Visual C++ runtime DLLs. Ubuntu debs declare their libc/libgcc dependencies. Alfred packages retain sorted ZIP entries, fixed timestamps, explicit Unix modes, the single tracked SVG source and required empty Profile ID configuration. Installer inputs are selected explicitly and never include prefs, caches, tokens or user data.

## Runner and distribution compatibility

Release producers are macOS 26 arm64, Windows Server 2025 x64, and Ubuntu 24.04 amd64/arm64. The macOS 26 Intel job also builds/tests both Universal slices but does not upload a release artifact. Rust 1.99.0, resvg 0.45.1, cargo-audit 0.22.2, NSIS 3.11 and the Action SHAs remain fixed.

Each Darwin cross-build explicitly sets `MACOSX_DEPLOYMENT_TARGET`: **10.12 for x86_64**, **11.0 for arm64**, retaining the pre-update binary settings. `lipo` checks both slices; `vtool` checks their actual load-command minimum, including the binaries inside pkg and Workflow. The product's documented macOS 13+ requirement follows current Chrome support; older binary deployment targets alone do not establish Kwel/Chrome compatibility on obsolete OS versions. macOS 15 arm64/Intel jobs install the original macOS 26 arm64 producer's Universal pkg and execute its Host, JSON CLI and Workflow search binary. They do not rebuild those packages. macOS 13/14 runtime and real Chrome/Alfred UI remain manual checks.

Linux release debs are built only on Ubuntu 24.04 and keep `Depends: libc6 (>= 2.39), libgcc-s1`. Verification extracts the actual deb payload/control archives and inspects Native Host, JSON CLI, preinst and prerm with readelf. Required numeric GLIBC symbol versions must not exceed 2.39; the known DT_RELR ABI marker is supported since 2.36. Unknown/private ABI requirements and undeclared shared-library dependencies fail verification.

Ubuntu 26.04 amd64/arm64 jobs run source tests/builds separately, then download the corresponding **24.04-produced deb**, verify its producer/hash/ELF metadata, and install, communicate, reinstall and remove that unchanged file. No Ubuntu 26.04 build is uploaded or used for release. `BUILDINFO.json` records only product version, OS/architecture and package hashes. Collection checks producer roles and all hashes before assembling the same five distribution names and SHA256SUMS; build provenance is not an extra Release asset.

Release producer artifacts are always transferred within the run to support these consumer tests, with one-day retention by default or two days for release/review runs. Compatibility consumers never upload release artifacts. The release collector's own runner can be Ubuntu 26.04 because it copies validated files without rebuilding Linux binaries.

actionlint 1.7.12's bundled runner catalog does not yet include the two [official Ubuntu 26.04 labels](https://docs.github.com/en/actions/reference/runners/github-hosted-runners). For that version, validate syntax with `actionlint -ignore '^label "ubuntu-26\.04(-arm)?" is unknown\.' .github/workflows/test.yml .github/workflows/release.yml`. Only those catalog false positives are excluded; actual hosted execution and installer/security checks remain required. No self-hosted label aliases are added.

## Versions and migration

The original `1.00` Workflow display version maps to SemVer **1.0.0**; new release tags are `v1.0.0` and later. No repository tags or Releases existed when this migration was inspected; no historical tag is renamed or deleted. `scripts/check-release-version.py` compares the tag, Cargo, distribution config and Workflow version. Release publication requires the tagged commit already be on main and refuses an existing Release.

The GitHub repository was renamed in place to `Masa-Ryu/kwel-connect`; existing history, Issue #2 and PR #1 remain. Canonical URLs use the new name. The Alfred bundle ID `com.kwel.alfred.search` is intentionally stable for preference/import compatibility; the Native Host has the new canonical name and a legacy alias.

Issue #2 originally requested a research-only RFC. The implementation request supersedes that scope. The original Go CI baseline passed static analysis and race tests before migration. Rust's protocol/security/registration/installer tests passed on all five requested hosted runners before Go removal. The retained fixtures compare 81 original Go validation/normalization cases, including UTF-16 limits, framing, URL authority/escaping and IPv6 zones. No Go toolchain is used by the final build or tests. Final CI evidence and remaining real-app checks are recorded in PR #1 and Issue #2.

## Shared Native v1 release gate

`cargo test --workspace --locked` includes the 65-case Native-only corpus and all 81 archived Go-reference cases with documented corrections. `scripts/check-native-fixture.py --kwel-root <checkout>` verifies the adopted corpus hash, the pinned Kwel commit and protocol source, and identical JSON bytes. The separate TypeScript CI job selects Node.js 24 from the hosted runner tool cache, requires that major version, and uses pnpm 10.17.1. It runs the reference suite without building Chrome or reading any personal profile. The existing three checkout/upload/download Action SHAs and repository allowlist remain unchanged; a missing Node.js 24 installation fails the check.

The reusable Test workflow is required by Release's `validate` job. Rust or TypeScript conformance failure therefore blocks `publish`; there is no allow-failure or skipped conformance path. Actual Chrome/Alfred interaction remains a separate manual validation step.
