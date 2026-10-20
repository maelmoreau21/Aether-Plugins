# Extension Aether : IMDb (Internet Movie Database)

Extension Rust fournissant l'accès à la base de données IMDb, aux notes critiques de référence et aux affiches officielles.

## Fonctionnalités
- Recherche ultra-rapide par auto-complétion sans clé API via l'API de suggestion IMDb.
- Fiche détaillée complète parsée à partir du standard Schema.org JSON-LD officiel.
- Récupération de la note de référence mondiale IMDb (ex: 8.8/10) et du volume de votes (ex: 2 500 000).
- Durée normalisée en minutes depuis le format ISO 8601 (`PT2H28M` -> 148 min).
- Réalisateurs, scénaristes, acteurs principaux et classification d'âge (MPAA / PG-13).

## Configuration
- `api_key` (optionnel) : Clé optionnelle si utilisation d'un proxy ou service tiers.
- `base_url` (optionnel) : URL de substitution pour proxy ou tests.
- `language` (optionnel, défaut `fr-FR`) : Langue transmise via l'en-tête `Accept-Language`.

## Exécution
```powershell
# Manifeste
./target/release/aether-plugin-imdb.exe --manifest

# Recherche
./target/release/aether-plugin-imdb.exe search --title "Interstellar" --year 2014

# Fiche détaillée par ID IMDb (tt...)
./target/release/aether-plugin-imdb.exe details --id tt0816692
```
