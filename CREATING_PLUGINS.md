# Créer et distribuer son plugin

Stockage privé v1 : déclarer `storage.private`, puis utiliser les modules
`media_extension_sdk::storage` et `media_extension_sdk::objects` (également
réexportés par `aether-plugin-api`). Le Core contrôle l'identité, les scopes et
les quotas ; aucune connexion SQL ni chemin disque dans le guest. Exemple
synthétique hors catalogue : `templates/persistent-storage` ; compteur durable
avec CAS, compilable pour `wasm32-unknown-unknown`. Voir le contrat complet dans
`Aether/docs/EXTENSION_STORAGE.md`, notamment rétention, expiration et objets.

Aucune liste d’auteurs ni de fournisseurs n’est compilée dans Aether. Un auteur
publie son module Wasm, son manifeste signé et un catalogue HTTPS. L’administrateur
ajoute ce lien, installe, configure puis active le plugin. Aucune approbation
manuelle d’éditeur, modification d’Aether ou recompilation du serveur n’est requise.
La signature contrôle la cohérence du paquet avec la clé du catalogue : elle
n’est pas une certification de son auteur. Le court avertissement reste dans Dépôts.

## Exemple indépendant prêt à compiler

Le dossier `templates/metadata-provider` est un projet autonome, hors du catalogue
officiel. Copiez-le dans votre dépôt, adaptez le chemin `media-extension-sdk` dans
Cargo.toml vers un clone d’Aether, puis modifiez `manifest.json` et `src/lib.rs`.
Le manifeste déclare identité, version, permissions, hôtes réseau et `configSchema`.
L’interface génère les paramètres texte, secret, liste et interrupteur à partir
de ce schéma ; aucune page Svelte propre à votre plugin n’est nécessaire.

Depuis Aether-Plugins (PowerShell 7 ; clones Aether et Aether-Plugins côte à côte) :

```powershell
rustup target add wasm32-unknown-unknown
cargo build --manifest-path templates/metadata-provider/Cargo.toml --release --target wasm32-unknown-unknown
cargo build --manifest-path ../Aether/Cargo.toml -p media-poc --bin media-extension-pack -j 1
$creatorKeys = Join-Path $env:LOCALAPPDATA 'MyPluginSigning'
New-Item -ItemType Directory -Force $creatorKeys | Out-Null
$creatorPrivate = Join-Path $creatorKeys 'publisher.pk8'
$creatorPublic = Join-Path $creatorKeys 'publisher-public.txt'
# À faire une seule fois ; keygen refuse d’écraser une clé existante.
$public = & ../Aether/target/debug/media-extension-pack.exe keygen $creatorPrivate
[IO.File]::WriteAllText($creatorPublic, ($public -join '').Trim(), [Text.UTF8Encoding]::new($false))
New-Item -ItemType Directory -Force artifacts/my-repository/packages | Out-Null
& ../Aether/target/debug/media-extension-pack.exe pack templates/metadata-provider/manifest.json templates/metadata-provider/target/wasm32-unknown-unknown/release/my_metadata_provider.wasm $creatorPrivate my-author artifacts/my-repository/packages/my-provider-0.1.0.package.json
./scripts/New-RepositoryManifest.ps1 -PackageDirectory artifacts/my-repository/packages -PublicKeyPath $creatorPublic -BaseUrl https://example.org/plugins/packages/ -OutputPath artifacts/my-repository/manifest.json
```

Hébergez le manifeste et les paquets à ces URL HTTPS ; ajoutez le lien du
manifeste dans Aether → Extensions → Dépôts. Une URL raw GitHub convient aussi.
Conservez la clé privée hors de Git. Le Core refuse les paquets altérés,
les modules trop grands et les versions incompatibles ; il conserve ses limites
Wasm, réseau, configuration, session et CSRF. Installer n’active pas le plugin.

Le modèle démontre `metadata.propose`, `metadata.search`, `metadata.details` et un
paramètre dynamique `prefix`. Le SDK fournit également les contrats images et
avatars. Les opérations métier disponibles sont celles décrites dans
`Aether/docs/EXTENSIONS_API.md` ; un manifeste ne crée pas une nouvelle API Core,
un accès disque arbitraire ou une interface HTML tierce.

## Développement dans ce dépôt

Déclarez `permissions` dans `PluginManifest` : le script de paquetage les reprend
sans reconnaître un nom de fournisseur. Ajoutez une crate Rust avec une cible
binaire `--manifest`, un module Wasm et un nom `aether-plugin-*` au workspace ;
Build-Repository la découvre automatiquement. Le code d’Aether reste inchangé.
Les stdout des manifestes sont décodés explicitement en UTF-8, indépendamment de
la console Windows. `test-repository.cjs` vérifie signature, empreintes, URL,
limites et absence des séquences de texte corrompues observées.
