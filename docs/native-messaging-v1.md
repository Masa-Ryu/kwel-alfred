# Native Messaging Protocol v1

This document and the [Draft 2020-12 JSON Schema](schemas/native-messaging-v1.schema.json) are the Native v1 specification. Markdown defines transport, direction, defaults and session behavior; the Schema defines closed message shapes and structural constraints. The [Native65 corpus](../crates/kwel-protocol/tests/fixtures/native-messaging-v1.json) is conformance test data, not a separate specification.

Native Messaging connects **Google Chrome's Kwel extension ↔ `com.kwel.bridge`**. Enable **Local Connection** in Kwel Settings. Launchers use the separate [Local Client Protocol](local-client-protocol-v1.md), never Chrome's stdin/stdout transport.

| Boundary | Native Messaging | Local Client |
| --- | --- | --- |
| Profile selection | `hello.profileId` binds the whole connection | Every request requires `profileId` matching its endpoint |
| Search | Required `query`, including `""`; omitted `limit` becomes 20 | Required `query` and explicit `limit` |
| Open request | Opaque nonempty `openRef`, at most 100 UTF-16 units | UUID `openRef`; no arbitrary URL |
| Uncorrelated error ID | `null` or omission; encoding emits `null` | String, possibly `""`; never `null` |
| Discovery | No `profile-probe` operation | `profile-probe` on the public IPC endpoint |

## Transport and version

Chrome starts the Host with `chrome-extension://<extension-id>/` as its first argument. The manifest and Host origin check admit exactly the configured Kwel extension ID. A developer-loaded extension with another ID needs a separate local build; unknown IDs are never trusted automatically.

Each stdin/stdout frame is a four-byte **little-endian unsigned byte length**, followed by one UTF-8 JSON object. The payload is 1–1,000,000 bytes, excluding the header. The extension's `connectNative()` Port exchanges JavaScript objects; Chrome handles its framing. Host stdout contains framed messages only. Diagnostics must not contain queries, result data, profile labels or Profile IDs.

Reject empty, oversized or truncated frames, invalid UTF-8, duplicate keys, trailing JSON and unknown fields. All strings must contain Unicode scalar values: escaped surrogate pairs are valid, lone surrogates are not. These checks happen before or alongside Schema validation.

Every message requires numeric `v: 1` and a recognized `type`. Wire encoders emit `v` and `limit` as unsigned integer literals. Rust's typed decoder rejects decimal/exponent spellings such as `1.0`, even though JSON Schema considers that value an integer; ordinary `JSON.stringify()` emits `1` for it. Unsupported versions fail with `INCOMPATIBLE_VERSION` when a valid envelope can be decoded. An unknown type/field is invalid, not a forward-compatible extension. Incompatible wire changes require another protocol version; the Host's product SemVer is separate from `v`.

## Fields and directions

Only the listed fields are allowed, in addition to required `v` and `type`.

| Type | Direction | Required fields | Optional fields and decoder defaults |
| --- | --- | --- | --- |
| `hello` | Extension → Host | `profileId` | `profileLabel`: `""` |
| `hello-ack` | Host → Extension | none | none |
| `ping` | Host → Extension | none | none |
| `pong` | Extension → Host | none | none |
| `search` | Host → Extension | `requestId`, `query` | `limit`: 20 |
| `search-result` | Extension → Host | `requestId`, `items`, `hasMore` | none |
| `open-page` | Host → Extension | `requestId`, `openRef` | none |
| `open-result` | Extension → Host | `requestId`, `ok: true` | none |
| `error` | Extension → Host | `code` | `requestId`: `null`; `message`: `""` |

`profileId` is a hyphenated RFC-variant UUID, version 1–8, accepted case-insensitively and normalized to lowercase for routing. It is an identifier, not an authentication token. `profileLabel` is a string of at most 64 UTF-16 units without CR or LF. Request IDs are nonempty strings of at most 100 UTF-16 units. Queries are strings of at most 200 UTF-16 units; an empty query is valid and must not be omitted. `limit` is an integer from 1 through 100; `null`, zero, negative, fractional and out-of-range values are invalid.

`items` is a required array of 0–100 result objects. It must also fit the pending request's limit. Preserve order and original URL values; empty results use `items: []`. Each closed item requires:

| Field | Constraint |
| --- | --- |
| `title` | String containing a character outside Unicode `White_Space`, matching Rust `str::trim()` |
| `url` | HTTP(S) URL with a valid hostname/port and the checks below |
| `domain` | String; may be empty |
| `openRef` | Hyphenated RFC-variant UUID, version 1–8 |

Result URLs use WHATWG URL parsing with an explicit HTTP(S) authority. Reject controls, backslash repair, encoded hostnames, non-ASCII userinfo, malformed path/fragment escapes and invalid ports. IPv6 literals are supported; scoped IPv6 zone identifiers are not. The HTTP prefix pattern in the Schema is only a structural check; use the implementation URL validator as well. Validation never rewrites the URL. The Native `open-page.openRef` is a lookup token, not a URL: Kwel issues UUIDs and independently verifies its reference table.

## Minimal examples

The UUIDs below are synthetic examples, not user identifiers. Each example is one payload, excluding the frame header.

`hello` (extension selects the profile):
```json
{"v":1,"type":"hello","profileId":"00000000-0000-4000-8000-000000000001"}
```

`hello-ack`:
```json
{"v":1,"type":"hello-ack"}
```

`ping`:
```json
{"v":1,"type":"ping"}
```

`pong`:
```json
{"v":1,"type":"pong"}
```

`search` (required empty query; omitted limit decodes as 20):
```json
{"v":1,"type":"search","requestId":"r","query":""}
```

`search-result` (the array remains present when empty):
```json
{"v":1,"type":"search-result","requestId":"r","items":[],"hasMore":false}
```

`open-page`:
```json
{"v":1,"type":"open-page","requestId":"o","openRef":"00000000-0000-4000-8000-000000000002"}
```

`open-result` (failure uses `error`, never `ok: false`):
```json
{"v":1,"type":"open-result","requestId":"o","ok":true}
```

`error` (an uncorrelated notice):
```json
{"v":1,"type":"error","requestId":null,"code":"BAD_REQUEST"}
```

A nonempty item inside a search result:
```json
{"title":"Kyoto","url":"https://example.com/","domain":"example.com","openRef":"00000000-0000-4000-8000-000000000002"}
```

## Errors and correlation

The Host accepts these Native error codes:

| Codes | Meaning |
| --- | --- |
| `BAD_REQUEST`, `INVALID_REQUEST` | Invalid request |
| `INCOMPATIBLE_VERSION` | Unsupported protocol version |
| `NOT_READY`, `SEARCH_FAILED` | Search unavailable or failed |
| `RESULT_EXPIRED`, `OPEN_FAILED` | Reference expired/consumed or page opening failed |
| `PROFILE_CONFLICT` | Duplicate active Profile ID |
| `HOST_NOT_FOUND`, `DISCONNECTED` | Host/profile connection unavailable |
| `TIMEOUT`, `PERMISSION_REQUIRED` | Deadline exceeded or connection permission missing |

Current Kwel emits `BAD_REQUEST`, `INCOMPATIBLE_VERSION`, `NOT_READY`, `SEARCH_FAILED`, `RESULT_EXPIRED` and `OPEN_FAILED`. The other accepted codes are reserved in this v1 vocabulary; the Host also maps its own failures to Local Client errors. Installation/resource codes `BUSY`, `UNSAFE_PATH`, `INSTALL_CONFLICT`, `DOWNGRADE_REFUSED` and `HOST_IN_USE` are Local-only and must not appear on the Native wire. Error message text is explanatory; clients branch on `code`, not exact wording.

Search/open responses require a nonempty request ID matching a pending Native request. The Host generates a distinct Native ID for every operation, validates its response and restores the original Local Client ID. Unknown IDs never complete another request. `pong` has no request ID and is handled by the serialized probe path.

Only `error` accepts `requestId: null` or omission. They mean an uncorrelated notice; encoding emits explicit `null`. An empty string is invalid on Native errors. An uncorrelated error cannot complete a request/probe, consume a pending entry, renew a deadline or be assigned to any waiting caller. A well-formed notice is ignored by response dispatch. A malformed message, invalid direction or other protocol violation closes the Native transport and fails pending work.

## Session behavior and openRef

`hello` must arrive within five seconds. Its Profile ID selects an OS-user-owned session; no routing/profile fields are allowed on Native search/open requests. A duplicate profile gets no hello-ack. The established session fails closed with `PROFILE_CONFLICT` while the duplicate overlaps. Disconnect cancels pending work; a new connection creates a new reference namespace.

The Host allows 32 pending operations with a 12-second response deadline. Outbound sends and probe serialization/ping-pong waits have separate two-second bounds. The Native input queue is bounded to four messages. These are independent limits, not an unlimited retry mechanism.

openRefs expire after five minutes and are consumed **before** the open attempt, including attempts that fail. They belong to one Native connection/profile and cannot be reused or transferred. The Host retains at most 500 references. Kwel independently checks expiry, single use, connection, HTTP(S) and current page availability. Clients can only open a returned reference, never submit an arbitrary URL.

## Schema and runtime checks

Standard JSON Schema validation does not change the input: [`default` is an annotation](https://json-schema.org/understanding-json-schema/reference/annotations), not automatic value insertion. Native decoder defaults must be implemented explicitly. `x-utf16-maxLength` and `x-direction` are custom annotations and ordinary validators may ignore them. Normative string limits count **UTF-16 code units**, not Unicode code points: 200 Japanese BMP characters or 100 emoji fit a query; 201 or 101 do not.

| Validation layer | Responsibility |
| --- | --- |
| Draft 2020-12 Schema | Types, required/optional fields, closed objects, ranges, UUID syntax, result arrays and nonblank titles |
| Decoder and semantic validation | UTF-16 bounds, well-formed Unicode, integer wire spelling, duplicate keys, framing/byte limits and full URL safety |
| Session/behavior tests | Direction, request ID correlation, per-request result limit, order, timeouts, profile isolation and openRef expiry/single use |

[Rust Native tests](../crates/kwel-protocol/tests/native_v1.rs) validate all 65 unchanged fixture cases against Rust and the standard Schema. Exactly three UTF-16-overflow cases are accepted by standard code-point validation and rejected by Rust; the tests assert this distinction instead of changing expectations or skipping cases. [Boundary tests](../crates/kwel-protocol/tests/validation.rs) cover both Schemas and constraints outside them. The Schema validator is a pinned test-only dependency with network/file resolution disabled, not part of the Host or CLI.

The fixture's [source metadata](../crates/kwel-protocol/tests/fixtures/native-messaging-v1.source.json) records its initial Kwel adoption origin, not a moving implementation pin. Counterpart validation and release evidence belong in [development documentation](development.md#shared-native-v1-release-gate). Parser/synthetic tests do not establish real Chrome/Alfred or OS-installer behavior; use the separate [E2E procedure](chrome-e2e.md).
