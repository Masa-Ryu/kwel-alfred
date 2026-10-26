# Development

## Workspace

| Crate | Responsibility |
| --- | --- |
| `kwel-protocol` | Native v1 and local v1 types, validation and framing |
| `kwel-native-host` | bounded async sessions, secure IPC, OS registration and installers |
| `kwel-client` | language-neutral JSON CLI and public protocol client |
| `kwel-alfred` | official Alfred JSON, connection retry and Chrome activation |

Use the Rust toolchain pinned by `rust-toolchain.toml`. Cargo.lock is committed. Tokio is used for bounded asynchronous Named Pipe/Unix socket I/O, cancellation and timeouts; no unbounded per-connection thread model is used. Windows FFI is isolated.

```sh
bash scripts/verify.sh
python3 -m unittest discover -s scripts/tests -v
cargo build -p kwel-protocol --example native-conformance --locked
node scripts/test-kwel-parser.cjs /path/to/current/kwel-checkout --current-kwel
node --test scripts/tests/chrome-e2e.test.mjs
```

Tests use synthetic IDs and isolated directories. `scripts/test-installer.py` additionally exercises actual package install/upgrade/removal only on disposable GitHub-hosted runners; it refuses ordinary local environments. Do not run installer tests against a real user home.

The current Kwel parser comparison runs in Kwel's private CI against the public Connect commit pinned in Kwel's `tests/fixtures/local-connection/native-messaging-v1.source.json`. Both checkouts are immutable for the run; fixture, specification and schema hashes are checked before executing the real TypeScript/Zod parser/builders and actual Rust frames. This avoids giving public PRs credentials to private source. The adapter checks all 65 shared cases plus 12 Unicode boundaries, including lone-surrogate rejection. No test changes the search engine or fixture expectations. For a local review of a newer Connect commit against the same corpus, add `--review-current`: it verifies Kwel's recorded source hashes from its immutable ancestor commit and runs the current Rust adapter. CI rejects this review mode and still requires its exact pair pin. Update the Kwel pin explicitly before coordinated release; editing only Connect does not update that private CI.

For real Chrome-launched Host, two-profile history/ranking/openRef and OS approval checks, use [the E2E verifier and manual sign-off procedure](chrome-e2e.md). Node/CDP guard tests, parser tests and the installed synthetic-peer test are not real Chrome/Alfred E2E and must never be reported as such.

macOS builds need both Darwin Rust targets, Xcode's `lipo`, `pkgbuild`, `productbuild`, the pinned LLVM tools component, and **resvg 0.45.1** (`cargo install resvg --version 0.45.1 --locked`). Windows packaging needs NSIS 3.11. Ubuntu packaging uses `dpkg-deb`. Install the audit tool with `cargo install cargo-audit --version 0.22.2 --locked`. These are development tools; end users install prebuilt packages.

macOS builds all three executables for arm64/x86_64 and creates Universal binaries. Windows release builds explicitly target x64 MSVC with static CRT; package verification rejects references to separately installed Visual C++ runtime DLLs. Ubuntu debs declare their libc/libgcc dependencies. Alfred packages retain sorted ZIP entries, fixed timestamps, explicit Unix modes, the single tracked SVG source and required empty Profile ID configuration. Installer inputs are selected explicitly and never include prefs, caches, tokens or user data.

## Runner and distribution compatibility

Release producers are macOS 26 arm64, Windows Server 2025 x64, and Ubuntu 24.04 amd64/arm64. The macOS 26 Intel job also builds/tests both Universal slices but does not upload a release artifact. Rust 1.99.0, resvg 0.45.1, cargo-audit 0.22.2, NSIS 3.11 and the Action SHAs remain fixed.

Each Darwin cross-build explicitly sets `MACOSX_DEPLOYMENT_TARGET`: **10.12 for x86_64**, **11.0 for arm64**, retaining the pre-update binary settings. `lipo` checks both slices; `vtool` checks their actual load-command minimum, including the binaries inside pkg and Workflow. The product's documented macOS 13+ requirement follows current Chrome support; older binary deployment targets alone do not establish Kwel/Chrome compatibility on obsolete OS versions. macOS 15 arm64/Intel jobs install the original macOS 26 arm64 producer's Universal pkg and execute its Host, JSON CLI and Workflow search binary. They do not rebuild those packages. macOS 13/14 runtime and real Chrome/Alfred UI remain manual checks.

Linux release debs are built only on Ubuntu 24.04 and keep `Depends: libc6 (>= 2.39), libgcc-s1`. Verification extracts the actual deb payload/control archives and inspects Native Host, JSON CLI, preinst and prerm with readelf. Required numeric GLIBC symbol versions must not exceed 2.39; the known DT_RELR ABI marker is supported since 2.36. Unknown/private ABI requirements and undeclared shared-library dependencies fail verification.

Ubuntu 26.04 amd64/arm64 jobs run source tests/builds separately, then download the corresponding **24.04-produced deb**, verify its producer/hash/ELF metadata, and install, communicate, reinstall and remove that unchanged file. No Ubuntu 26.04 build is uploaded or used for release. `BUILDINFO.json` records only product version, OS/architecture and package hashes. Collection checks producer roles and all hashes before assembling the same five distribution names and SHA256SUMS; build provenance is not an extra Release asset.

Release producer artifacts are always transferred within the run to support these consumer tests, with one-day retention by default or two days for release/review runs. Compatibility consumers never upload release artifacts. The release collector's own runner can be Ubuntu 26.04 because it copies validated files without rebuilding Linux binaries.

actionlint 1.7.12's bundled runner catalog does not yet include the two [official Ubuntu 26.04 labels](https://docs.github.com/en/actions/reference/runners/github-hosted-runners). For that version, validate syntax with `actionlint -ignore '^label "ubuntu-26\.04(-arm)?" is unknown\.' .github/workflows/test.yml .github/workflows/release.yml`. Only those catalog false positives are excluded; actual hosted execution and installer/security checks remain required. No self-hosted label aliases are added.

## First release and development installations

Kwel Connect 1.0.0, implemented in Rust, is the first formal release. Before removing development-only Go
compatibility, the GitHub API was checked on 2026-10-26: no tags, Releases,
Release assets or Release workflow runs existed. The old README described local
builds only. This establishes the repository's visible distribution history,
not the absence of private developer copies.

The display version `1.00` maps to SemVer `1.0.0`; release tags are `v1.0.0`
and later. No historical tags are renamed or deleted. Release preflight requires
a main commit and matching Cargo/config/Workflow version; publication refuses
existing Releases. The repository was renamed in place, retaining history,
Issue #2 and PR #1. `com.kwel.alfred.search` remains the Alfred bundle ID.

Developers with an old local Host must follow the [one-time manual replacement
procedure](development-host-replacement.md). The product registers only
`com.kwel.bridge`; there is no old-Host socket discovery, fallback, coordination
or registration rewrite.

## Manual release

Only the repository owner's explicit **Actions → Release → Run workflow**
starts Release. Merging a PR, pushing main, or pushing a tag never starts it.
The Test workflow keeps its existing automatic triggers. The Release workflow
must first be merged into the default branch to appear in the Actions UI.

1. Prepare and review main with matching Cargo, distribution configuration and
   Alfred Workflow versions. Check `python3 scripts/check-release-version.py v1.0.0`
   from that checkout, substituting the intended version.
2. Create and push the matching tag yourself at the current main HEAD. For example,
   after fetching main, inspect `git rev-parse origin/main`, then run
   `git tag v1.0.0 <that-full-SHA>` and `git push origin refs/tags/v1.0.0`.
   If the correct tag already exists, reuse it. If it points elsewhere, stop;
   do not force, move or overwrite it. Prepare a new version/tag when needed.
3. Open **Actions → Release → Run workflow**, select branch **main**, enter
   the existing tag (for example `v1.0.0`), and press **Run workflow**. This is
   the publication request; there is no second approval screen. After all
   validations succeed, the workflow publishes the five installers/Workflow
   files and `SHA256SUMS` automatically.

Preflight requires `workflow_dispatch`, main, an existing commit tag exactly
matching the dispatch's `github.sha`, matching SemVer/product versions, main
ancestry and no existing Release (including drafts). API/authentication/network
errors fail closed; only HTTP404 means no Release. Lightweight and annotated
tags are supported. No script creates or moves tags.

The dispatch SHA is main's HEAD **at dispatch time**. Every checkout and the
reusable Test workflow use that immutable snapshot. Main can advance afterwards
without changing the selected artifacts, as long as the snapshot remains in its
history. Publication fetches fresh refs and repeats preflight so a moved tag or
newly created Release is rejected before upload. It uses only this run's
validated artifacts, verifies checksums and stages a draft before making it
public. No Linux package is rebuilt by publish; only publish has write permission.

If validation fails, no publication job runs. An upload/publication failure can
leave an unpublished draft for the owner to inspect; retries reject that existing
Release rather than overwriting it. Inspect or resolve it manually before retrying.
Actual Release publication is not exercised by repository tests: they use
isolated Git fixtures, mocked read-only GitHub responses and workflow policy checks.

## Shared Native v1 release gate

`cargo test --workspace --locked` includes the canonical Native65, current validation boundaries, secure IPC/session and installation regressions. `python3 scripts/check-native-fixture.py` checks the canonical hash and immutable adoption metadata. Its optional `--kwel-root` verifies the original adoption checkout, not the latest parser. Kwel's `scripts/sync-native-fixture.mjs --check --upstream` checks its copy and source pin without modifying expectations; `--write --ref <full-public-Connect-SHA>` intentionally synchronizes all three canonical files and records hashes.

The public Connect Test workflow's canonical-corpus job and all OS Rust tests are required by Release's reusable `validate`. Kwel's required hosted **Native v1 pair** job tests its current PR parser against its pinned Connect source, builds the real Host/CLI and production extension, then runs Google Chrome search/open in two disposable profiles. Failed, cancelled, skipped or missing pair results block Kwel's `Verify` and automatic Release. The original application suites on N100 and full local pre-push gate are unchanged. Actions remain SHA-pinned, with read-only PR permissions and no pair-test secrets or artifact uploads.

Before a formal Connect release, also review the matching successful private pair run and perform the OS/GUI checks in [the E2E procedure](chrome-e2e.md). Connect cannot query a private Kwel run using its public PR token; its green CI alone does not attest that private run or approve a release. No release is created by these protocol changes.

Unix installer security tests execute each fixture in a dedicated subprocess so another test's fork cannot briefly inherit its live file lock. Every original assertion still runs. Production locking and concurrent IPC tests remain unchanged; no retry, sleep or skip hides a lock failure.

The shared corpus is checked out with LF on every OS via `.gitattributes`; Windows `core.autocrlf` must not change the bytes whose SHA-256 is recorded. Reference paths reject both POSIX and Windows roots, drive-relative paths and parent traversal on every platform.

## Fixture commit references

| Metadata | Meaning |
| --- | --- |
| Kwel's `tests/fixtures/local-connection/native-messaging-v1.source.json` | Pins the Connect commit tested by Kwel CI. Adoption of another commit is explicit. |
| Connect's `crates/kwel-protocol/tests/fixtures/native-messaging-v1.source.json` | Records the Kwel commit from which the shared fixture was first adopted. It is historical provenance, not a moving latest-HEAD pin. |

Changing the Release trigger updates neither Kwel's pin nor the adoption-origin
SHA. The Release tag/dispatch SHA identifies a separate release snapshot. None of
these Git references is a runtime Protocol field or a runtime dependency.
