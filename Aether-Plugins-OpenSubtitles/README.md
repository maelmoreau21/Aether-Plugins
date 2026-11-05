# OpenSubtitles

Optional REST provider, not installed automatically. A personal application API key is
required. An optional OpenSubtitles username/password selects that account's quota.
No key, account or unlimited allowance is bundled. Configuration remains private to
the installation; the provider returns only bounded error codes.

Generic Wasm ABI v1 is unchanged. Signed permission: `subtitles`. Operations:

- `subtitles.search`: input `{title,year?,mediaType:"movie"|"episode",externalIds,language,forced?,sdh?,page?,seasonNumber?,episodeNumber?,config}`.
  Output `{items:[{id,language,label,forced,sdh,match:"identity"|"title",downloadCount}],totalPages}`.
  Each ID is a decimal OpenSubtitles `file_id`. IMDb then TMDb are preferred over title.
  Identity means a search by identifier, not a guarantee of timing or an exact hash match.
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

Offline checks: `cargo test -p aether-plugin-opensubtitles` and release Wasm build.
These checks do not establish live service access, a licensed account, NAS playback,
or production compatibility. No live request or package publication is performed.
