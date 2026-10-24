# Aether Plugin — Intro Skipper

Portage officiel, autonome et hautement optimisé en **Rust** du célèbre plugin [intro-skipper/intro-skipper](https://github.com/intro-skipper/intro-skipper).

Intro Skipper permet de détecter automatiquement les **génériques d'ouverture (intros)** et les **génériques de fin (crédits / outro)** dans les séries télévisées et animes pour proposer le saut d'introduction ("Skip Intro") instantané et optionnellement automatique.

---

## ⚡ Caractéristiques & Optimisations

1. **Zéro dépendance native lourde à l'exécution** :
   - Algorithme d'alignement audio Chromaprint pur Rust, sans allocations superflues.
   - Calcul de distance de Hamming vectorisé avec les instructions CPU `POPCNT` (`count_ones()`).
   - Analyse par diagonales de corrélation croisée (Diagonal Cross-Correlation Matrix) avec sous-échantillonnage adaptatif (accélération 4x à 9x).
2. **Consensus multi-épisodes (Season Consensus)** :
   - Détection des décalages d'intro (Cold Opens / résumés de début d'épisode).
   - Validation croisée entre plusieurs épisodes d'une même saison pour éliminer 100% des faux positifs.
3. **Double moteur d'analyse** :
   - **Détection acoustique (Chromaprint / FPcalc)** : identifie les signatures musicales répétées entre épisodes.
   - **Détection sémantique par chapitres** : extraction immédiate (0ms CPU) si le conteneur MKV/MP4 contient déjà des chapitres nommés multilingues (`Intro`, `Opening`, `OP`, `Générique`, `Credits`, `ED`, etc.).
4. **Détection des crédits & génériques de fin** :
   - Analyse de la queue d'épisode pour proposer la transition fluide vers l'épisode suivant.
5. **Double cible de compilation** :
   - **WebAssembly (`wasm32-unknown-unknown`)** : exécutable dans le bac à sable Wasm du Core Aether via l'ABI standard `media-extension-sdk`.
   - **Binaire natif** : support de `--manifest` et `--run` pour les scripts d'administration et tests CLI.

---

## 🛠️ Opérations supportées

- `manifest` : retourne le manifeste complet du plugin et son schéma de configuration.
- `intro.detect` / `markers.detect` : analyse une liste d'épisodes et produit les segments détectés.
- `intro.match_fingerprints` : compare deux séries d'empreintes acoustiques et retourne le segment commun.
- `intro.parse_chapters` : extrait les marqueurs d'intro/crédits à partir des chapitres du conteneur.

---

## ⚙️ Schéma de configuration (`configSchema`)

| Clé | Type | Défaut | Description |
|---|---|---|---|
| `min_intro_duration` | number | `15` | Durée minimale d'intro en secondes |
| `max_intro_duration` | number | `120` | Durée maximale d'intro en secondes |
| `min_credits_duration` | number | `10` | Durée minimale de crédits en secondes |
| `similarity_threshold` | number | `0.75` | Seuil d'accord acoustique (0.70 - 0.90) |
| `search_window_end` | number | `600` | Plage de recherche (10 premières minutes) |
| `auto_skip_intro` | boolean | `false` | Sauter l'intro automatiquement |
| `auto_skip_credits` | boolean | `false` | Passer à l'épisode suivant automatiquement |
| `enable_chapter_detection` | boolean | `true` | Détection préalable par chapitres |
| `enable_fingerprint_detection` | boolean | `true` | Détection acoustique par empreintes |
