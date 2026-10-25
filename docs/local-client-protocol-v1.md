# Local Client Protocol v1

This public API is independent of Alfred and Rust. Launchers can implement the framed JSON transport or invoke the installed `kwel-bridge` JSON CLI. [JSON Schema](schemas/local-client-v1.schema.json), [Python example](../samples/client.py), and [Rust example](../crates/kwel-client/examples/search.rs) are provided.

## Supported operations

One connection carries one request and one response, then closes. Each frame is a four-byte little-endian byte count followed by UTF-8 JSON. Requests are limited to **16,384 bytes**, responses to **1,000,000 bytes**. Unknown and duplicate fields, trailing values, invalid versions and malformed IDs fail closed.

Every request has `v: 1`, `type`, `requestId` (1–100 UTF-16 units), and `profileId` (UUID). The profile must match the endpoint. Queries are ≤200 UTF-16 units. Never interpolate user input into a shell command.

| Request | Additional fields | Success response |
| --- | --- | --- |
| `profile-probe` | none | `profile-info`: `profileId`, `profileLabel`, `hostVersion` |
| `search` | `query`, `limit` (1–100) | `search-result`: ordered `items`, `hasMore` |
| `open-page` | `openRef` (UUID) | `open-result`: `ok: true` |

Responses include `v` and the same `requestId`. A failure has `type: "error"`, `code`, and a constant safe `message`. Native v1 error codes are retained; local failures also include `BUSY`, `UNSAFE_PATH`, `INSTALL_CONFLICT`, `DOWNGRADE_REFUSED`, and `HOST_IN_USE`. Never treat an error response as an actionable search result.

Example request (identifiers are supplied by the user/client, not copied fixtures):

```json
{"v":1,"type":"search","requestId":"request-1","profileId":"<Kwel Profile ID>","query":"Kyoto","limit":20}
```

The schema requires a real UUID in place of the placeholder. Each item contains `title`, `url`, `domain`, and an opaque `openRef`. Preserve item order. Open by submitting only `openRef`; arbitrary URLs are rejected. References expire after five minutes, are single use, and cannot move between profiles or connections.

## Discovering endpoints

| OS | Endpoint |
| --- | --- |
| macOS | `/private/tmp/kwel-connect-<uid>/<profileId>.sock` |
| Ubuntu | `$XDG_RUNTIME_DIR/kwel-connect/<profileId>.sock`; if XDG is unset, `/tmp/kwel-connect-<uid>/<profileId>.sock` |
| Windows | `\\.\pipe\kwel-<SID hash>-<runtime hash>-<profileId>` |

Unix parents must be owned by the current user with mode 0700; sockets must be owned by that user with mode 0600. Reject symlinks and unsafe ancestors, and verify peer UID. An explicitly configured but unsafe XDG directory is an error, not a fallback.

Windows runtime metadata is under `%LOCALAPPDATA%\Kwel\NativeBridge\runtime`, with a protected DACL granting only the current user's SID. The SID hash is the first 24 lowercase hex characters of SHA-256 over the SID's UTF-8 string. The runtime hash is the first 16 hex characters of SHA-256 over the runtime directory's canonical UTF-8 path. Profile UUIDs are lowercase. `.endpoint` files enumerate candidates; pipe names are derived, never taken from untrusted file contents. Reject remote pipes, verify the server's user SID, and use identification-only SQOS. Servers verify client SID and reject remote clients.

Probe candidate endpoints to check live connections. `kwel-bridge profiles --format json` performs bounded discovery for callers and returns `{"v":1,"profiles":[...]}`. Do not guess another profile if a configured ID is disconnected or conflicted.

## JSON CLI

```sh
kwel-bridge profiles --format json
kwel-bridge profile-probe --profile-id "$PROFILE_ID" --format json
kwel-bridge search --profile-id "$PROFILE_ID" --query "Kyoto" --limit 20 --format json
kwel-bridge open --profile-id "$PROFILE_ID" --ref "$OPEN_REF" --format json
```

macOS binaries are installed under `~/Library/Application Support/Kwel/NativeBridge/bin/`; Windows binaries under `%LOCALAPPDATA%\Kwel\NativeBridge\bin\`; Ubuntu installs `kwel-bridge` in `/usr/bin/`. The Python sample locates these paths without requiring Rust or PATH edits.

CLI stdout contains only the requested JSON response (or a version string for `--version`). This includes requested result data; it is not a diagnostic log. Failures return nonzero. Do not send search output to shared telemetry/logs.

## Resource and management boundaries

The Host admits at most 32 public connections and 32 pending requests. Request input has a three-second deadline; response writes have two seconds; Native searches/opens have 12 seconds. Control connections have separate limits of four connections, 1 KiB requests and one-second deadlines. Native input has a bounded four-message queue.

`uninstall`, `doctor`, `profile-claim`, and `profile-release` are **not public operations**. They are rejected on the public endpoint. Duplicate-session coordination uses a separate owner-only endpoint and a random capability that is never returned through the public API. Host installation/removal uses local filesystem/registry ownership checks, not public IPC commands.

Kwel officially supports the Host and Alfred Workflow. Third-party launcher behavior, permissions, data handling and quality remain the launcher's responsibility.
