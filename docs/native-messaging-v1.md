# Native Messaging Protocol v1

Canonical Host: `com.kwel.bridge`. The compatibility registration `com.kwel.alfred` points to the same Rust executable for older extensions. Enable **Local Connection** in current Kwel Settings. Native Messaging and the [Local Client Protocol](local-client-protocol-v1.md) have separate validators and wire shapes.

The executable accepts the Chrome-supplied `chrome-extension://<extension-id>/` origin as its first argument. Both manifests and Host origin validation admit exactly the configured Kwel extension ID. A developer-loaded extension with a different ID requires a separate local build; IDs are never discovered and trusted automatically.

## Framing and limits

Native framing is a four-byte **little-endian** unsigned byte length followed by one UTF-8 JSON object. The Host owns framing; the extension's `connectNative()` Port exchanges JavaScript objects. Payload length is 1–1,000,000 bytes in either direction. Empty, oversized, truncated, invalid UTF-8, duplicate-field, unknown-field and trailing-JSON inputs are rejected. `stdout` contains framed messages only; diagnostics do not include queries, results, labels or profile IDs.

All messages have numeric `v: 1` and `type`. The [JSON Schema](schemas/native-messaging-v1.schema.json) uses a separate closed object for each message type. A field valid for one operation is not implicitly accepted by another. Profile IDs and client routing fields never appear in Native `search` or `open-page` messages.

String bounds count **UTF-16 code units**: query ≤200, request ID 1–100, profile label ≤64 without CR/LF. Astral characters count as two units. An empty query is valid. JSON Schema's ordinary `maxLength` counts code points, so implementations must also enforce the normative `x-utf16-maxLength` annotation. Rust uses `encode_utf16().count()`; Kwel uses JavaScript `.length`.

## Messages and direction

| Direction | Type | Fields |
| --- | --- | --- |
| Extension → Host | `hello` | required `profileId` UUID, optional `profileLabel` (default `""`) |
| Host → Extension | `hello-ack` | no other fields |
| Host → Extension | `ping` | no other fields |
| Extension → Host | `pong` | no other fields |
| Host → Extension | `search` | required `requestId` and `query`; optional `limit` (1–100, default **20**) |
| Extension → Host | `search-result` | matching `requestId`, required `items` array and `hasMore` boolean |
| Host → Extension | `open-page` | required `requestId`, `openRef`; no profile ID or URL |
| Extension → Host | `open-result` | matching `requestId`, `ok: true` |
| Extension → Host | `error` | nullable `requestId`, required `code`, optional `message` (default `""`) |

Search always serializes `query`, including an empty string:

```json
{"v":1,"type":"search","requestId":"r","query":"","limit":20}
```

An omitted query is invalid. A missing limit decodes as 20; explicit zero, negative, fractional and >100 values are invalid. This is separate from the public Local Client request, which requires a profile ID and an explicit limit. No default or field is silently copied from the Local Client representation into the Native channel.

Each result item contains `title`, `url`, `domain`, and `openRef`. Titles are nonblank and URLs are HTTP(S) with a hostname; result references use hyphenated RFC-variant UUIDs, versions 1–8. Empty results serialize `items: []`, never omit the array. Order is preserved. The Native open request treats a reference as an opaque nonempty string of at most 100 UTF-16 units, matching Kwel's lookup boundary. The public client API separately requires a UUID, and Kwel rechecks its own reference table before opening. A URL is never accepted as an open operation.

Kwel emits `BAD_REQUEST`, `INCOMPATIBLE_VERSION`, `NOT_READY`, `RESULT_EXPIRED`, `OPEN_FAILED`, and `SEARCH_FAILED`. The legacy Native error vocabulary also retains `INVALID_REQUEST`, `PROFILE_CONFLICT`, `HOST_NOT_FOUND`, `DISCONNECTED`, `TIMEOUT`, and `PERMISSION_REQUIRED`. Local installation/resource errors (`BUSY`, `UNSAFE_PATH`, `INSTALL_CONFLICT`, `DOWNGRADE_REFUSED`, `HOST_IN_USE`) remain outside the Native wire.

## Correlation and uncorrelated errors

Normal search/open responses require a nonempty request ID exactly matching their pending Native request. The Host generates a distinct Native ID for each request and restores the caller's local ID only after validation. Unknown IDs never consume a different pending request.

The Host accepts `requestId: null` on `error`. A legacy omitted error ID has the same uncorrelated-notice meaning; serialization emits explicit `null`. An empty string ID is rejected. An uncorrelated notice is ignored by response dispatch: it cannot satisfy a search, open or probe, consume a pending entry, renew a deadline, or be assigned to an arbitrary waiting client. `Message::response()` rejects an uncorrelated error even if the caller supplies an empty expected ID.

```json
{"v":1,"type":"error","requestId":null,"code":"BAD_REQUEST","message":"Invalid request."}
```

## Sessions and authorization

Hello must arrive within five seconds. Its profile ID selects an OS-user-owned session; Profile ID is an identifier, not an authentication token. Public `LocalRequest` validation requires that ID and the server matches it to its endpoint. Only after this check does `Session::request()` remove client routing and forward a Native request. Private migration/coordination messages use their own control type and cannot be encoded as Native messages.

A duplicate profile receives no hello-ack, and an established session rejects requests with `PROFILE_CONFLICT` while a duplicate overlaps. Native requests are bounded to 32 pending operations and a 12-second deadline. Probe ping/pong and probe serialization have separate two-second bounds. Disconnect cancels pending work.

openRefs expire after five minutes, are consumed once before opening and stay within one Native connection/profile. The Host retains at most 500 references. Kwel independently enforces its own single-use, expiry, connection, HTTP(S), deletion, Block and current-search checks. Clients cannot request an arbitrary URL.

## Canonical shared conformance corpus

[`docs/fixtures/native-messaging-v1.json`](fixtures/native-messaging-v1.json) is the canonical language-independent Native v1 corpus adopted unchanged from Kwel PR #211 at `1ac876f8859e9f95100d7fcae4df86b3d7af890c`. Its SHA-256 is `27eeee1369b438e3376b16451e6dc011752c9d98ca8ec2af6e7845a2c30c0227`. [Provenance](fixtures/native-messaging-v1.source.json) records that commit, source hashes and canonical ownership. It contains 65 cases covering requirements, responses, bounds, Unicode, empty strings, errors and forbidden fields.

Rust tests load this JSON directly. The mandatory TypeScript CI job verifies the corpus hash, checks out that exact Kwel commit, requires byte-for-byte fixture and protocol-source agreement, and runs Kwel's `pnpm test:local-protocol`. It never substitutes a locally modified fixture to make tests pass. The Release workflow's reusable `validate` includes both Rust and TypeScript checks; failure prevents publishing.

All 81 archived Go-reference cases remain in `crates/kwel-protocol/tests/fixtures/go-v1.json`. They are historical input data, not the normative wire schema. Tests document corrections: missing query is rejected, empty search query and empty result arrays serialize explicitly, and profile-routed open requests are checked as Local Client requests. No Go toolchain or Go source is needed.

Protocol conformance and synthetic Native/IPC tests do not establish actual Chrome/Alfred UI behavior, installer approval prompts or user-profile compatibility. Keep those manual/OS checks distinct from CI and coordinate the Kwel/Host release.
