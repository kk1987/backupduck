# BackupDuck protocol v2

Every route requires `Authorization: Bearer <token>`. Assets and responses use
JSON. Chunks use raw bytes. Maximum chunk size is advertised in capabilities and
is currently 4 MiB. Maximum manifest size is 64 KiB.

| Method | Path | Meaning |
| --- | --- | --- |
| GET | `/v2/capabilities` | Protocol version, chunk bound, supported asset/target capabilities |
| POST | `/v2/assets` | Validate/register a manifest and return persisted status |
| GET | `/v2/assets/{id}` | Query resource offsets, receipt and processing state |
| PUT | `/v2/assets/{id}/resources/{sha256}?offset=N&sha256=CHUNK_HASH` | Upload a verified chunk |
| POST | `/v2/assets/{id}/commit` | Verify all original resources and atomically acknowledge the asset |
| GET | `/v2/publications?cloud=due\|all&after=ID&limit=N` | List published gallery copies for a cloud auditor (`cloud_audit`) |
| POST | `/v2/cloud-observations` | Record cloud auditor verdicts for gallery copies (`cloud_audit`) |

Registration is idempotent. A full-file SHA-256 identifies each original resource.
The `sha256` query parameter is the digest of this request's chunk, not the entire
resource. Only resources belonging to the registered asset may be written.

Offsets are contiguous byte offsets. A replay wholly inside received bytes must
match those bytes. Gaps, conflicting bytes, and partial overlaps return conflict.
On a lost response or interruption, query status before scheduling another chunk.
After an HTTP error the caller must not infer that an operation did not happen.

Receipt statuses are `receiving` and `received`. Processing statuses are
`not_requested`, `pending`, `complete`, and `failed`. Processing completion has no
implied Google Photos cloud meaning. No deletion endpoint exists. Asset status may
include `cloud_state`; older receivers omit it.

Typical errors: 400 invalid input, 401 missing/wrong authentication, 404 unknown
asset/resource, 409 conflict/incomplete asset, 413 oversized request, 422 integrity
failure, 429 busy, 507 capacity exhausted, 500 storage/internal failure. Original
paths and database errors are not returned over HTTP.


A received receipt records that all original resources were verified and accepted
at commit time. Receiver-local explicit retention policies can subsequently reclaim
originals after a verified archive or opt-in verified gallery relay. This does not
change asset identity, resume offsets or historical deduplication. Publication and
cloud backup are separate from transfer receipt; no cloud-proof claim is introduced.

## Optional complete-asset upload

A receiver advertising `bundle_upload: true` accepts `POST /v2/bundles` with
`Content-Type: application/x-backupduck-bundle`. Missing capability means false.
The file-backed envelope contains eight bytes `BDCK0002`, a four-byte big-endian
manifest length, the UTF-8 JSON manifest, then each resource's original bytes in
manifest order. Manifest and resource bounds are unchanged. A provided content
length must exactly match the envelope; trailing or missing bytes prevent commit.

The receiver streams bounded chunks into the same durable store. A replay verifies
existing bytes before appending; both original resources of a motion photo must
pass whole-file verification before a received receipt is returned. An existing
receipt remains authoritative even after explicitly authorized local reclamation.
A lost response is safe to replay, though the client currently resends the envelope
from its beginning. Normal receiver-side publication is independent of this request.

Apple senders cache the authenticated capability and prepare up to four immutable
file-backed OS requests. Already-started legacy transfers finish their existing
sequence. When the additional envelope would exceed the sender's staging budget
or free-space reserve, it uses the original bounded chunk path. No extra background
execution entitlement is implied; actual locked-device scheduling remains subject
to the operating system.

## Cloud verification

A receiver advertising `cloud_audit: true` lets an external auditor check whether
published gallery copies reached the cloud. Missing capability means false. The
receiver never contacts a cloud service; it records verdicts it is given (the
macOS sender is one such auditor, see
[architecture](architecture.md#cloud-audit)). Gallery evidence carries the SHA-1
of the gallery-copy bytes, which cloud media lookups use, beside the SHA-256.
Copies published before SHA-1 evidence existed are re-read on the receiver and
gain it later.

`GET /v2/publications?cloud=due&after=ID&limit=N` returns
`{"items":[{asset_id,sha1,sha256,size,display_name,kind,published_at_ms,cloud_state,cloud_checks}],"next":ID|null}`
in asset ID order. `limit` is 1–100 (default 100); `after` is an asset ID from a
previous `next`, which is null once a page is not full. `due` lists published
copies in state `pending` or `verified_counts_against_quota` that are at least ten
minutes old and whose last lookup is older than ten minutes doubled per previous
check, capped at one day. `cloud=all` lists every published copy with SHA-1
evidence, regardless of state or timing.

`POST /v2/cloud-observations` takes at most 100 observations in at most 64 KiB:
`{"observations":[{"asset_id":ID,"sha1":HEX,"result":"free"|"counts_against_quota"|"not_found","media_key":S?,"device_model":S?}]}`. `device_model` is the EXIF camera model Google Photos reports for the item (for example `iPhone 17 Pro` for an iPhone original pushed through the Pixel), not the uploading device; only `result` carries the quota verdict.
Optional strings are at most 64 bytes without control characters. Malformed input
returns 400. An observation is rejected, not applied, when the asset is unknown or
unpublished, or its SHA-1 differs from the stored copy (a stale verdict). `free`
sets `verified`; `counts_against_quota` sets `verified_counts_against_quota`;
`not_found` counts a check and sets `missing` once the copy was published more
than seven days ago. Quota and missing copies can still become `verified`;
`verified` never changes again. The reply counts resulting states:
`{"verified":N,"quota":N,"still_pending":N,"missing":N,"rejected":N}`.

Cloud states are `unknown` (no SHA-1 evidence yet), `pending`, `verified`,
`verified_counts_against_quota` and `missing`. A verdict is the auditor's report,
not a receiver-side proof, and nothing is deleted because of it.
