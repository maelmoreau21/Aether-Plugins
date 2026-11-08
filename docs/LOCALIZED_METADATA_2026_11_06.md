# Métadonnées localisées — TMDb, TVDB et contrat SDK (6 novembre 2026)

Pendant côté fournisseurs de `Aether/docs/METADATA_LOCALIZATION.md` : le Core stocke
les titres, résumés et slogans par langue et par source ; les fournisseurs doivent
donc livrer plusieurs langues sans multiplier les appels.

## Constat sur TMDb (avant correction)

`details` demandait `append_to_response=credits,external_ids`, mais le code lisait
aussi `images` et `translations` dans cette réponse. Ces deux blocs n'existent que
s'ils sont demandés : les boucles ne s'exécutaient jamais. Le test existant ne le voyait
pas, car son faux serveur renvoyait `images` sans qu'on l'ait demandé. Trois autres
défauts, vérifiés dans le code :

- les appels de saison utilisaient `request.id` (vide quand l'identification passe par
  un identifiant IMDb/TVDB) : `tv//season/1`, erreur silencieusement ignorée, donc
  **aucun épisode** ;
- une requête par saison sans plafond, alors que le Core autorise **10 requêtes HTTP par
  invocation** (`quota_exceeded` au-delà), espacées d'**au moins une seconde** ;
- l'affiche issue de `poster_path` était étiquetée avec la langue de la requête, alors
  que TMDb la choisit parmi la langue demandée, la langue originale puis la mieux notée.

Limites d'exécution du Core qui dimensionnent le flux (`extensions/runtime.rs`, SDK) :
10 requêtes HTTP, 256 Kio par réponse, **64 Kio de sortie JSON** par invocation.

## Capacités vérifiées

| Besoin | TMDb (documentation officielle) | TVDB v4 (swagger officiel) |
|---|---|---|
| Plusieurs langues du titre/résumé/slogan | **une** requête `…/translations` : toutes les langues, entrées distinctes par langue **et pays** (`fr`/FR, `fr`/CA) | **une requête par langue** `…/translations/{lang}` (codes à 3 lettres) ; `extended?meta=translations` existe |
| Saisons, épisodes | `tv/{id}/season/{n}` accepte `language` ; `…/season/{n}/translations` existe ; l'endpoint de traductions d'épisode existe aussi | `seasons/{id}/translations/{lang}`, `episodes/{id}/translations/{lang}`, `series/{id}/episodes/{type}/{lang}` |
| `append_to_response` | oui (films, séries, saisons, épisodes), 20 éléments au plus ; non retenu pour les traductions (voir ci-dessous) | sans objet |
| Images localisées | `images` avec `include_image_language` (ex. `fr,en,null`) ; **langue seule, pas de région** | `language` sur chaque illustration |

Non établi par la documentation consultée : les sous-ressources autorisées pour
`append_to_response` sur une saison ; l'existence de régions côté TVDB. Aucun appel à
un service réel n'a été fait (voir « Validation »).

## Flux TMDb après correction

| Cas | Requêtes |
|---|---|
| Film, une seule langue | 1 (`credits,external_ids`) — comme avant |
| Film, autres langues demandées | 2 (+ `…/translations`) |
| Série | 1 + 1 si langues + 1 par saison **dans la limite de 10** |
| Identifiant connu par IMDb/TVDB seulement | +1 (`find`) |

- Les traductions ne sont pas ajoutées via `append_to_response` : avec une grande
  distribution, la réponse risquerait de dépasser les 256 Kio, et l'échec emporterait
  les détails de la langue principale. Une panne de cette requête n'efface donc pas les
  détails (seules les erreurs d'authentification et de quota sont remontées).
- Les saisons sont lues dans l'ordre (spéciaux en dernier) avec l'identifiant résolu.
  Le budget restant achète les mêmes saisons dans deux autres langues au plus ;
  sur une longue série la couverture est partielle, et le lecteur complète par repli
  champ par champ. Au-delà de 10 requêtes, les saisons restent listées sans épisodes.
- Les images ne sont plus lues dans `details` : l'opération `images` (budget propre)
  transmet `include_image_language` quand une politique de langues existe, sinon la liste
  complète est conservée comme avant.
- La sortie est ramenée sous 60 Kio : textes d'épisodes dans les autres langues d'abord,
  puis résumés d'épisodes tronqués à 200 caractères, puis épisodes des dernières saisons.
  Identifiants, titres et crédits ne sont jamais retirés.

## TVDB

`DetailsRequest.languages` ajoute au plus trois langues (une requête chacune, après la
connexion, la résolution et l'enregistrement ; avec la langue principale et son repli, au plus 8 requêtes). Les clés restent
les codes à trois lettres du fournisseur (`fra`) que le Core ramène à `fr`. Aucune
région n'est inventée. Traductions d'épisodes TVDB : non implémentées.

## Contrat SDK (`LOCALIZATION_CONTRACT = 2`)

Ajouts compatibles, `API_VERSION` inchangé : `DetailsRequest.languages`,
`StructuredMetadata.locale`, `SeasonMetadata.translations`, `EpisodeMetadata.translations`,
et `aether_plugin_api::canonical_locale`. Un plugin du premier contrat ignore `languages`
et répond dans une seule langue ; un Core du premier contrat ignore les nouveaux champs.
Rupture **visible** : un littéral `DetailsRequest { … }`, `StructuredMetadata { … }`
ou `EpisodeMetadata { … }` ne compile plus sans `..Default::default()` (corrigé ici dans
IMDb, OMDb, fanart.tv, Discogs, LrcLib, MusicBrainz et les tests).

Note : la clé `languages` désigne la liste de langues d'images dans l'opération
`metadata.images` et la liste de traductions dans `metadata.details`. Les deux sens ne
se mélangent pas ; l'opération détermine l'usage.

## Dépendance et ordre de livraison

1. Pull request Aether (`feat/metadata-translations`) : tables, résolution par compte, SDK.
2. Cette pull request : `Cargo.toml` pointe le SDK sur `49ecd2ed…`, le commit de la branche
   Aether. **Après fusion (surtout par squash), remplacer `rev` et les trois occurrences
   dans `Cargo.lock` par le commit de fusion**, sinon la CI ne résout pas la dépendance.
3. Versions : TMDb 0.1.5 et TVDB 0.1.2 (« release pending » jusqu'à la publication signée).
   Les autres plugins ne changent que par `..Default::default()` : aucune version n'est
   bumpée ici, le planificateur de publication décidera.

## Validation

Docker (Rust 1.99.1, Node 22), faux serveurs fidèles à la requête (ils ne renvoient que
ce qui est demandé). `cargo test --workspace`, `clippy -D warnings`, `fmt`, modules Wasm
optimisés puis `test-wasm`, `test-metadata-wasm`, `test-tvdb-wasm`, `test-manifests
--allow-unpublished`, `test-repository`, catalogue et plan de release : réussis.
Module TMDb optimisé : **256 887 octets pour 262 144 autorisés** (marge de 2 %) ; TVDB
250 812. Toute addition à TMDb doit être mesurée avant signature.

Non validé : appels à TMDb/TVDB réels (clés, licence), paquets signés, instance Aether
déployée, PostgreSQL du côté des plugins (sans objet), chronométrage réel de la pause
d'une seconde entre requêtes.
