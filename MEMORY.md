## 27 octobre 2026 — Audit, sécurisation et finalisation des extensions (P0, P1, P2, P3)

- **P0 — Architecture & Stabilité du SDK** :
  - **Gestion de l'ABI et des versions API** : vérification stricte et uniforme de `req.api_version == 1` dans chaque point d'entrée d'extension Wasm (`handle_*_extension_request`) et dans le gestionnaire central `aether_plugin_api::http::handle_extension`. Refus immédiat et propre avec `{"error": "unsupported_api_version"}` en cas d'incompatibilité sans panic.
  - **Audit des permissions** :
    - `IntroSkipper` : suppression de la permission superflue `metadata.read`. Seule la permission dédiée `playback.markers` est conservée, garantissant le principe de moindre privilège.
    - Vérification des hôtes réseau (`http_hosts`) : `IntroSkipper` et `Avatar` n'ont aucun accès réseau sortant (`http_hosts` vide, exécution 100 % locale). Les fournisseurs de métadonnées sont strictement restreints à leurs domaines vérifiés (`api.themoviedb.org`, `image.tmdb.org`, `m.media-amazon.com`, `assets.fanart.tv`, `api.discogs.com`, `musicbrainz.org`, `coverartarchive.org`, `lrclib.net`).
  - **Extensions devenues natives** :
    - `Poster Rotator` : retrait confirmé et suppression du dossier résiduel vide. Aucune référence orpheline.
    - `Avatars` : gestion désormais 100 % native côté Aether Core (routes `/api/v1/admin/avatar-packs`, support PNG/JPEG/WebP et import GitHub sans Wasm). Le plugin `Aether-Plugins-Avatar` est prêt pour dépréciation/retrait coordonné sans rompre les signatures historiques.

- **P1 — Finalisation et Optimisation d'IntroSkipper** :
  - **Optimisation des algorithmes** :
    - Détection de silence/audio plat `is_silent_or_flat` réécrite en O(1) mémoire sans allocation `HashSet` sur le tas (économie de jusqu'à 8192 allocations par paire d'épisodes).
    - Indexation par bandes d'octets optimisée : remplacement de `HashMap<(u8, u8), Vec<usize>>` par une table plate de 1024 buckets directs, éliminant les coûts de hachage SipHash et collisions.
    - Consensus multi-épisodes pour le générique de fin (`detect_fingerprint_credits`) étendu aux paires immédiates `(i, i+1)` et décalées `(i, i+2)` pour maximiser le rappel.
  - **Benchmark représentatif sur 24 épisodes (`tests/representative_benchmark.rs`)** :
    - Validation complète simulant 24 épisodes réels (cold opens variés, intros standard de 45s, intros longues de 90s, chapitres récapitulatifs, bruit acoustique 15 %, épisodes spéciaux sans générique et silences).
    - **Résultats obtenus** :
      - Précision d'intro : 100.0 % | Rappel d'intro : 100.0 % (22/22 détectés).
      - Précision crédits : 100.0 % | Rappel crédits : 100.0 % (24/24 détectés).
      - Faux positifs sur épisode spécial et silence : 0.
      - Temps d'exécution : ~340 ms pour 24 épisodes complets (~14 ms par épisode).
  - **Distribution Wasm** : module Wasm compilé et optimisé sous les 256 Kio (194 845 octets).

- **P2 — Finalisation des fournisseurs externes** :
  - `TMDb` : élimination de `Box::leak` dans `resolve_api_key` grâce à `Cow<'a, str>` pour la variable d'environnement `TMDB_API_KEY`.
  - `LRCLIB` : filtrage robuste avec tolérance de durée lors du fallback de recherche pour éviter les mauvaises associations.
  - `handle_extension` enrichi de l'opération `metadata.propose` pour tous les fournisseurs.

- **P3 — Validation & Qualité** :
  - `cargo fmt --all -- --check` : 100 % conforme.
  - `cargo clippy --workspace --all-targets -- -D warnings` : 0 avertissement, 0 erreur.
  - `cargo test --workspace` : Toutes les suites de tests passées avec succès.
  - `python scripts/test-release-policy.py` : 7/7 tests passés.
  - `node scripts/test-manifests.cjs` & `test-repository.cjs` : 100 % synchronisés et valides.
  - `node scripts/test-wasm.cjs` : 9/9 modules Wasm testés et validés sous la limite des 256 Kio.

## 25 octobre 2026 - Publication manuelle uniquement

- Les pushes/merges ne lancent plus Release Plugin. Run workflow sur main selectionne un plugin et PATCH (defaut), MINOR ou MAJOR. Une release manuelle ne selectionne plus les autres plugins modifies. Pas de reutilisation d'une version deja publiee.
- test-release-policy.py : 3 increments et isolation du plugin verifies ; ajoute au job workflows de CI. Aucun paquet, tag ou release publie par cette intervention.

# Etat et validations des plugins

## 25 octobre 2026 — Exemple synthétique de stockage persistant

- `templates/persistent-storage` hors catalogue : guest Rust/Wasm avec compteur durable via le SDK officiel, permission `storage.private` et CAS. Aucun SQL, chemin ou connexion DB dans le plugin ; imports `media_host_storage_v1` et `media_host_objects_v1` reconnus par le contrôle Wasm.
- Compilation Wasm Release réelle : module131331 octets. Dans Aether, intégration HTTP signée/activation et deux invocations via de vrais workers réussies ; compteur1 puis2. Documentation `CREATING_PLUGINS.md` et contrat `Aether/docs/EXTENSION_STORAGE.md`.
- Aucun paquet du catalogue reconstruit/signé/publié, aucun push. L'exemple n'est pas une nouvelle extension produit et reste hors workspace/catalogue officiels.

## 25 octobre 2026 - Modules Wasm sous la limite de 256 Kio

- Ajout de Binaryen 132.0.0 verrouille par package-lock.json, optimisation -Oz apres compilation et avant validation, empreintes et signature. Meme parcours dans CI, release et Build-LocalPackage.ps1. Pas de changement du runtime ni de la limite de 262144 octets ; les paquets signes existants sont conserves.
- Verification locale Windows, Rust 1.99.1 : cargo build --locked --workspace --lib --target wasm32-unknown-unknown --release, puis optimize-wasm.mjs et test-wasm.cjs. Neuf modules valides, imports autorises et execution ABI sans reseau ; IMDb 266928 -> 224263 octets, module le plus gros de cette compilation optimisee. IntroSkipper 181284 octets.
- Tests negatifs : module absent, module de 262145 octets et appel de l'optimiseur sans entree refusent ; selection isolee IMDb valide. test-manifests.cjs et test-repository.cjs valident les huit paquets signes existants ; IntroSkipper reste en attente de premiere publication signee.
- cargo fmt --all --check, syntaxe JavaScript, YAML des workflows et parseur PowerShell : controles locaux reussis. Pas de modification Rust ; suites natives non relancees. CI Linux distante et publication non prouvees par ces controles locaux.
- Le manifeste develop peut encore referencer les paquets main : il ne compile pas le code source automatiquement. Les droits du volume Synology restent un diagnostic de deploiement distinct.
## 26 octobre 2026 — Réparation de la validation des workflows GitHub

- Les deux workflows de release utilisent maintenant des chaînes shell valides pour le corps de Pull Request et les notes GitHub Release ; les textes Markdown restent inchangés. Les sorties de `prepare-plugin-release.yml` sont regroupées pour ShellCheck SC2129, avec exceptions SC2016 limitées aux littéraux JavaScript et Markdown.
- Vérifié localement : `actionlint` 1.7.12 sur tous les workflows et `python scripts/test-release-policy.py` (7 tests). Aucune release ni Pull Request déclenchée ; exécution distante à confirmer après intégration.

## 27 octobre 2026 — Publication des paquets aux noms abrégés

- `release-plugins.yml` déduit désormais le fichier signé et le préfixe du tag
  de `packageUrl` dans le catalogue vérifié. `avatars` publie `avatar-*` et
  `fanart-tv` publie `fanart-*` ; un paquet manquant bloque le workflow au lieu
  d'être ignoré. La relance peut reprendre après la création du tag sans
  dupliquer une GitHub Release. Vérification locale par test ciblé et tests de
  catalogue ; aucune publication réelle n'a été déclenchée.
