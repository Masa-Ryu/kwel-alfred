# Local Client Protocol v1

This document and the [Draft 2020-12 JSON Schema](schemas/local-client-v1.schema.json) specify the public **launcher ↔ Host** API. It is independent of Alfred and Rust. Implement framed JSON directly or invoke the installed `kwel-bridge` JSON CLI; [Python](../samples/client.py) and [Rust](../crates/kwel-client/examples/search.rs) examples are available.

This is separate from [Native Messaging](native-messaging-v1.md). Local requests always select a profile, search always includes a limit, and opening requires a UUID. `hello`, `ping`, `pong` and internal management operations do not belong on this endpoint. The Native65 corpus tests the Native protocol; it does not define the Local wire contract.

## Transport and version

One connection carries **one request and one response**, then closes. Each frame contains a four-byte little-endian unsigned byte count followed by one UTF-8 JSON object. Request payloads are 1–16,384 bytes; response payloads are 1–1,000,000 bytes, excluding the header. Search and open use different local connections to the same live Native session.

Every message requires `v: 1` and `type`. Objects are closed per message type. Reject unknown/duplicate fields, trailing JSON, invalid UTF-8, lone surrogates, malformed IDs and unsupported versions. Encode numeric fields as unsigned integer literals; the [Native Schema/runtime distinction](native-messaging-v1.md#schema-and-runtime-checks) also applies here. A different wire contract requires another protocol version; `hostVersion` is the installed Host's product version, not the protocol version.

Standard Schema validation is read-only. It does not apply `default`, enforce `x-utf16-maxLength`, select a direction, authenticate the peer or track a session. Validate requests through `#/$defs/request` and responses through `#/$defs/response` when using the Schema outside these Rust tests.

## Requests and responses

All requests require `requestId` (a nonempty string, at most 100 UTF-16 units) and `profileId` (hyphenated RFC-variant UUID, version 1–8). Profile IDs are accepted case-insensitively and normalized to lowercase for endpoint/profile selection; request IDs are opaque and case-sensitive. The requested profile must match that endpoint; a Profile ID is not an authentication token. Never guess another profile or interpolate user input into a shell command.

| Type / direction | Additional required fields | Success response |
| --- | --- | --- |
| `profile-probe` / Client → Host | none | `profile-info` |
| `search` / Client → Host | `query` (0–200 UTF-16 units), `limit` (integer 1–100) | `search-result` |
| `open-page` / Client → Host | `openRef` (UUID) | `open-result` |
| `profile-info` / Host → Client | `profileId`, `profileLabel`, `hostVersion` | n/a |
| `search-result` / Host → Client | `items`, `hasMore` | n/a |
| `open-result` / Host → Client | `ok: true` | n/a |
| `error` / Host → Client | `code`, `message` | n/a |

Every response also requires `requestId`. A response to a valid request must echo its ID and match the requested operation, or be an error. Clients reject wrong versions, unrelated IDs, mismatched profiles and unexpected response types. Concurrent clients may use equal local IDs: the Host assigns separate Native IDs and restores the caller's ID only after validation.

`profileLabel` is a string of at most 64 UTF-16 units without CR/LF, possibly empty. `hostVersion` is a nonempty string of at most 100 **UTF-8 bytes**; standard Schema `maxLength` alone cannot enforce that byte bound.

`query: ""` is valid and required. Unlike Native search, an omitted Local `limit` is invalid; the CLI supplies its own default 20 before creating the request. A result requires `items: []` when empty and a boolean `hasMore`. Each item requires `title`, `url`, `domain` and UUID `openRef`, with the [same result constraints](native-messaging-v1.md#fields-and-directions) as Native results. The response must fit the requested limit, at most 100 items. Preserve Kwel's order, titles and URLs.

Open only a returned UUID reference, preserving its exact string and case. Arbitrary URLs and opaque non-UUID strings are rejected by this public API even though Native uses opaque lookup tokens. References expire after five minutes, are single use, and cannot move between profiles or Native sessions. Consumption precedes the open attempt; retry by searching again. See [Native session behavior](native-messaging-v1.md#session-behavior-and-openref).

## Minimal examples

These UUIDs are synthetic examples, not user identifiers. Each JSON object is a separate framed payload. A probe request and its response:

```json
{"v":1,"type":"profile-probe","requestId":"p","profileId":"00000000-0000-4000-8000-000000000001"}
```
```json
{"v":1,"type":"profile-info","requestId":"p","profileId":"00000000-0000-4000-8000-000000000001","profileLabel":"","hostVersion":"1.0.0"}
```

A search request and an empty result:

```json
{"v":1,"type":"search","requestId":"s","profileId":"00000000-0000-4000-8000-000000000001","query":"Kyoto","limit":20}
```
```json
{"v":1,"type":"search-result","requestId":"s","items":[],"hasMore":false}
```

An open request and its success response:

```json
{"v":1,"type":"open-page","requestId":"o","profileId":"00000000-0000-4000-8000-000000000001","openRef":"00000000-0000-4000-8000-000000000002"}
```
```json
{"v":1,"type":"open-result","requestId":"o","ok":true}
```

An error response:

```json
{"v":1,"type":"error","requestId":"o","code":"RESULT_EXPIRED","message":"This Kwel result has expired. Search again."}
```

## Errors and request IDs

`code` uses the [Native error vocabulary](native-messaging-v1.md#errors-and-correlation), plus these Local-only values:

| Code | Meaning |
| --- | --- |
| `BUSY` | Connection, queue or resource limit reached |
| `UNSAFE_PATH` | Unsafe path, ownership or permissions |
| `INSTALL_CONFLICT` | Unrecognized Host registration |
| `DOWNGRADE_REFUSED` | A newer compatible Host is installed |
| `HOST_IN_USE` | Active Host cannot be removed |

Installation codes may also appear in JSON CLI failures; installation/removal is not an IPC operation. Error text is a constant safe explanation from the Host; branch on `code`. Never treat an error as an actionable result.

Local error `requestId` is always a **string**, never Native's `null`. If the request cannot be decoded, the Host replies with `requestId: ""`. If decoding succeeds but validation fails, the Host echoes the decoded ID, including an invalid empty or overlength ID. Consequently error IDs use a separate Schema shape from valid request/response IDs; the response-byte limit still applies. Such an error does not make the original request valid or satisfy an unrelated pending operation. Normal responses and responses to valid requests retain the nonempty 100-UTF-16-unit request-ID bound.

## Discovering endpoints and peer authorization

| OS | Endpoint |
| --- | --- |
| macOS | `/private/tmp/kwel-connect-<uid>/<profileId>.sock` |
| Ubuntu | `$XDG_RUNTIME_DIR/kwel-connect/<profileId>.sock`; if XDG is unset, `/tmp/kwel-connect-<uid>/<profileId>.sock` |
| Windows | `\\.\pipe\kwel-<SID hash>-<runtime hash>-<profileId>` |

Unix parents must be owned by the current user with mode 0700; sockets must be owned by that user with mode 0600. Reject symlinks and unsafe ancestors, and verify peer UID. An explicitly configured but unsafe XDG directory is an error, not a fallback.

Windows runtime metadata is under `%LOCALAPPDATA%\Kwel\NativeBridge\runtime`, with a protected DACL granting only the current user's SID. The SID hash is the first 24 lowercase hex characters of SHA-256 over the `ConvertSidToStringSidW` SID's UTF-8 string. The runtime hash is the first 16 hex characters of SHA-256 over the canonical UTF-8 path returned by `GetFinalPathNameByHandleW` with normalized DOS volume paths, including its `\\?\` prefix and returned casing. Profile UUIDs are lowercase. `.endpoint` files enumerate candidates; derive pipe names rather than reading untrusted names. Reject remote pipes, verify the server's user SID, and use identification-only SQOS. Servers read the first bounded frame, then verify client SID before processing it.

Probe candidate endpoints for live connections. `kwel-bridge profiles --format json` performs bounded discovery and returns `{"v":1,"profiles":[...]}`. This CLI discovery object is not an IPC response and is not described by the Local message Schema. A disconnected or conflicted configured profile is an error, not permission to search another profile.

## JSON CLI

```sh
kwel-bridge profiles --format json
kwel-bridge profile-probe --profile-id "$PROFILE_ID" --format json
kwel-bridge search --profile-id "$PROFILE_ID" --query "Kyoto" --limit 20 --format json
kwel-bridge open --profile-id "$PROFILE_ID" --ref "$OPEN_REF" --format json
```

macOS binaries are installed under `~/Library/Application Support/Kwel/NativeBridge/bin/`; Windows under `%LOCALAPPDATA%\Kwel\NativeBridge\bin\`; Ubuntu installs `kwel-bridge` in `/usr/bin/`. The Python sample locates these paths without requiring Rust or PATH edits.

CLI stdout contains only the requested JSON response (or a version string for `--version`). Requested result data is not a diagnostic log; do not send it to shared telemetry. Failures return nonzero.

## Lifetimes, resource and management boundaries

The Host admits at most 32 public connections and 32 pending Native requests. Request input has a three-second deadline; response writes have two seconds; Native searches/opens have 12 seconds. Probes serialize their two-second ping/pong wait. The installed Rust client bounds a probe exchange to four seconds and a search/open exchange to 15 seconds. Disconnect cancels pending work. Oversized/malformed input is rejected without forwarding it to Chrome; unsuccessful writes may close without a response.

`uninstall`, `doctor`, `profile-claim` and `profile-release` are **not public operations**. They are rejected on this endpoint. Duplicate-session coordination uses a separate owner-only Control endpoint and a random capability never exposed by the public API. Control has separate limits of four connections, 1 KiB requests and one-second deadlines. Host installation/removal uses filesystem/registry ownership checks, not public management commands.

The existing Rust/IPC tests cover correlation, profile isolation, parallel callers, openRef expiry/single use, limits and registration. Schema validation cannot establish these behaviors. Kwel officially supports the Host and Alfred Workflow; third-party launcher behavior, permissions, data handling and quality remain that launcher's responsibility.
