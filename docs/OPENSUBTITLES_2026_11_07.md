# OpenSubtitles — fiabilité de la recherche et des erreurs, 7 novembre 2026

Portée : `Aether-Plugins-OpenSubtitles` uniquement. Aucune modification du Core, du SDK, des
permissions (`subtitles`), de l'ABI Wasm v1 ni des `httpHosts`. Les sorties existantes
(`id`, `language`, `label`, `forced`, `sdh`, `match`, `downloadCount`) sont inchangées ; le Core
retire les champs ajoutés (`hashMatch`, `machineTranslated`, `aiTranslated`) tant qu'il ne les lit pas.

## Audit : écarts constatés

| # | Constat (code d'origine) | Effet |
|---|---|---|
| 1 | Aucun paramètre `moviehash` | Pas de correspondance par fichier ; le README promettait seulement de ne pas en inventer |
| 2 | `externalIds.imdb = ""` → `invalid_external_id` même avec un TMDb valide | Une recherche identifiable échoue |
| 3 | Épisode : seuls les IDs de l'épisode ; titre de série sans saison/épisode accepté | Résultats de toute la série |
| 4 | Année de la série envoyée pour un épisode | Exclut les autres saisons |
| 5 | Langue : minuscules et `-` seulement ; `pt-BR`, `pt_br`, `es-419` refusés ou mal filtrés | Variantes régionales inutilisables |
| 6 | Indicateurs forcé/SDH transmis au fournisseur sans contrôle de la réponse | Une piste forcée pouvait être proposée comme complète |
| 7 | Un `file_id` répété ou une langue `es-419` dans la réponse | Le Core refuse **toute** la page (`invalid_subtitle_output`) |
| 8 | `release` composé de caractères de contrôle → étiquette vide | Idem : page entière refusée |
| 9 | 404, 400/422, 408, 502/504, login 401 → `provider_error`/`unauthorized`; refus du courtier → `network_error` | Causes indiscernables |
| 10 | Corps coupé ou vide → `invalid_provider_response`; silence → `network_error` | Idem |

## Changements

- `movieHash` (16 chiffres hexadécimaux) → `moviehash`, `requireHash` → `moviehash_match=only`. L'empreinte
  interne d'Aether (`b3s1:`/`b3f1:`) est refusée (`invalid_movie_hash`), jamais transmise.
- Ordre des critères : hash, IMDb, TMDb, identifiants de série (`parent_imdb_id`/`parent_tmdb_id`), titre.
  Identifiants vides ignorés, mal formés refusés. Épisode sans identifiant ni hash : saison et épisode obligatoires.
- Langues normalisées (casse, `_`), un sous-tag de région/script ; filtrage de la réponse (`pt-br` ≠ `pt-pt`,
  `pt` accepte les deux) ; région numérique réduite à la langue principale pour rester acceptée par le Core.
- Forcé/SDH/complet contrôlés aussi sur la réponse ; doublons supprimés ; étiquette jamais vide ;
  fichiers dont le hash correspond classés en premier.
- Codes d'erreur documentés dans le README (même vocabulaire que le SDK). Aucun retry, aucun délai dans l'invité.
- Téléchargement : hôte strict inchangé, espaces et `\` refusés ; quota (`remaining`, `reset_time_utc`) relayé tel quel.

## Corpus et résultats (exécutés le 7 novembre 2026, Docker `rust:slim-bookworm`, rustc 1.99.1)

Corpus : `tests/corpus/search_hash_and_duplicates.json` (8 entrées : hash, doublon, autre langue, sans
fichier, sans langue, identifiants invalides, étiquette vide) et `search_language_variants.json`
(`pt-BR`, `pt-PT`, `pt`, `pt_BR`, `es-419`, `zh-CN`, forcé, SDH, traduit automatiquement).

| Contrôle | Résultat |
|---|---|
| `cargo test --locked -p aether-plugin-opensubtitles` | 14 tests unitaires + 4 tests à faux serveur : 18/18 réussis |
| `cargo clippy --locked -p aether-plugin-opensubtitles --all-targets -- -D warnings`, `cargo fmt --check` | réussis |
| Build release Wasm (`--lib`) puis Binaryen | 243 122 → 203 266 octets (limite 262 144) |
| `node scripts/test-subtitles-wasm.cjs` (vrai invité, courtier synthétique) | réussi : hash, série, variante `pt-BR`, login 401, 11 statuts |
| `node scripts/test-manifests.cjs --allow-unpublished` | réussi |

Faux serveur : une seule requête par appel pour chaque statut testé (401, 403, 404, 400, 406, 429, 502, 500, 302),
corps HTML, vide, coupé en plein JSON, coupé avant `Content-Length`, > 256 Kio, silence (timeout < 5 s) ; la clé API
n'apparaît dans aucune sortie ; l'URL de recherche est triée (aucune redirection de canonicalisation).

## Ce qui n'est pas établi ni fait

- **Aucun appel réel** à OpenSubtitles : les noms et valeurs de paramètres (`moviehash`, `moviehash_match`,
  `parent_*_id`, `hearing_impaired`, `foreign_parts_only`) viennent de la documentation REST consultée
  précédemment ; la documentation en ligne n'a pas pu être relue pendant ce lot. Le comportement de
  `moviehash` combiné à un identifiant (filtre ou simple tri) reste à vérifier avec une clé réelle ; le plugin
  s'appuie sur `hashMatch` et ne suppose pas un filtrage.
- **Le plugin ne calcule pas le hash** : il n'a pas accès au fichier. Tant que le Core ne fournit pas `movieHash`,
  le comportement est celui d'avant (`subtitle_require_hash` continue de bloquer côté Core).
- Le Core valide `match` ∈ {identity, title} : `hashMatch` est un champ distinct pour ne pas casser ce contrat.
- Pas de limite par compte conservée entre invocations (le login est refait à chaque appel, sans stockage privé).
- Le 503 reste `rate_limited` (contrat existant) ; le SDK classe 502-504 en `provider_unavailable`.

## Suite côté Aether (hors de ce commit, PR Core séparée)

1. Calculer le hash OpenSubtitles sur le Node (lecture seule, 2 × 64 Kio) et le transmettre ; lever
   `subtitle_require_hash` seulement lorsqu'il existe ; ne jamais le déduire de l'empreinte `b3s1`.
2. Accepter `hashMatch` dans `validate_result` ; passer `parentExternalIds` des épisodes.
3. `to_webvtt` : une seule cue vide ou illisible fait échouer tout le fichier après consommation du quota ;
   seul l'UTF-8 est accepté (UTF-16 et ISO-8859 sont refusés, `subtitle_encoding_unsupported`) — à trancher
   avec un corpus de fichiers (BOM, UTF-16, CRLF, cues corrompues, archives).
4. Préférences utilisateur et pistes existantes : non touchées ici ; les dérivés téléchargés vivent en base,
   jamais dans les dossiers médias.

Rollback : revert du commit ; aucune migration, aucune écriture de média.
