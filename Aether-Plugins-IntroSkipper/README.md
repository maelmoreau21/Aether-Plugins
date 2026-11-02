# Aether Plugin — Intro Skipper

Portage officiel, autonome et hautement optimisé en **Rust** du célèbre plugin [intro-skipper/intro-skipper](https://github.com/intro-skipper/intro-skipper).

Intro Skipper classe des chapitres et compare des empreintes déjà fournies par l'hôte. L'intégration automatique au scanner, au stockage et au lecteur Aether reste à réaliser ; installer ce paquet seul ne déclenche aucune analyse média.

---

## ⚡ Caractéristiques & Optimisations

1. **Zéro dépendance native lourde à l'exécution** :
   - Algorithme d'alignement audio Chromaprint pur Rust, sans allocations superflues.
   - Calcul de distance de Hamming vectorisé avec les instructions CPU `POPCNT` (`count_ones()`).
   - Analyse par diagonales de corrélation croisée avec sous-échantillonnage adaptatif. Les gains de vitesse sur un corpus réel restent à mesurer.
2. **Consensus multi-épisodes (Season Consensus)** :
   - Détection des décalages d'intro (Cold Opens / résumés de début d'épisode).
   - Comparaisons entre épisodes fournis dans un même lot ; les lots mélangeant séries ou saisons sont rejetés. La précision et les faux positifs restent à mesurer sur un corpus annoté.
3. **Double moteur d'analyse** :
   - **Comparaison acoustique** : compare des vecteurs d'empreintes `u32` fournis par l'hôte. Le plugin ne calcule pas ces empreintes et ne lance pas FPcalc.
   - **Détection sémantique par chapitres** : extraction immédiate (0ms CPU) si le conteneur MKV/MP4 contient déjà des chapitres nommés multilingues (`Intro`, `Opening`, `OP`, `Générique`, `Credits`, `ED`, etc.).
4. **Détection des crédits & génériques de fin** :
   - Analyse de la queue d'épisode pour proposer la transition fluide vers l'épisode suivant.
5. **Double cible de compilation** :
   - **WebAssembly (`wasm32-unknown-unknown`)** : exécutable dans le bac à sable Wasm du Core Aether via l'ABI standard `media-extension-sdk`.
   - **Binaire natif** : support de `--manifest` et `--run` pour les scripts d'administration et tests CLI.

---

## 🛠️ Opérations supportées

- `manifest` : retourne le manifeste complet du plugin et son schéma de configuration.
- `intro.detect` / `markers.detect` : analyse une liste d'épisodes et produit des candidats. L'hôte doit fournir des épisodes d'une seule série et saison, avec des empreintes de même cadence.
- `intro.match_fingerprints` : compare deux séries d'empreintes acoustiques et retourne le segment commun.
- `intro.parse_chapters` : extrait les marqueurs d'intro/crédits à partir des chapitres du conteneur.

---

## ⚙️ Schéma de configuration (`configSchema`)

| Clé | Type | Défaut | Description |
|---|---|---|---|
| `min_intro_duration` | number | `15` | Durée minimale d'intro en secondes |
| `max_intro_duration` | number | `120` | Durée maximale d'intro en secondes |
| `min_credits_duration` | number | `10` | Durée minimale de crédits en secondes |
| `max_credits_duration` | number | `300` | Durée maximale du marqueur de crédits (jusqu'à 300 s) |
| `similarity_threshold` | number | `0.75` | Seuil d'accord acoustique (0.70 - 0.90) |
| `search_window_end` | number | `600` | Plage de recherche (10 premières minutes) |
| `search_window_start` | number | `0` | Début de recherche acoustique pour exclure un résumé récurrent |
| `auto_skip_intro` | boolean | `false` | Sauter l'intro automatiquement |
| `auto_skip_credits` | boolean | `false` | Passer à l'épisode suivant automatiquement |
| `enable_chapter_detection` | boolean | `true` | Détection préalable par chapitres |
| `enable_fingerprint_detection` | boolean | `true` | Détection acoustique par empreintes |

Les durées doivent être finies et positives, avec minimum ≤ maximum. Les intros
sont plafonnées à 600 secondes, les crédits à 300 ; la fenêtre acoustique doit
rester dans 0–600 secondes et contenir la durée minimale d'intro. Un réglage
numérique invalide renvoie `invalid_detection_config` avant toute comparaison.
Les valeurs numériques JSON et les chaînes du formulaire d'administration sont
acceptées. La fenêtre de recherche ne modifie pas les chapitres nommés ; le
maximum des crédits s'applique aux chapitres et à la comparaison acoustique.

Ces limites n'allongent pas l'extraction du Node : le code hôte actuel conserve
au plus 2500 empreintes en tête (~290 s) et 1500 en queue (~174 s pour un épisode
long). Le plugin cherche uniquement dans les empreintes disponibles. Une fenêtre
entièrement silencieuse ou plate est écartée même si l'audio hors fenêtre varie.
Le bouton de détection par chapitres désactive aussi `intro.parse_chapters`.
Les nouveaux champs apparaîtront dans l'administration après construction et
installation d'un paquet contenant ce schéma ; aucun paquet signé n'est modifié.
