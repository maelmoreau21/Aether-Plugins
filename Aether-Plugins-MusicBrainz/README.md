# MusicBrainz

Métadonnées d'albums (*releases*) via [MusicBrainz](https://musicbrainz.org) et pochettes via Cover Art Archive.

- Aucune clé. Un `User-Agent` explicite est envoyé (surchargeable via `extra.user_agent`).
- `external_ids.artist` affine la recherche. L'identifiant attendu est un MBID de *release*.
- Hôtes : `musicbrainz.org`, `coverartarchive.org`.
- Cadence : limitation de débit respectée (~1 requête/1.1s par défaut, configurable via `extra.min_interval_ms`).
- Limites : les redirections de Cover Art Archive vers archive.org dépendent du courtier HTTP de l'hôte (non testé en conditions réelles).
