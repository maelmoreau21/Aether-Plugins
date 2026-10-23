# LRCLIB

Paroles depuis [lrclib.net](https://lrclib.net), sans clé API (`base_url` pour une instance auto-hébergée).

- `search` / `details` : paroles simples dans `synopsis`.
- Opération dédiée `lyrics.get` (`{id}` ou `{track, artist, album?, durationSecs?}`) : renvoie `plain`, `synced` (LRC brut) et `syncedLines` (`timeMs`/`text`). Recherche exacte puis repli sur la recherche.
- Contrat `lyrics.get` et capacité `Lyrics` définis dans le SDK d'Aether (`media-extension-sdk`).
