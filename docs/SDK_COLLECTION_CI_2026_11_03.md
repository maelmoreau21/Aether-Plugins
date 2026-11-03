# CI — compatibilité du contrat collection, 3 novembre 2026

## Cause et correction

La CI du commit `0943099a6f0586a92c2f1d348f83e13c33df769f` échouait avec
E0422/E0560 : TMDb utilisait `MetadataCollection` et `StructuredMetadata.collection`,
absents du SDK épinglé à `8f3373803a30d11e8c2c9d97ca5bfcf4bf70fe07`.

Le correctif d'alignement était déjà présent dans le worktree au début de cette
mission : `Cargo.toml` et `Cargo.lock` épinglent désormais le SDK Git publié
`133896830c609a177e4c4c611084154d7e4aab5d`. Il est conservé et validé sans
substitution par un chemin local ni duplication des types du SDK.

Deux protections sont ajoutées :

- `crates/aether-plugin-api/tests/collection_contract.rs` vérifie la réexportation
  des types, l'aller-retour JSON d'une collection et la lecture d'une ancienne
  réponse sans ce champ. Le champ absent reste absent à la réémission.
- `scripts/test-metadata-wasm.cjs` exécute le module TMDb compilé avec et sans
  `belongs_to_collection`, via un broker HTTP déterministe hors ligne. L'identité
  reste préfixée `tmdb:` ; aucune collection artificielle n'est ajoutée.

## Vérifications exécutées localement

Windows, Rust/Cargo 1.99.1, dépendance SDK Git figée, sans patch local de SDK :

- `cargo test --locked -p aether-plugin-api -p aether-plugin-tmdb --tests` :
  **24 tests réussis**, dont les deux nouveaux contrats et le détail TMDb HTTP.
- `cargo test --locked --workspace` : **72 tests réussis, 0 échec, 0 ignoré**.
- `cargo clippy --locked --all-targets -- -D warnings` : réussi sur les sources
  finales, y compris le nouveau test.
- `cargo fmt --check` : réussi.
- `cargo build --locked --workspace --lib --target wasm32-unknown-unknown --release` : réussi.
- Optimisation Binaryen puis `node scripts/test-wasm.cjs` : **9 modules valides**,
  exports/imports et smoke ABI réussis, chacun sous 262 144 octets.
  Plus gros module de ce run : IMDb, 237 886 octets ; TMDb, 231 012 octets.
- `node scripts/test-metadata-wasm.cjs` : réussi, dont le nouveau contrat collection.
- `node --check scripts/test-metadata-wasm.cjs` et `git diff --check` : réussis.

Cargo émet des avertissements Windows de collision de noms `.pdb` entre cibles
bin/lib ; aucune erreur de compilation ou de test observée. Pas de modification
du runtime, des permissions, des quotas ou des signatures existantes.

## Livraison et limites

Lot indépendant : dépendance SDK + lockfile + tests de contrat. Les autres
modifications préexistantes des fournisseurs/paquetage/stockage ne sont pas
attribuées à ce correctif et ne doivent pas être embarquées par défaut dans sa PR.
Les tests ont toutefois porté sur le worktree courant, pas un checkout contenant
uniquement le changement de pin.

Les modules locaux ont été compilés et exécutés ; aucun paquet signé ou
catalogue publié n'a été réécrit. Aucun push, nouvelle CI Linux, service fournisseur
réel, clé API ou installation NAS n'est validé par ces tests. Les tailles sont
des mesures ponctuelles, sans benchmark comparable avant/après ni promesse de gain.

Prochaine priorité : corriger l'attente ASS du scénario Chrome Aether après
reproduction de l'échec et vérification du rendu réel.
