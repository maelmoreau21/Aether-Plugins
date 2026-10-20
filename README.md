# Extensions Aether — Fournisseurs de métadonnées officiels

Ce dépôt regroupe les premières extensions Rust de fournisseurs de métadonnées pour la plateforme multimédia **Aether** :
- **TMDb (The Movie Database)** : métadonnées complètes, casting détaillé, synopsis, notes et affiches.
- **IMDb (Internet Movie Database)** : recherche instantanée, notes de référence mondiales, distribution et posters.
- **fanart.tv** : illustrations et images haute définition (ClearLogos, Backdrops 4K/HD, ClearArt, Bannières, Disques).

---

## 1. Architecture Core → Extensions

Le système de métadonnées d'Aether suit une séparation stricte des responsabilités :

```
┌─────────────────────────────────────────────────────────────────┐
│                          Aether Core                            │
│                                                                 │
│  - Décide quand rechercher ou actualiser les métadonnées        │
│  - Identifie les œuvres (titre, année, identifiants techniques) │
│  - Gère l'ordre de priorité des fournisseurs                    │
│  - Fusionne les données (titre, synopsis, notes, images)        │
│  - Valide et assainit (limites de taille, anti-XSS)             │
│  - Met en cache et persiste en base (inventory_works, etc.)     │
│  - Respecte les règles de conservation des métadonnées          │
└────────────────────────────────┬────────────────────────────────┘
                                 │
           API commune versionnée (JSON IPC / CLI / Subprocess)
                                 │
       ┌─────────────────────────┼─────────────────────────┐
       ▼                         ▼                         ▼
┌──────────────┐          ┌──────────────┐          ┌──────────────┐
│  TMDb Plugin │          │  IMDb Plugin │          │fanart.tv Plg │
└──────┬───────┘          └──────┬───────┘          └──────┬───────┘
       │                         │                         │
       ▼                         ▼                         ▼
  api.themoviedb.org         imdb.com             webservice.fanart.tv
```

### Règles architecturales non négociables
1. **Initiative du Core** : Aether décide souverainement quand scanner, chercher ou rafraîchir.
2. **Appel via API versionnée** : Aether appelle les extensions installées et activées via l'enveloppe commune `apiVersion: 1`.
3. **Données structurées** : Chaque extension interroge son fournisseur externe via HTTPS et renvoie des données standardisées (`StructuredMetadata`).
4. **Pas d'accès direct** : Les extensions **ne touchent jamais à la base de données SQLite/PostgreSQL** et **ne modifient jamais les fichiers médias**.
5. **Indépendance et mise à jour à chaud** : Les extensions sont indépendantes du Core. Elles peuvent être installées, configurées ou mises à jour sans recompiler ni redémarrer Aether.

---

## 2. API Commune Versionnée (`aether-plugin-api`)

Le crate commun `crates/aether-plugin-api` (v1) définit le contrat :

### Enveloppe de requête (`PluginRequest`)
```json
{
  "apiVersion": 1,
  "operation": "search | details | images | manifest",
  "input": { ... },
  "config": {
    "apiKey": "...",
    "language": "fr-FR",
    "timeoutSecs": 10
  }
}
```

### Enveloppe de réponse (`PluginResponse<T>`)
```json
{
  "apiVersion": 1,
  "success": true,
  "data": { ... },
  "error": null
}
```

### Modèle de métadonnées structurées (`StructuredMetadata`)
- **Identifiants** : `provider`, `providerId`, `externalIds` (tmdb, imdb, tvdb)
- **Titres & Résumé** : `title`, `originalTitle`, `tagline`, `synopsis`
- **Chronologie & Format** : `year`, `releaseDate`, `runtimeMinutes`, `status`, `mpaa`
- **Distribution** :
  - `genres`: liste de genres textuels
  - `directors`: réalisateurs
  - `writers`: scénaristes
  - `actors`: `ActorCredit { name, role, thumbUrl, order }`
- **Notes** : `ProviderRating { provider, value, max, votes }`
- **Affiches et Images** : `MediaImage { url, imageType, width, height, language, rating, isPrimary }`
  - Types supportés : `Poster`, `Backdrop`, `Banner`, `Logo`, `ClearArt`, `Thumb`, `DiscArt`.

---

## 3. Les Extensions

### TMDb (`Aether-Plugins-TMDb`)
- **ID** : `tmdb`
- **Priorité par défaut** : 100
- **Fonctionnalités** : Recherche films/séries, fiche complète avec crédits (acteurs, réalisateurs, scénaristes), affiches officielles et bannières.
- **Authentification** : Clé API v3 ou jeton v4 Bearer (via `config.apiKey` ou variable d'environnement `TMDB_API_KEY`).

### IMDb (`Aether-Plugins-IMDb`)
- **ID** : `imdb`
- **Priorité par défaut** : 80
- **Fonctionnalités** : Recherche auto-complétée instantanée via endpoint dédié, parsing Schema.org JSON-LD (note de référence mondiale, nombre de votants, durée ISO, casting principal, classification d'âge).
- **Authentification** : Fonctionne sans clé API via les endpoints publics IMDb ; supporte une clé API optionnelle pour les proxies/RapidAPI.

### fanart.tv (`Aether-Plugins-fanart.tv`)
- **ID** : `fanart-tv`
- **Priorité par défaut** : 60
- **Fonctionnalités** : Spécialiste des visuels haute fidélité (ClearLogos transparents, fonds d'écran 4K/HD, ClearArt, jaquettes de disques, bannières).
- **Authentification** : Clé API projet requise (via `config.apiKey` ou `FANART_TV_API_KEY`), clé VIP optionnelle (`config.userKey`).

---

## 4. Compilation et Tests

Les extensions sont écrites en Rust édition 2024, ultra-légères, sans dépendances superflues, compilées avec LTO et symboles dénudés (`strip = true`).

```powershell
# Vérification globale
cargo check --all-targets

# Lancement des tests unitaires et mocks
cargo test

# Vérification du formatage et clippy strict
cargo fmt --check
cargo clippy --all-targets -- -D warnings

# Compilation release optimisée (binaires légers ~3.8 Mo)
cargo build --release
```

Les exécutables générés se trouvent dans `target/release/` :
- `aether-plugin-tmdb.exe`
- `aether-plugin-imdb.exe`
- `aether-plugin-fanart.exe`

---

## 5. Utilisation en CLI / IPC

Chaque extension peut être exécutée de manière autonome ou appelée par un processus superviseur :

### Afficher le manifeste du plugin
```powershell
./target/release/aether-plugin-tmdb.exe --manifest
```

### Exécution IPC via STDIN / STDOUT (pour Aether Core)
```powershell
'{"apiVersion": 1, "operation": "search", "input": {"title": "Inception", "year": 2010, "mediaType": "movie"}, "config": {}}' | ./target/release/aether-plugin-imdb.exe --run
```

### Recherche interactive en ligne de commande
```powershell
# Recherche IMDb
./target/release/aether-plugin-imdb.exe search --title "Interstellar" --year 2014

# Recherche TMDb (nécessite clé API)
./target/release/aether-plugin-tmdb.exe search --title "Dune" --year 2021 --api-key "VOTRE_CLE"
```
