# Identification des métadonnées — correctifs du 2 novembre 2026

Les sources TMDb, IMDb, OMDb, fanart.tv, Discogs, MusicBrainz et LRCLIB sont en
version **0.1.1**. Les paquets de
préparation locale sont sous `artifacts/2026-11-02-repair/<provider>/`.
Le catalogue public signé `repository/` reste inchangé : il n'annonce pas une
publication qui n'a pas eu lieu. Les paquets peuvent servir à une nouvelle
installation. Pour une extension déjà installée, les routes de mise à jour
refusent actuellement toute bascule avec
`extension_update_compatibility_unverified` : la migration et le retour arrière
restent à réaliser (voir `Aether/docs/PLUGIN_UPDATE_LIFECYCLE.md`). Une
désinstallation n'est pas un moyen de contourner cette limite et ne constitue
pas une sauvegarde des paramètres ou des données privées.

## Comportement corrigé

- TMDb résout d'abord les IDs TMDb, IMDb ou TVDB transmis par le Core ; IMDb
  et OMDb résolvent directement l'ID IMDb. Un titre local traduit ou une année
  incorrecte ne remplacent pas une identité connue.
- TMDb réémet l'ID TMDb résolu dans `providerId` et `externalIds.tmdb`, y compris
  après conversion d'un ID IMDb. Les affiches utilisent la même résolution.
- Les erreurs d'authentification et de résolution d'ID ne sont plus masquées
  par une tentative de détail sur un identifiant invalide.
- Sans ID, TMDb conserve sa recherche titre/année avec nouvelle tentative sans
  année si la première ne retourne aucun résultat.
- Les options personnalisées du formulaire sont conservées dans `config.extra`.
  Les champs `timeout_secs`, `fallback_languages` et `include_neutral` sont
  convertis vers le contrat du SDK. Un délai reçu comme chaîne est accepté et
  borné entre 1 et 120 secondes ; un délai invalide n'efface pas la clé API.

## Paramètres effectivement utilisés

| Fournisseur | Paramètres |
| --- | --- |
| TMDb | Clé API v3 / Bearer v4, URL, langue, délai HTTP, recherche adulte |
| IMDb | Clé de proxy facultative, URL, délai HTTP |
| OMDb | Clé API, URL, délai HTTP |
| fanart.tv | Clé application, clé personnelle facultative, URL, délai HTTP |
| Discogs | Jeton API, URL, délai HTTP |
| MusicBrainz | URL, délai HTTP |
| LRCLIB | URL, délai HTTP |

La clé TMDb reste nécessaire au runtime ; le Core peut fournir sa propre clé
par défaut. Le plugin ne prétend plus posséder une clé communautaire intégrée.
La clé personnelle fanart.tv correspond au réglage `PersonalApiKey` du plugin
Jellyfin : elle complète la clé application et passe dans l'en-tête `user-key`.
Les délais Discogs/MusicBrainz/LRCLIB étaient déjà consommés par le client HTTP
partagé ; ce jalon les expose dans le schéma signé. Ce complément de paramètres
ne constitue pas une validation de tous les parcours de ces fournisseurs.

## Référence Jellyfin examinée

`MediaBrowser.Providers/Plugins/Tmdb/Movies/TmdbMovieProvider.cs` effectue
successivement la résolution TMDb, IMDb/TVDB puis la recherche par titre.
`Configuration/PluginConfiguration.cs` expose notamment la recherche adulte.
Cette priorité d'identité a été reprise dans le contrat de recherche Aether,
sans copier les médias ni modifier les sources Jellyfin.

## Validation reproductible

```powershell
cargo test -p aether-plugin-api -p aether-plugin-tmdb -p aether-plugin-imdb -p aether-plugin-omdb --tests
cargo test -p aether-plugin-fanart -p aether-plugin-discogs -p aether-plugin-musicbrainz -p aether-plugin-lrclib --tests
node scripts/test-wasm.cjs target/wasm32-unknown-unknown/release aether-plugin-tmdb aether-plugin-imdb aether-plugin-omdb aether-plugin-fanart aether-plugin-discogs aether-plugin-musicbrainz aether-plugin-lrclib
node scripts/test-metadata-wasm.cjs
node scripts/test-manifests.cjs --allow-unpublished
node scripts/test-repository.cjs
```

Les fixtures HTTP locales et le broker simulé valident les requêtes, les
réponses et l'exécution réelle des guests Wasm. Ils ne prouvent pas la
disponibilité actuelle des services externes, la validité d'une clé déployée
ni le fonctionnement sur le NAS de l'utilisateur.

Le contrôle `test-manifests.cjs` sans option reste strict sur `main` et dans les
workflows de préparation/publication. La CI de développement et des PR utilise
explicitement `--allow-unpublished` : elle accepte les sources plus récentes et
signale leur publication en attente. Le contrôle des signatures et de
l'intégrité du catalogue reste strict partout. L'option ne remplace pas une
publication du catalogue. La CI exécute également les fixtures des guests Wasm.
