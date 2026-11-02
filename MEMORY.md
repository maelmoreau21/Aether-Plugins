## 2 novembre 2026 — Conservation des anciennes versions signées

- Le contrôle du catalogue comparait le nombre total de fichiers au nombre
  d'entrées actives : publication de huit paquets avec sept anciennes versions
  conservées échouait sur `15 != 8` malgré les signatures valides.
- Les anciens paquets restent conservés et sont aussi vérifiés : signature,
  SHA-256, taille, encodage et unicité id/version. Les paquets actifs restent
  contrôlés contre les empreintes et URLs du catalogue.
- Quatre régressions ciblées et catalogue local validés. Test ajouté à la CI.

## 2 novembre 2026 — Manifeste signé compatible Core

- Les workflows de publication transmettaient le manifeste interne du fournisseur
  au Core strict (`provider`, `capabilities`, `supportedMediaTypes`, `defaultPriority`
  non reconnus). Les deux workflows utilisent maintenant `package-manifest.py`
  pour produire explicitement le contrat signé, en conservant droits, domaines,
  paramètres, versions et bannière et en calculant le SHA-256 du module.
- Vérification ciblée : vrai manifeste Discogs, conversion puis signature par
  media-extension-pack réussies ; Actionlint et diff propres. Aucun rebuild ni
  suite de tests générale lancés. Correctif local, non poussé.

## 2 novembre 2026 — Clé du catalogue configurée dans GitHub

- Échec du run `41493653249` : seul le secret Poster Rotator existait dans
  l'environnement `production`. La clé locale `catalog-v1.pk8` a été vérifiée
  contre la clé publique du catalogue, puis enregistrée comme secret GitHub
  `ED25519_PRIVATE_KEY` dans Aether-Plugins/production, sans afficher sa valeur.
- Seul le job de publication en échec a été relancé ; les jobs CI réussis n'ont
  pas été relancés. Aucune nouvelle identité de signature n'a été créée.

## 2 novembre 2026 — Paquets develop automatiques

- Après CI verte sur push develop : calcul depuis le dernier catalogue, incrément
  PATCH si nécessaire, compilation/signature, catalogue develop, GitHub prerelease.
  Les tags seuls ont été remplacés par des paquets réellement installables.
- Manifestes numériques pour compatibilité Core ; suffixe bêta porté par le tag.
  Pas d'écrasement de paquet ni push forcé. Stable main garde sa préparation contrôlée.
- Détection du nom du binaire réel (dont intro-skipper-cli) dans les deux workflows.
  Core bêta suit le catalogue develop pour le dépôt officiel préconfiguré.
- TMDb details : ne demande plus le lot de toutes les traductions et images avec
  les crédits ; affiches principales via poster_path, recherche images séparée.
- Vérifié : 8 règles de release, plan de versions et Actionlint. Signature et
  publication GitHub non déclenchées ; secret production existant requis.

## 2 novembre 2026 — Dependabot et résolution indépendante du SDK

- La CI plugins du commit `cba7c7e` est verte (run `41446201445`). L'échec
  distinct de Dependabot Cargo (run `41381973942`) venait du chemin externe
  `../Aether/crates/extension-sdk/Cargo.toml`, absent dans son checkout.
- Le SDK est désormais une dépendance Git à la révision exacte publique
  `8f3373803a30d11e8c2c9d97ca5bfcf4bf70fe07`. Le lockfile ne change que sa
  provenance. README et modèle de plugin utilisent la même révision.
  Une surcharge locale reste possible explicitement pour le développement.
- Suppression des deux checkouts Aether qui masquaient cette dépendance externe
  dans la CI plugins ; la préparation des releases garde ses outils de signature.
- Validation réelle : résolution `cargo metadata --locked --offline` dans une
  copie temporaire isolée sans dossier voisin Aether ; source SDK Git confirmée.
  `cargo check -p aether-plugin-api -j 1` et ses cinq tests unitaires réussis,
  huit tests de politique de release, cohérence des manifestes avec versions
  non publiées, Actionlint 1.7.12 et contrôle du diff réussis.
- Correctifs locaux ; aucun nouveau run GitHub, push ou publication exécuté.

## 31 octobre 2026 — Finalisation et robustesse de l'extension IntroSkipper

- **Améliorations majeures d'IntroSkipper (Aether-Plugins-IntroSkipper)** :
  - **Détection des génériques de fin & scènes post-crédits (credits.rs)** :
    - Prise en charge des empreintes compactes de fin (	ailFingerprintsCompact en Base64).
    - Détection fine des scènes post-crédits : si le générique de fin se termine avant duration - 12.0s, le marqueur s'arrête exactement à la fin du générique pour ne pas masquer la scène finale.
    - Élimination des faux positifs sur les silences ou musiques d'ambiance de fin via corrélation croisée.
  - **Consensus & clustering multi-épisodes (consensus.rs)** :
    - Traitement par grappes (clustering) multi-épisodes : analyse conjointe de 2 à 8 épisodes par lot.
    - Élimination des faux positifs : rejet automatique des correspondances isolées (1 seul vote sur une saison >= 3 épisodes) si la confiance acoustique est inférieure à 0.80.
    - Fusion hybride chapitre + acoustique : affinage automatique des bornes du chapitre dès qu'une correspondance audio concorde (confiance fixée à 0.99).
  - **Chapitres multilingues et étendus (chapters.rs)** :
    - Détection multilingue étendue (français, anglais, japonais, allemand, espagnol, italien).
    - Support des marqueurs 
ecap (« Précédemment... », « Recap », « ダイジェスト ») et preview (« Dans le prochain épisode... », « Preview », « 次回予告 »).
  - **Compatibilité WebAssembly & empreinte binaire** :
    - Compilation cible wasm32-unknown-unknown --release réussie : taille binaire de **229 Ko**, respectant le budget strict de 256 Ko.
    - Temps d'analyse par épisode < 35 ms.
    - Nommage du binaire CLI en intro-skipper-cli dans Cargo.toml pour éviter les collisions de symboles sous Windows.
- **Validations réelles** :
  - cargo test --manifest-path Aether-Plugins-IntroSkipper/Cargo.toml : 15/15 tests intro_skipper_tests.rs passés.
  - Benchmark 24 épisodes (
epresentative_benchmark.rs) : 100% précision, 100% rappel (TP=22 intros, TP=24 crédits, FP=0, FN=0).
  - cargo fmt -- --check et cargo clippy --all-targets -- -D warnings : 100% conformes sans avertissement.


## 31 octobre 2026 — Automatisation des versions et mises à jour groupées des plugins

- **Support du bump et de la release globale (`all`)** :
  - `detect_changed_plugins.py` enrichi pour accepter `--manual-plugin all` (ou `*`), déclenchant la détection et l'incrément de version coordonné pour tous les plugins.
  - `test-release-policy.py` étendu avec un test `test_manual_release_all_plugins` (8/8 tests validés).
  - Workflow GitHub Actions `prepare-plugin-release.yml` refactorisé pour itérer sur l'ensemble des plugins détectés ou demandés (`plugin_id: all`), compiler les modules Wasm, optimiser via Binaryen, signer avec Ed25519, mettre à jour `repository/manifest.json` et ouvrir une Pull Request unique et sécurisée.
  - Remplacement de la référence figée d'Aether dans le workflow par la branche de travail active.

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

## 2 novembre 2026 — Test de découverte des releases sur develop

- Le test utilisait toujours les URLs `main` et la version fanart `0.1.0`, alors
  que le catalogue de développement référence désormais ses paquets `develop`.
  Le test vérifie les entrées actives avec une liste explicite des bases
  `main`/`develop` et lit la version courante ; des fixtures indépendantes
  vérifient chaque branche, les anciennes versions, les paquets manquants,
  domaines étrangers, mauvaise branche/version, query/fragment et tags doublons.
- Le résolveur de publication reste strict sur la base fournie ; contrôles de
  signatures et paquets publiés inchangés. Validation ciblée uniquement :
  `node --test scripts/test-discover-plugin-releases.cjs`, 4 tests réussis.
  Aucun build Rust/Wasm, push ni publication. Nouvelle CI distante à confirmer.
