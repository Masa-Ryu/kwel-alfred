# Security model

## Trust boundary

Kwel Connect is a local, same-OS-user integration service. It uses no TCP listener, cloud relay, telemetry, or search index of its own. Search and page opening remain Kwel extension responsibilities. stdout is reserved for protocol/explicit CLI JSON; stderr uses fixed messages without queries, browsing URLs, profile IDs or tokens.

Chrome validates the Native Messaging manifest allowlist; the Host separately validates the exact caller origin. The published Kwel extension ID is pinned in `config/distribution.json` and compiled into the Host. No wildcard, automatic extension discovery/trust, or UI for widening that allowlist is provided. An unpacked extension may have a different ID; developers must make an explicitly scoped local build. The public release remains pinned to the published ID.

An extension ID is not a cryptographic defense against an attacker already controlling the same OS account or Chrome's developer configuration. Such an attacker can also read that user's browser data and processes. Other OS users, remote pipe clients, unrelated extension origins, malformed clients and accidental profile mixing are within the enforced boundary.

## IPC and profile isolation

Unix directories/sockets are 0700/0600, owned by the current UID, with unsafe ancestor and symlink rejection, peer UID checks, and inode-checked stale/replacement cleanup. Linux uses a verified private XDG runtime directory or a private child of root-owned sticky `/tmp`. macOS uses a short private runtime path.

Windows uses local Named Pipes with a protected current-user-only SID DACL, remote-client rejection, first-instance ownership, client SID checks, and server-process SID checks. Identification-only SQOS prevents the server obtaining a reusable impersonation token. Every unsafe Win32 block is confined to `crates/kwel-native-host/src/windows.rs`, documents lifetime/ownership assumptions, and reverts impersonation before returning to async code.

Profile IDs must match the selected endpoint. The Host scopes pending responses and openRefs to a session, bounds requests/connections, fails closed on duplicate profile ownership, and never chooses a different profile automatically. It consumes references before forwarding opens; Kwel also retains its own TTL and availability checks. Public clients cannot request arbitrary URL opens or management operations.

The internal coordination token is owner-only and absent from the public schema. It prevents ordinary API clients invoking management by sending undocumented operations. It does not claim isolation from a malicious process with full access to the same user account. Rejected duplicate leases have a bounded lifetime to recover safely from crashes.

## Installation and migration

macOS installs in the current user's `Library/Application Support/Kwel/NativeBridge`; Windows uses LocalAppData plus HKCU. Ubuntu debs install root-owned binaries and Google Chrome's documented system manifest location; each spawned Host still executes as Chrome's user and creates user-isolated IPC. Installers never enumerate or copy browsing databases or profile identities.

Registration is preflighted, recognized product/version metadata is checked, and binaries/manifests are replaced atomically in private directories. An owner-only pending hash journal allows interrupted upgrades to retry without an older installer rolling back newer files. A newer compatible Host is reused by an older Alfred client. Unrecognized registrations and changed executable hashes are preserved and cause an error.

macOS copy uses `ditto --rsrc --extattr --acl`; quarantine is not stripped. Tests retain source quarantine flags, extended attributes and resource forks. No installer disables Gatekeeper, SmartScreen, UAC, Defender or browser permission checks. Distributions are unsigned and unnotarized; OS approval/reputation limitations still apply.

Both `com.kwel.bridge` and the legacy `com.kwel.alfred` registration point to the shared Host. Old AlfredBridge files are not blindly deleted. A read-only v1 adapter can finish using an already-running legacy socket until Kwel reconnects; the Workflow's stable bundle ID preserves its Profile ID preference. Removing the Workflow never calls Host uninstall. Explicit Host uninstall refuses an active shared Host and removes only recognized files/registrations, retaining runtime/legacy/unrelated data.

## CI and release boundary

All PR execution uses the requested five GitHub-hosted OS/architecture runners, never persistent N100 runners. PR jobs have read-only repository permissions, no configured Secrets, and no persisted checkout credentials. Official Actions are pinned to verified commit SHAs. Cargo.lock, format, Clippy, protocol/security tests and RustSec vulnerability audit are mandatory; security failures are not skipped or ignored.

Only the Release publication job has write permission. Tags must match the SemVer distribution and name a commit reachable from main. Every platform validates its installer before release collection; publication refuses existing releases and uploads to an unpublished draft before making it public. A failed upload leaves an unpublished draft for review, and retries refuse overwrites.

Unit/integration fixtures and hosted installer checks do not establish real Kwel/Chrome/Alfred/Gatekeeper/SmartScreen behavior. Human end-to-end checks are reported separately in the migration report.
