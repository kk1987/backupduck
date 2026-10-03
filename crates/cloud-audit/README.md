# backupduck-cloud-audit

Checks whether originals uploaded from a Pixel exist in a Google Photos library
and whether they count against storage quota. It talks to the private RPC that
the photos.google.com web app uses.

**This is an undocumented, unsupported interface.** Google can change or block
it at any time, and using it may conflict with Google's terms. Treat every
result as advisory, run it by hand, and keep call volume low.

The protocol notes below come from
[xob0t/Google-Photos-Toolkit](https://github.com/xob0t/Google-Photos-Toolkit)
and [xob0t/google_photos_web_client](https://github.com/xob0t/google_photos_web_client)
(both MIT). No code is copied from them; parser fixtures are vendored under
`tests/fixtures/` with their licence.

## Exporting cookies

The tool authenticates with a Netscape `cookies.txt` from a signed-in browser
session. **That file grants full access to the Google account** (mail, drive,
everything), not just Photos.

1. Open a dedicated private/incognito window and sign in to
   photos.google.com. Use this window for nothing else.
2. Export cookies for photos.google.com with a local-only exporter such as the
   "Get cookies.txt LOCALLY" extension (allow it in private windows).
3. Close the window without signing out; signing out revokes the cookies.
4. Store the file outside any repository with `chmod 600`. The CLI warns when
   group or others can read it. Delete it when done.

Only cookies for `google.com` and its subdomains are loaded; values are never
printed.

## CLI

```
backupduck cloud-audit status --cookies cookies.txt [--account N]
backupduck cloud-audit lookup --cookies cookies.txt [--account N] FILE...
```

`--account N` selects `photos.google.com/u/N/` in a multi-login session.
`status` prints the account index and whether the session is valid; it never
prints the account email. `lookup` prints one JSON object per file: SHA-1,
whether a library item has that hash, media key, camera model, the quota flags
and a verdict (`free`, `counts_against_quota`, `not_found`, `unknown`).

## Protocol

Bootstrap: `GET https://photos.google.com/` (or `/u/N/`) without following
redirects, then read `<script data-id="_gd">window.WIZ_global_data = {...};`.
Keys: `SNlM0e` (XSRF `at`), `FdrFJe` (`f.sid`), `cfb2h` (`bl`), `Im6cmf` (RPC
path prefix, e.g. `/_/PhotosUi`; Toolkit reads `eptZe`). A redirect, 401/403 or
a missing key means the session expired. The account email (`oPEP7c`) is not
read.

RPC: `POST {Im6cmf}/data/batchexecute?rpcids=ID&source-path=/&f.sid=..&bl=..&rt=c`
with form body `f.req=[[[ID, "<payload JSON>", null, "generic"]]]&at=<SNlM0e>`.
The response starts with `)]}'`; lines containing `"wrb.fr"` hold
`["wrb.fr", ID, "<payload JSON>", ..]`, and an empty payload string is an RPC
error. HTTP 429 is rate limiting.

| RPC | Payload | Used for |
| --- | --- | --- |
| `swbisb` | `[[sha1_base64, ..], null, 3, 0]` | library items matching file SHA-1s; unmatched hashes are absent |
| `EWgK9e` | `[[[[[key], ..]], [[null x24, [], null x10, []]]]]` | name, size and quota vector for up to 50 media keys |
| `VrseUb` | `[key, null, null, null, null]` | single item; quota vector only (lower confidence) |
| `fDcn4b` | parser only | extended single-item info |

The quota vector is `[takesUpSpace, spaceTaken, quality, ..]`: `takesUpSpace`
is `1` when the item counts against quota, `quality` is `2` for original
quality. Field indices are documented in `src/parser.rs`.

## Limits

- Sequential calls at least 300 ms apart, at most 24 RPCs per run, 50 hashes
  or keys per RPC. A 429 waits 30 s and retries once; an empty response retries
  once after 2 s.
- Matching is by SHA-1 of the local file bytes. Whether items Google stored
  re-encoded (storage saver) still match the original's hash is unverified.
- The live RPC behaviour has not been verified by this project yet; parsers are
  tested against upstream's recorded responses only.
