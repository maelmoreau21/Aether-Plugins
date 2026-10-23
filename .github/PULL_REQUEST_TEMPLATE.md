## Description

<!-- Modifications et motivation. -->

## Plugin(s) concerné(s)

- [ ] Avatar
- [ ] IMDb
- [ ] TMDb
- [ ] fanart.tv
- [ ] SDK `aether-plugin-api`
- [ ] Dépôt de plugins / scripts / CI / documentation

Issue liée : <!-- ex: Closes #123 -->

## Garde-fous

- [ ] Aucun secret, clé API ou jeton dans le code, les fixtures ou les logs.
- [ ] Les tests n'appellent pas de service réseau réel.
- [ ] Aucune clé de signature privée n'est ajoutée au dépôt.
- [ ] Documentation mise à jour si le comportement change.

## Vérifications locales

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `cargo build --workspace --lib --target wasm32-unknown-unknown --release`
- [ ] `node scripts/test-repository.cjs` (si le dépôt signé est modifié)
