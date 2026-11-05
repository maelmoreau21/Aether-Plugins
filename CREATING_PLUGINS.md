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
officiel. Copiez-le dans votre dépôt, conservez la révision Git figée du SDK dans
`Cargo.toml`, puis modifiez `manifest.json` et `src/lib.rs`. Sa compilation ne
nécessite pas un clone local d'Aether ; les outils de signature ci-dessous, eux,
utilisent un checkout voisin.
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
`New-RepositoryManifest.ps1` produit l'ancien format (`apiVersion: 1`, une entrée
par paquet), toujours accepté ; pour un dépôt qui garde l'historique des versions,
préférez le format versionné ci-dessous.
Conservez la clé privée hors de Git. Le Core refuse les paquets altérés,
les modules trop grands et les versions incompatibles ; il conserve ses limites
Wasm, réseau, configuration, session et CSRF. Installer n’active pas le plugin.

Le modèle démontre `metadata.propose`, `metadata.search`, `metadata.details` et un
paramètre dynamique `prefix`. Le SDK fournit également les contrats images et
avatars. Les opérations métier disponibles sont celles décrites dans
`Aether/docs/EXTENSIONS_API.md` ; un manifeste ne crée pas une nouvelle API Core,
un accès disque arbitraire ou une interface HTML tierce.

<a id="publier"></a>
## Publier avec des releases GitHub (catalogue versionné, à la Jellyfin)

Un dépôt de plugins Aether peut être un simple dépôt GitHub : un fichier
`manifest.json` liste chaque plugin avec toutes ses versions, et chaque version
est un fichier attaché à une release GitHub.

```json
{
  "apiVersion": 2,
  "plugins": [{
    "id": "my-provider", "name": "Mon fournisseur", "description": "…",
    "overview": "Texte long facultatif", "owner": "Moi", "category": "Metadata",
    "imageUrl": "https://raw.githubusercontent.com/moi/mes-plugins/main/logo.png",
    "publisherId": "my-author", "publicKey": "<contenu de publisher-public.txt>",
    "versions": [{
      "version": "0.2.0", "targetAbi": "1.0", "minServerVersion": "0.1.0",
      "sourceUrl": "https://github.com/moi/mes-plugins/releases/download/my-provider-v0.2.0/my-provider-0.2.0.package.json",
      "checksum": "<sha256 du fichier sourceUrl>",
      "timestamp": "2026-11-05T10:00:00Z", "changelog": "- Nouveauté"
    }, {
      "version": "0.1.0", "targetAbi": "1.0",
      "sourceUrl": "https://github.com/moi/mes-plugins/releases/download/my-provider-v0.1.0/my-provider-0.1.0.zip",
      "checksum": "<sha256 du zip>", "timestamp": "2026-10-01T10:00:00Z"
    }]
  }]
}
```

Règles appliquées par le Core :

- `id` (ou `guid`) suit les identifiants Aether (`a-z`, `0-9`, `.`, `-`, `_`) ;
  une liste JSON nue de plugins (forme Jellyfin) est aussi acceptée.
- `sourceUrl` est le paquet signé `.package.json`, ou un `.zip` qui en contient
  exactement un. Les redirections de `github.com` vers ses hôtes de contenu
  (`objects.githubusercontent.com`, `release-assets.githubusercontent.com`,
  `codeload.github.com`) sont suivies, trois au plus, chaque saut étant revalidé.
- `checksum` est le SHA-256 du fichier téléchargé (MD5 refusé). Le paquet doit
  porter exactement l'`id`, la `version` et le `publisherId` de l'entrée, et sa
  signature doit correspondre à `publicKey`.
- `targetAbi` est un minimum comparé numériquement (`1`, `1.0`, `1.0.0.0`).
- Limites : 1 Mio de manifeste, 200 plugins, 1 000 versions, 8 Mio par fichier
  téléchargé, 2 Mio de paquet extrait.

Pour signer sans compiler le Core, Node.js suffit :

```powershell
node scripts/sign-package.cjs manifest.json module.wasm $creatorPrivate my-author $creatorPublic my-provider-0.2.0.package.json
node scripts/add-catalogue-version.cjs --manifest manifest.json --package my-provider-0.2.0.package.json `
  --public-key $creatorPublic --changelog "- Nouveauté" `
  --source-url https://github.com/moi/mes-plugins/releases/download/my-provider-v0.2.0/my-provider-0.2.0.package.json
```

`media-extension-pack verify PAQUET.json publisher-public.txt` (Core) contrôle un
paquet exactement comme à l'installation. Ne retirez ni ne remplacez jamais une
version publiée : publiez une nouvelle version.

Dans ce dépôt, tout cela est automatisé par **Actions → Release plugin**
(voir [docs/RELEASES_AND_VERSIONING.md](docs/RELEASES_AND_VERSIONING.md)) :

1. augmenter la version dans le `Cargo.toml` du plugin et la committer ;
2. lancer le workflow en choisissant le plugin (ou pousser `<préfixe>-v<x.y.z>`) ;
3. le workflow construit, signe, crée la release et ajoute la version au catalogue.

Configuration GitHub nécessaire (une fois) : secret `ED25519_PRIVATE_KEY`
(base64 du fichier `.pk8` de `keygen`) dans l'environnement `production` ; la
branche `main` doit autoriser le push de `github-actions[bot]` (ou définir la
variable `CATALOGUE_BRANCH` vers une branche non protégée et y pointer Aether).

### Installer une version précédente

Aether → Extensions → Catalogue → l'extension : chaque version listée a un bouton
**Installer cette version**. Si une version plus récente est déjà installée, le
retour arrière demande une confirmation, désactive l'extension et garde la version
courante comme sauvegarde restaurable. Il est refusé pour une extension qui stocke
des données privées ou dont les paramètres enregistrés ne conviennent pas à
l'ancienne version.

## Développement dans ce dépôt

Déclarez `permissions` dans `PluginManifest` : le script de paquetage les reprend
sans reconnaître un nom de fournisseur. Ajoutez une crate Rust avec une cible
binaire `--manifest`, un module Wasm et un nom `aether-plugin-*` au workspace ;
Build-Repository la découvre automatiquement. Le code d’Aether reste inchangé.
Les stdout des manifestes sont décodés explicitement en UTF-8, indépendamment de
la console Windows. `test-repository.cjs` vérifie signature, empreintes, URL,
limites et absence des séquences de texte corrompues observées.
