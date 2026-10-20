# Extension Aether : fanart.tv

Extension Rust spécialisée dans l'acquisition d'images et d'illustrations communautaires haute définition pour les médias d'Aether.

## Fonctionnalités
- Récupération d'images par identifiants externes (TMDb ID, IMDb ID pour les films ; TVDB ID pour les séries).
- Extraction typée de toutes les catégories d'illustrations :
  - **ClearLogos / Logos** : titres de films et séries avec fond transparent (HD / UHD).
  - **Backdrops / Fanart** : fonds d'écran de haute qualité sans texte.
  - **Posters** : affiches alternatives haute résolution avec langue et votes.
  - **ClearArt** : visuels de personnages et éléments détourés sans logo.
  - **Bannières & Vignettes** : bannières horizontales et thumbs.
  - **Disques** : illustrations rondes des disques (Blu-ray / DVD).

## Configuration
- `api_key` (obligatoire) : Clé client de projet fanart.tv (peut aussi être définie via `FANART_TV_API_KEY`).
- `user_key` (optionnel) : Clé VIP personnelle pour les nouveautés sous embargo temporaire.
- `base_url` (optionnel) : URL personnalisée pour proxy ou mock.

## Exécution
```powershell
# Manifeste
./target/release/aether-plugin-fanart.exe --manifest

# Fiche et illustrations pour un film (par ID TMDb ou IMDb)
./target/release/aether-plugin-fanart.exe details --id 27205 --api-key "VOTRE_CLE"

# Fiche et illustrations pour une série (par ID TVDB)
./target/release/aether-plugin-fanart.exe details --id 81189 --media-type tv --api-key "VOTRE_CLE"
```
