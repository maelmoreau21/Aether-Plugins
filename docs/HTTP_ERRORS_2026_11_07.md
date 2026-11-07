# Client HTTP des fournisseurs — classification des erreurs (7 novembre 2026)

Portée : SDK (`aether-plugin-api::http`), TMDb et TVDB. Les autres fournisseurs ne sont pas
modifiés ; leur état est listé en fin de document.

## Contrat d'erreurs

Le Core ne publie que le **code** (`pluginError`, `extensions/adapters.rs::validate_result`) ;
les messages sont écartés. Les codes sont donc le contrat. Un seul appel HTTP par opération du
SDK : **aucun retry**, ni pour un `GET` ni pour la connexion TVDB (`POST /login`).

| Situation | Code | `retry_after_secs` |
|---|---|---|
| clé absente / invalide (401) | `missing_api_key` / `unauthorized` | — |
| accès refusé malgré une clé valide (403) | `forbidden` (nouveau, était `unauthorized`/`provider_error`) | — |
| média absent (404, 410) | `not_found` (TMDb renvoyait `provider_error`) | — |
| quota fournisseur (429) | `rate_limited` | valeur de `Retry-After` en secondes, sinon 10 |
| panne fournisseur (502-504) | `provider_unavailable` (nouveau, était `rate_limited`/`provider_error`) | `Retry-After` si annoncé |
| délai dépassé (408, silence, DNS) | `timeout` (était `network_error`) | — |
| connexion coupée / JSON incomplet | `truncated_response` (nouveau) | — |
| réponse non JSON | `parse_error` / `invalid_provider_response` (TVDB) | — |
| réponse > 256 Kio | `response_too_large` (TVDB : `provider_response_too_large`) | — |
| redirection | `provider_redirect_refused` | — |
| autre statut | `provider_error` | — |
| refus du courtier Core | code du courtier conservé (`quota_exceeded`, `domain_not_allowed`…) ; `dns_timeout`→`timeout`, `provider_rate_limited`→`rate_limited` | — |

Pourquoi aucune attente ni retry dans le SDK : un invité Wasm n'a ni horloge ni minuterie, et le
Core transforme déjà un 429/503 en délai durable (`provider_retry`, `Retry-After` en secondes ou
date HTTP, borné à 24 h) et interrompt l'invocation. Le SDK relaie donc `Retry-After` quand il est
exprimé en secondes ; une date HTTP est laissée au Core (`retry_after_secs` absent).

## Bugs prouvés (tests en échec avant correction, TMDb)

1. 404 → `provider_error` (jamais `not_found`), 403 → `provider_error`, 503 → `provider_error`.
2. `Retry-After` ignoré ; `rate_limited(5)` inventé.
3. Délai dépassé indiscernable d'une panne réseau.
4. Un identifiant IMDb **vide** masquait un identifiant TVDB valide (`invalid_external_id`).
5. `find` renvoyant plusieurs œuvres : la première était choisie sans avertissement ; désormais
   `ambiguous_match`.
6. Natif : le message d'erreur réseau reprenait l'URL de la requête, clé `api_key` comprise.

Corrigés de la même façon côté TVDB (tests écrits avec la correction, car la base de départ
n'avait aucun moyen d'être pointée vers un faux serveur) : le chemin Wasm remplaçait tout refus du
courtier (`quota_exceeded`, `response_too_large`…) par `network_error` ; un identifiant vide
masquait l'autre et forçait une recherche « identifiée » ; l'échec d'une langue supplémentaire
effaçait la fiche déjà lue (seuls identifiants/quotas sont désormais fatals pour ces langues).

## Autres changements du SDK

- `get_json` natif : pas de redirection (le courtier Core les refuse ; les suivre renverrait les
  en-têtes d'authentification), corps plafonné à 256 Kio comme le courtier, erreurs sans URL.
- `status_error` conserve sa signature ; `classify_status`, `host_error`, `retry_after_secs`,
  `parse_body`, `json_error` s'y ajoutent. `API_VERSION` et `LOCALIZATION_CONTRACT` inchangés :
  seuls des codes d'erreur sont ajoutés (le Core les traite comme opaques).
- TVDB : `base_url` n'est lu que hors Wasm (tests, CLI) ; le module Wasm ne joint toujours que
  `api4.thetvdb.com`. Aucune permission ni `httpHosts` modifiés.

## Limites connues (non traitées ici)

- Le courtier Core appelle `reqwest::Error::to_string()` : son message inclut l'URL, donc la clé
  v3 de TMDb / OMDb / fanart.tv. Le SDK le supprime, mais le courtier (`runtime.rs`, `network_error`)
  devrait utiliser `without_url()` et distinguer `is_timeout()` (aujourd'hui seul `dns_timeout`
  est reconnu comme délai côté Wasm).
- Aucune requête n'a été faite vers un service réel : quotas, conditions d'usage et capacités
  linguistiques ont été relus dans le code et la documentation du dépôt, pas vérifiés en ligne.
- TMDb : recherche limitée à la première page ; `Sans titre` est un libellé de remplacement français
  pour un résultat sans titre. TVDB : saisons et épisodes non implémentés.

## Taille Wasm (limite du runtime : 262 144 octets)

TMDb optimisé : environ 257,5 Ko avant (estimé d'après la taille brute 310 820), **259 777** après —
marge de 2,3 Ko seulement. Un nouvel ajout au chemin HTTP de TMDb doit s'accompagner d'une
réduction équivalente ; TVDB : 253 671.

## Autres plugins, par priorité

1. **fanart.tv** — même classe de bug que TMDb : gestion de statuts dupliquée (403/503 en
   `provider_error`, `rate_limited(10)` fixe), clé dans l'URL, message natif avec URL. À faire
   passer par `classify_status`/`host_error`.
2. **OpenSubtitles** — `401|403 → unauthorized`, `429|503 → rate_limited` en dur ; aligner sur
   `classify_status`. Le téléchargement consomme un quota : ne jamais retenter.
3. **IMDb** — lit des pages HTML d'imdb.com avec sa propre gestion d'erreurs (403 →
   `provider_error`). Vérifier les conditions d'usage d'IMDb sur l'extraction de pages avant toute
   évolution.
4. **MusicBrainz** — signale la limite de débit par 503 ; désormais `provider_unavailable` avec
   `retry_after_secs` au lieu de `rate_limited(5)` : confirmer que l'appelant traite les deux comme
   une attente. `throttle` n'agit qu'en natif.
5. **OMDb** — erreurs lues dans le JSON (`Request limit reached!` → `rate_limited(3600)` fixe,
   quota journalier à confirmer) ; profite déjà des corrections via `get_json`.
6. **Discogs, LRCLIB** — passent par `get_json` (profitent des corrections) ; pagination et quotas
   non audités.
