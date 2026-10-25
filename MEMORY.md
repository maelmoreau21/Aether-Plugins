# Etat et validations des plugins

## 25 octobre 2026 - Modules Wasm sous la limite de 256 Kio

- Ajout de Binaryen 132.0.0 verrouille par package-lock.json, optimisation -Oz apres compilation et avant validation, empreintes et signature. Meme parcours dans CI, release et Build-LocalPackage.ps1. Pas de changement du runtime ni de la limite de 262144 octets ; les paquets signes existants sont conserves.
- Verification locale Windows, Rust 1.99.1 : cargo build --locked --workspace --lib --target wasm32-unknown-unknown --release, puis optimize-wasm.mjs et test-wasm.cjs. Neuf modules valides, imports autorises et execution ABI sans reseau ; IMDb 266928 -> 224263 octets, module le plus gros de cette compilation optimisee. IntroSkipper 181284 octets.
- Tests negatifs : module absent, module de 262145 octets et appel de l'optimiseur sans entree refusent ; selection isolee IMDb valide. test-manifests.cjs et test-repository.cjs valident les huit paquets signes existants ; IntroSkipper reste en attente de premiere publication signee.
- cargo fmt --all --check, syntaxe JavaScript, YAML des workflows et parseur PowerShell : controles locaux reussis. Pas de modification Rust ; suites natives non relancees. CI Linux distante et publication non prouvees par ces controles locaux.
- Le manifeste develop peut encore referencer les paquets main : il ne compile pas le code source automatiquement. Les droits du volume Synology restent un diagnostic de deploiement distinct.
