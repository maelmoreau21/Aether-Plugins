# OpenSubtitles

Optional REST provider, not installed automatically. A personal application API key is
required. An optional OpenSubtitles username/password selects that account's quota.
No key, account or unlimited allowance is bundled. Configuration remains private to
the installation; the provider returns only bounded error codes.

Generic Wasm ABI v1 is unchanged. Signed permission: `subtitles`. Operations:

- `subtitles.search`: input `{title,year?,mediaType:"movie"|"episode",externalIds,parentExternalIds?,language,forced?,sdh?,page?,seasonNumber?,episodeNumber?,movieHash?,requireHash?,config}`.
  Output `{items:[{id,language,label,forced,sdh,match:"identity"|"title",hashMatch,machineTranslated,aiTranslated,downloadCount}],totalPages}`.
  Each ID is a decimal OpenSubtitles `file_id`. Criteria, in order: `movieHash`, IMDb, TMDb, series
  IDs (`parentExternalIds`, episodes only), then title. Blank identifiers are ignored; a non-blank
  malformed one is an error, never a silent downgrade.
  Identity means a search by identifier or hash, not a guarantee of timing: `hashMatch: true`
  is the only statement that the provider matched the caller's own video file.
  - `movieHash` is **OpenSubtitles' hash** (file size plus the 64-bit sums of the first and last
    64 KiB, 16 hex digits). It is not Aether's content fingerprint (`b3s1:`/`b3f1:`, BLAKE3 over
    samples, used to follow renamed files): that value is refused with `invalid_movie_hash` and is
    never sent. `requireHash` limits results to hash matches (`movie_hash_required` without one).
    Files matching the hash are listed first; the provider's order is kept otherwise.
  - `language` is one code with an optional regional or script subtag, any case: `fr`, `pt-br`,
    `PT_pt`, `zh-cn`, `zh-hant`. `pt-br` returns only `pt-br`; `pt` returns every Portuguese
    variant. Results in another language are dropped. A numeric region (`es-419`) is reported as
    its primary language because the Core accepts alphabetic subtags only.
  - `forced`/`sdh`: `true` = only, `false` = exclude, absent = both. `false`/`false` is the
    complete track. The flags are also enforced on the answer, in case the provider ignores them.
  - Episodes need the season and episode numbers unless an episode ID or `movieHash` designates
    them (`season_episode_required`). The year is not sent for an episode title search.
  - Files repeated across results are returned once; a label is never empty.
- `subtitles.download`: input `{fileId,config}`; output `{url,remaining?,resetTimeUtc?}`.
  The Core must authorize and correlate the result to a prior search, deduplicate
  before this call, fetch with DNS-pinned HTTPS and no provider headers, normalize
  the subtitle, and persist it outside media folders. The temporary URL is private.

The API consumes download allowance when issuing the link, even if the later file
fetch fails. Never blindly retry this operation after an ambiguous network failure.
The plugin does not download text through Wasm JSON, alter media, synthesize a hash,
automatically select a result, or automate downloads. It refuses redirects; search
parameters are canonicalized to avoid the service's parameter-order redirects.
Unknown API/CDN hosts fail closed and require review before adding signed hosts.
Tokens are scoped to one invocation; failed login is not retried automatically.

Service reference checked 2026-11-03:
[REST API](https://ai.opensubtitles.com/api-docs),
[getting started](https://opensubtitles.tawk.help/article/getting-started),
[paid consumer plans](https://opensubtitles.tawk.help/article/pro-packages).
Quotas and subscription conditions depend on the actual API consumer/account;
the returned `remaining` and reset time are authoritative for that response.

Error codes (the Core publishes only the code): `missing_api_key`, `unauthorized` (401/403),
`invalid_credentials` (401 on login), `not_found` (404/410), `invalid_request` (400/422),
`download_quota_exceeded` (406 on a download only), `rate_limited` (429/503), `timeout` (408,
silence, broker DNS timeout), `provider_unavailable` (502/504), `provider_redirect_refused`,
`truncated_response`, `invalid_provider_response`, `provider_response_too_large`, `provider_error`;
broker refusals keep their meaning (`quota_exceeded`, `domain_not_allowed`).
`Retry-After` is applied by the Core broker (429/503); the guest has no clock and never waits.

Offline checks: `cargo test -p aether-plugin-opensubtitles` (unit contracts, corpus under
`tests/corpus`, scripted-server failures), then release Wasm build and
`node scripts/test-subtitles-wasm.cjs`.
These checks do not establish live service access, a licensed account, NAS playback,
or production compatibility. No live request or package publication is performed.
