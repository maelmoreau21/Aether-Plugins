# Extension synthétique de stockage

Ce module de test hors catalogue incrémente une valeur durable par CAS à chaque
invocation. Il ne demande aucun accès réseau ou média. Avec deux invocations
concurrentes, une peut recevoir `version_conflict` : le résultat n'est pas perdu
par écrasement silencieux. L'exemple ne réessaie pas automatiquement.

Depuis Aether-Plugins :

```powershell
cargo build --manifest-path templates/persistent-storage/Cargo.toml --target wasm32-unknown-unknown --release
```

Les imports sont `media:media_host_storage_v1`, fournis uniquement par le Core.
La portée globale requiert une invocation administrateur. Le manifeste demande
`metadata.read` et `storage.private` ; installation/signature/activation suivent
le parcours normal, sans grants implicites. Aucun paquet signé n'est publié.
Voir `Aether/docs/EXTENSION_STORAGE.md` pour les limites et le cycle de vie.

Pour une mise à jour de 0.1.0 vers 0.1.1 sans changement de format, ajouter au
manifeste cible `"storageMigration":{"fromVersion":"0.1.0","keyRenames":[]}`.
Ce champ doit être signé avec le paquet ; `scripts/package-manifest.py` le conserve.
Le module répond à `lifecycle.verify` sans lire ni incrémenter le compteur : le
contrôle préalable du Core ne dispose pas du broker de stockage. Les renommages
KV peuvent être déclarés explicitement ; transformations de valeurs et migrations
arbitraires ne sont pas prises en charge. Voir `Aether/docs/PLUGIN_UPDATE_LIFECYCLE.md`.
