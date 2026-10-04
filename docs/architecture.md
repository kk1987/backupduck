# Architecture and boundaries

## Asset model

An asset is a logical photo, video, or motion photo. Its manifest contains source
identity, revision, metadata, and ordered original resources. A motion asset has
one photo and one paired video. Resource digests refer to exact original bytes;
existing EXIF and container metadata therefore survive transport unchanged.
Gallery-only metadata is carried separately by a native source adapter.

The asset ID is SHA-256 of the validated compact JSON serialization. Metadata keys
are sorted. Bindings should call Rust's ID function rather than invent their own
JSON canonicalization. Resource order is part of the contract. Content digests
allow resource deduplication even when two clients have different source IDs.

## Shared Rust responsibilities

The common engine owns identity, transfer actions, acknowledgement semantics,
validation, and target-independent processing states. The HTTP implementation is
a reference executor, not a mandatory network runtime for every native app.
Native background upload engines execute persisted actions and reconcile with
receiver status after interruptions. Source file handles must not be assumed to
be universal filesystem paths. The current CLI materializes regular files; native
hosts must supply an appropriate resource export/access implementation.

The sender queue persists manifests, opaque source access references, receiver
identity, attempts, retry timestamps, pause state, confirmed progress and OS task
IDs. Each claim receives a generation. Acknowledgements from superseded attempts
are rejected. Foreground attempts left running on process exit are requeued;
OS-managed attempts stay attached until the native host reconciles its task list.
Unknown OS tasks are returned for cancellation. Missing tasks query receiver state
before uploading again. Network and capacity errors retry with capped exponential
backoff; busy replies retry every 60 s (±10 s) without growing or spending an
attempt. Authentication, integrity and source-access failures require attention.

The iOS host executes immutable file-backed requests through background URLSession.
Rust prepares register/chunk/commit operations and atomically records each reply
with its next-operation checkpoint. Swift binds the OS task before resume and
reconciles live tasks on process startup. No Rust network future needs to survive
suspension. Native PhotoKit change tokens and pending identifiers are persisted
atomically before export; the shared queue deduplicates the exported asset.
BGProcessingTask provides opportunistic discovery and retry wakes, not a timer or
an assurance of immediate work while the app is closed.

## Preparation retry policy

Sources wait in the maintenance database until their originals are exported and
queued. A failed preparation retries five minutes later and records a fixed error
code; caller text is never stored. Item-specific reasons (unavailable or hidden
originals, limited photo access, unsupported formats, export failures, an item
larger than the staging cache budget, and unknown codes) count toward a cap.
After five such failures the source is parked as `needs_attention`: it leaves the
preparation queue and delayed-work browsing, history rows label it, and
`needs_attention` lists it with its code and attempt count. Environmental reasons
(cache budget, local free space, network, receiver capacity, low space, busy,
receiver unavailable, cancellation) never spend the cap and keep retrying.

`retry_source` and `retry_all_needs_attention` return parked sources with a fresh
cap. `skip_source` removes one; a later history scan may schedule it again because
skipping is not a receipt. Rescheduling never revives a parked source. Saving
storage settings resets every cap, since a larger budget may resolve parked items;
an item that still fails costs at most five more exports. The PhotoKit discovery
queue hands a source to this queue after five failed attempts instead of retrying
it on its own.

An asset whose new bytes exceed the receiver's entire budget is rejected with
HTTP 507 and reason `receiver_budget_single_item`. The sender records it as a
failed task that needs a manual retry after the budget is raised, instead of
waiting forever like ordinary capacity pressure. Older receivers report plain
capacity, which keeps the automatic retry.

## Apple library scope

Every PhotoKit fetch on iOS and macOS goes through `photoLibraryFetchOptions()`,
so the grid, counts, historical scans, change history, burst expansion and
export see one membership: all burst frames, plus the Hidden album unless the
"Include Hidden Photos" setting (`includeHiddenPhotos`, default on) is off. The
default is on because a silent exclusion would leave hidden items out of a
whole-library migration. Turning it off makes hidden items missing from those
fetches; `importAssets` re-fetches the missing identifiers with hidden assets
included and reports the ones it finds as `hidden_excluded`, an item-specific
code that parks after the cap, instead of a missing original. A running
historical scan keeps the identifiers it captured at its start, so a changed
setting applies to the whole library only after "Check existing photos again".

A Hidden album locked with Touch ID, Face ID or a password in Photos is withheld
from third-party apps: on macOS with the lock on, PhotoKit returned no hidden
assets even with `includeHiddenAssets` (the Hidden smart album counted 0; after
turning the lock off it counted 1288 and the library total grew by the same
amount). There is no API for the lock state, so `LibraryScopeSettingsSection`
shows the `smartAlbumAllHidden` count PhotoKit currently exposes and, when it is
0 with the setting on, tells the user to turn the lock off for the migration.
A process that was already running saw the change only after several minutes
(the cause is not known); a relaunch shows it right away.

## Rescan and content identity

Gallery metadata (favorite, location, capture date, burst fields) is part of the
asset ID, and the PhotoKit revision changes with every edit. Exact
source/revision matching alone would therefore publish a second gallery copy
after a rescan or a metadata-only edit. Two rules prevent that. A source that
was received under any earlier revision counts as known: `history_batch` and the
Apple pre-download check treat `received_previous` like `received` unless the
user enabled "Back up edited photos again" (`rebackup_edited`) or explicitly
asked to back up a selection again. Independently, `Sender::enqueue` returns
the existing received job when the same receiver already holds the same source
with the same kind and identical `(role, sha256)` resources, logging
`transfer_deduplicated` instead of inserting. A `backupduck_rebackup_id` in the
metadata bypasses this check. Apple senders export original resources, so edits
made in Photos never change these digests; folder sources deduplicate a touched
but unchanged file the same way. Receiver-side content indexing across sources
is not implemented.

## Receiver durability

The receiver has a single writer lock for its root. SQLite stores assets and
unique blob reservations. Registration reserves all new resource sizes in one
transaction, so a rejected motion asset cannot leave half a reservation behind.
Uploaded bytes are written into hash-named partial files. Each accepted chunk is
synced before acknowledgement. Its actual durable length is the resume offset.
Exact chunk replays are allowed; contradictory or overlapping writes are rejected.

After the full resource hash matches, the file is renamed into the blob store and
its ready flag is recorded. A restart reconciles a complete partial file or a
renamed blob whose ready flag was not committed. The final asset commit verifies
all resources and records receipt. Missing committed files are integrity failures,
not evidence that a source should automatically upload again.

Capacity is a reservation budget for original resource bytes, including unfinished
uploads. It is not a physical disk-free-space guarantee. Filesystem exhaustion
returns an error without committing the asset. Native receivers must also expose
actual space/temperature constraints. An unreceived asset with no register, chunk
or commit for 7 days is abandoned: the receiver deletes its row and frees any
reserved blob and partial file no retained asset references. This runs when the
store opens and hourly from the Android receiver, and is logged as
`abandoned_reservations_expired`. A sender resuming an expired asset gets 404,
which is a network retry that drops its checkpoint, so it registers again from
offset zero. There is no explicit reservation cancellation.

The Android receiver reads battery temperature and Android's thermal status while
running. Temperature protection is on by default at 40°C (adjustable from 35–45°C,
or off). At the threshold or severe system thermal status it holds admission of
new assets and subsequent upload chunks. It resumes after cooling below the
threshold by 2°C and after the system status falls below moderate. The hold is
combined with Google Photos cleanup holds, so one guard cannot clear the other.
Senders get 409 busy while it holds and retry about once a minute (60 s ±10 s,
not exponential), so transfers resume within roughly a minute of cooling, from
receiver-confirmed offsets; the original bytes and receipts are retained. The setting controls BackupDuck's receiver, not
Android's own thermal management.

## Receipt and target processing

Receipt: `receiving -> received`.

Optional target processing:
`not_requested -> pending -> complete`, or `pending -> failed -> pending`.

A received asset remains received when processing fails. Ordinary Pixel assets
request original publication. Motion assets request Motion Photo generation only
if the Android host has a real converter. The Android host invokes native codecs, Rust container packaging and MediaStore
publication. The generic protocol still advertises no target-specific processing
capability; automatic capability negotiation remains future work. Receiver
receipt never claims Google Photos cloud backup.

### Cloud verification state

Gallery evidence records SHA-1 as well as SHA-256 of the published copy's bytes.
An external auditor lists published copies over the protocol, looks each up by
SHA-1 in the user's cloud library, and reports `verified`,
`verified_counts_against_quota` or not found; copies not found a week after
publication become `missing`. The receiver stores the state per asset and rejects
verdicts for bytes other than the stored copy. A receipt still does not mean the
copy is in the cloud: only a `verified` state reflects an auditor's lookup, and
only the opt-in [cloud-verified release](#cloud-verified-release) acts on it.
Copies published before SHA-1 evidence are re-read by the
Android receiver's background sweep to add it. See
[protocol](protocol.md#cloud-verification).

### Cloud audit

The macOS sender is the only auditor. Its native library is built with the
optional `cloud-audit` feature (never on iOS or Android) and links
[`backupduck-cloud-audit`](../crates/cloud-audit/README.md), which talks to the
undocumented Google Photos web RPC using a `cookies.txt` exported from a
signed-in browser. The user picks the file in Settings > Google Photos; the app
keeps only a security-scoped bookmark and Rust reads the file per run. Cookie
values, the path and the account email never appear in results or logs.

A run requires the receiver's `cloud_audit` capability, pages
`/v2/publications?cloud=due` up to 600 copies, looks them up by SHA-1, fetches
quota flags for the matches and posts `free`, `counts_against_quota` or
`not_found`. A match without a quota flag is not posted, so the copy stays
`pending` and is looked up again next run. Calls are sequential and capped per
run; rate limiting, the call budget or an expired session end the run with
partial counts. A dry run looks up without posting. Verdicts are mirrored onto
the sender's own jobs (`cloud_observations`), which drive the transfer list,
summary counts and the library grid's `backed_up` badge. The app runs it every
ten minutes when enabled and, by default, pauses backup when newly uploaded
copies count against Google storage.

A verdict means a library item with the same SHA-1 as the gallery copy exists
and Google reported its quota flag at that moment. It does not prove the item
stays in the account, that it is the same account the Pixel uploads to, or that
storage-saver re-encodes would match; the live RPC behaviour is unverified by
this project.

### Cloud-verified release

The Android receiver can delete its own gallery copy, and the originals it keeps
for it, once the cloud has the copy's exact bytes. It is off by default
(`cloud_release` in receiver settings; `cloud_release_grace_ms` defaults to one
hour, at most seven days). It never contacts a cloud service and relies on the
auditor's verdicts above.

| `cloud_release` | `receiver_relay` | On gallery publication | Later |
|---|---|---|---|
| off | off | keep originals and copy | unchanged |
| off | on | release originals (RC.3 relay, within its scope) | relay sweep; manual historical inspection |
| on | off or on | keep originals and copy | cloud release only; relay rows, manual inspection and `release_gallery` are refused |

A copy is eligible when it is received, published, `verified` (free at original
quality), not yet released, and its verdict is at least the grace old.
`verified_counts_against_quota`, `missing`, `pending` and `unknown` never are.
Rows whose originals relay or archive already released are still eligible; only
the gallery copy remains to delete.

The Android receiver runs a bounded sweep (four items) in the 5-second reception
loop, under the media-operation mutex, skipped during manual relay inspection
and cooling or Google Photos cleanup holds. For each candidate:

1. Re-read the MediaStore copy: owned by this package, in `DCIM/BackupDuck/`,
   not pending or trashed, SHA-256, SHA-1 and size equal to the stored evidence.
2. `release_cloud_verified`: the store re-checks the setting, eligibility and the
   proof, durably marks `originals_released=1, release_reason='cloud'`, then
   deletes unreferenced blobs.
3. Delete the MediaStore row, expecting one row.
4. `mark_gallery_released(id, "cloud")` sets `gallery_released` and its time.

A crash before step 3 repeats the item: step 2 is idempotent. A crash between 3
and 4 finds the row gone on the next sweep and records it as `missing`. If step
1 finds no MediaStore row at all, the SHA-1 match already proved the cloud has
those bytes, so the originals are released against the stored evidence and the
copy is marked `missing`. A row that exists but is not ours, moved, pending or
changed keeps everything; it is skipped until the service restarts and logged
once as `cloud_release_waiting`. A `SecurityException` on delete (lost
ownership) logs `cloud_release_not_owned` and leaves the copy for the user.
Events carry amounts only: `cloud_originals_released` (bytes),
`cloud_gallery_released` and `cloud_gallery_missing` (copy size).

Deleting a phone copy assumes Google Photos keeps its cloud item when another
app removes the local file. This project has not yet verified that on a device;
the receiver cannot check it.

Conversion location can be optimized later through capability negotiation without
changing asset identity or the original-resource receipt contract. Original
retention and derived-output cleanup require an explicit target policy.

## Native hosts

- iOS/macOS: photo access, permission UI, native transfers/scheduling and media APIs.
- Android: source gallery access and/or receiver lifecycle, MediaStore publication,
  hardware media APIs, device conditions.
- Windows: selected file sources, native desktop UI and lifecycle integration.

Hosts may be senders, receivers, or both. Only Pixel has a defined target extension.
There are no invented NAS, cloud-account, billing, or multi-tenant abstractions.

## Security scope

The executable binds loopback and uses a private randomly generated bearer token.
The client permits HTTPS origins or loopback HTTP, rejects credentials in URLs,
and does not follow redirects. Routes authenticate before body extraction. Chunk
size, manifest size, concurrent requests and request duration are bounded. Client
filenames never become receiver storage paths. The receiver root is trusted and
must not be modified by another application while it is serving.

The native receiver hosts HTTPS using a private generated identity. Its pairing
code contains a certificate and bearer token; clients verify the certificate
identity and TLS hostname. iOS keeps the pairing in its device-only Keychain.
The preview uses one receiver-wide token and a fixed network address. One-time
pairing, per-client revocation, discovery and multi-user isolation remain future
work. It is not an Internet-facing service.


## Local receiver presentation

`ReceiverHistory` is a host-only FFI query, not a network route. Its catalog opens
SQLite read-only and uses stable descending row cursors, filters before pagination,
and returns at most 100 items per page. Reading progress only inspects file lengths;
it does not invoke transfer verification, rename partial files, or remove blobs.
A retained receipt records completed receipt even after verified originals were
archived and reclaimed, or after the user opted into receiver relay retention.
Relay reclamation requires a fresh host-side hash of a ready delivery copy,
matching durable expected/publication evidence; it is distinct from an original
archive. It does not assert that a gallery copy or cloud backup still exists.
See [receiver retention](device-and-backup-experience.md#optional-receiver-relay-mode).

Android uses RecyclerView/ListAdapter for reusable rows and asynchronous diffs.
Visible-page refreshes do not reload every previously browsed page. Thumbnail
work is limited to two concurrent requests and a 12 MiB cache. MediaStore supplies
160-pixel previews; unsupported previews use a media icon. Theme and font changes
use native configuration handling. Main navigation views stay alive across tabs.

`ReceiverSettings` and `ReceiverLogs` are local commands that also work while the
network service is stopped. Saved settings before the first start are retained;
they are not overwritten by the host's initial capacity argument. No credentials
or original-resource paths are returned by these presentation queries.

Platform references: [Material navigation](https://github.com/material-components/material-components-android/blob/master/docs/components/BottomNavigation.md),
[RecyclerView ListAdapter](https://developer.android.com/reference/androidx/recyclerview/widget/ListAdapter).


## Background diagnostics

Lifecycle snapshots query whole-queue Rust counts and enumerate the platform's
system requests. The journal records pending exports, queued/running/waiting/failed
counts, pause state, request stage, system-reported bytes and the next retry time.
Completion callbacks include HTTP status and numeric platform error codes; only
a separately validated receiver receipt proves receipt. System-reported sent
bytes are diagnostic, not durable confirmed progress.

Dispatch decisions are recorded when their reason or queue/import state changes,
not on every unchanged foreground poll. The UI keeps these fields under an
expandable Details row and includes them in the existing native log export.
Context uses a Rust allowlist of typed counters and fixed enum values; paths,
source identifiers, endpoints and credentials cannot be added as free-form
context. The maintenance database adds a nullable context column, preserving old
events and the user's retention settings. Missing context in an old event means
unknown, not zero pending work. A short background completion or a manually
recorded test snapshot does not prove sustained lock-screen execution.

## Desktop folder-source capability

The optional `backupduck-folder-source` crate produces generic assets from a
read-only, persistent folder index. Mac owns folder authorization, filesystem
notifications and media metadata APIs; the existing Rust sender owns snapshots,
checksums, durable tasks and transfer. Mobile native builds never link the folder
crate. Source identity is namespaced without changing existing PhotoKit IDs.
See [folder backup](folder-backup.md) for behavior and validation boundaries.
