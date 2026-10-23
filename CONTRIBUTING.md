# Contribuer

- Branches : `main` (stable) et `develop` (intégration). Créez vos branches `feat/...` ou `fix/...` depuis `develop` et ouvrez la PR vers `develop`.
- Avant PR : `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace`, build `wasm32-unknown-unknown`.
- Tests sans accès réseau réel ; aucun secret ni clé de signature privée dans Git.
- Le SDK partagé vient du dépôt Aether (branche `develop`).
