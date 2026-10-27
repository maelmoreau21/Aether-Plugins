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
