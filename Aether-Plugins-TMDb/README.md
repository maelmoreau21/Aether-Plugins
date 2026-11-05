# Extension Aether : TMDb (The Movie Database)

Extension Rust officielle fournissant les métadonnées et affiches depuis l'API v3/v4 de The Movie Database.

## Fonctionnalités
- Recherche de films et de séries TV avec filtrage par année et langue (`fr-FR`, `en-US`, etc.).
- Fiche détaillée complète : synopsis, date de sortie, durée, classification, genres.
- Distribution : 20 premiers acteurs ordonnés, personnages, identifiants TMDb et photos disponibles.
- Équipe technique : réalisateurs et scénaristes, identifiants TMDb et photos disponibles.
- Affiches et arrière-plans (résolutions originale et W500) avec langue et notes.
- Identifiants externes : mapping automatique vers IMDb ID (`tt...`) et TVDB ID.

## Configuration
- `api_key` (obligatoire) : Clé API v3 TMDb ou jeton Bearer v4 (peut aussi être définie via la variable d'environnement `TMDB_API_KEY`).
- `language` (optionnel, défaut `fr-FR`) : Langue demandée aux endpoints TMDb.
- `include_adult` (optionnel, défaut `false`) : Inclusion des contenus adultes.

## Exécution
```powershell
# Manifeste
./target/release/aether-plugin-tmdb.exe --manifest

# Recherche
./target/release/aether-plugin-tmdb.exe search --title "Dune" --year 2021 --api-key "VOTRE_CLE"

# Fiche détaillée (ID TMDb)
./target/release/aether-plugin-tmdb.exe details --id 438631 --api-key "VOTRE_CLE"
```


### Collections (développement, 2 novembre 2026)

Les détails de film transmettent `belongs_to_collection` sous la forme
`collection: {id: "tmdb:<id>", name: "..."}`. Core peut en déduire les collections
accessibles lorsque l'option de bibliothèque est activée. Aucun appel à l'API
collection n'est nécessaire ; les films absents du catalogue ne sont pas ajoutés.

Le workspace épingle le SDK publié dans le commit Core
`133896830c609a177e4c4c611084154d7e4aab5d`, qui contient ce champ. Le test de
parsing ciblé fonctionne sans remplacement local de dépendance :

```powershell
cargo test -p aether-plugin-tmdb --test tmdb_tests test_tmdb_details_mock_server
```

Cette fixture HTTP locale ne valide ni un compte TMDb réel ni le module signé
installé sur un NAS.

### Personnes (0.1.4)

La réponse `metadata.details` ajoute `providerId` (`tmdb:<id>`) aux acteurs
identifiés et `crewCredits` pour les réalisateurs et scénaristes, au maximum 100.
Chaque crédit d'équipe fournit `name`, `role` (`Director`, `Writer` ou
`Screenplay`), `providerId` et `thumbUrl` lorsque TMDb les connaît. Les portraits
utilisent exclusivement `image.tmdb.org` ; une photo absente reste absente.
Les listes `directors` et `writers` restent présentes pour les clients existants.
Les métadonnées déjà enregistrées doivent être actualisées pour recevoir ces champs.
