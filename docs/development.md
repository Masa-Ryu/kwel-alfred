# Development

## Build and test

Use Linux or macOS with Go 1.22+, a C compiler for the Go race detector, Python 3, LLVM's `llvm-lipo`, and **resvg 0.45.1**. Linux also needs GNU coreutils for the metadata-preserving installation tests.

On Ubuntu 24.04, install `gcc libc6-dev python3 llvm-18`. The workflows install Go 1.27.3 and resvg 0.45.1 from official distributions and verify their pinned SHA256 hashes before extraction. On macOS, LLVM is available through Homebrew; resvg can be installed with `cargo install resvg --version 0.45.1 --locked`.

Set `LLVM_LIPO` if the command is versioned or outside PATH, for example `export LLVM_LIPO=llvm-lipo-18` on Ubuntu or `export LLVM_LIPO="$(brew --prefix llvm)/bin/llvm-lipo"` on macOS.

```sh
bash scripts/verify.sh
```

This runs `go vet ./...`, `go test -race ./...`, shell argument checks, the Darwin arm64/amd64 builds with `CGO_ENABLED=0`, Universal Binary creation, and package verification. To generate only the package:

```sh
bash scripts/package-workflow.sh
bash scripts/verify-package.sh
```

The output is `dist/Kwel.alfredworkflow`. Packaging uses the tracked SVG, fixed renderer version, sorted entries, fixed ZIP timestamps, and explicit file permissions.

## Security checks and platform limits

The Linux tests exercise the shared ownership, permissions, symlink rejection, private socket directory checks, Native Messaging origin validation, and profile isolation. The CLI remains macOS-only.

The Darwin filesystem implementation retains `ditto --rsrc --extattr --acl` to preserve quarantine and other macOS metadata. Linux uses GNU `cp --preserve=all`; a Linux test pass does not verify macOS quarantine, ACL/resource-fork preservation, Alfred, Chrome activation, or Spaces behavior. Those macOS-specific behaviors require separate macOS validation and are not covered by CI.

## CI and releases

Both workflows run directly on the **self-hosted Linux runner with labels `self-hosted, n100`**. The runner needs Git, curl, GCC, Python 3, tar, sha256sum, apt-get, and dpkg-deb. Go and resvg use isolated temporary directories. Missing LLVM 18 and GitHub CLI packages are downloaded from the runner's signed apt repositories and extracted into the same temporary directory without a system installation. Test CI runs for pull requests, pushes to main, and manual dispatch with read-only repository permissions and no configured Secrets.

After merging, create and push a tag matching `config/distribution.json` and `workflow/info.plist`; version `1.00` requires tag `v1.00`. `scripts/check-release-version.py` rejects a mismatch or a non-production distribution.

The repository allows only owner-maintained Actions. These workflows use shell steps with exact-commit checkout and checksum-pinned tools, so no external Actions or Action allowlist changes are required.

The Release job repeats validation and generates `Kwel.alfredworkflow` and `SHA256SUMS` before publication. Only the Release job has `contents: write`, and its GitHub token is exposed only to the final publication step. It verifies the checksum, refuses an existing Release, and uses `gh release create --verify-tag --draft`. The draft is published only after both assets upload successfully. A failed upload leaves an unpublished draft for manual review; retries never overwrite an existing Release.

**Release files are not signed with an Apple Developer ID and are not notarized.** CI does not perform signing or notarization.
