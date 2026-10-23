# Aether-Plugins — catalogue installable

## Le lien à ajouter dans Aether

```text
https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/main/repository/manifest.json
```

Les fichiers `repository/manifest.json`, `repository/packages/*.package.json` et
`repository/publisher-public.txt` forment le catalogue public : TMDb, IMDb,
fanart.tv, OMDb, MusicBrainz, Discogs, LRCLIB et avatars. Ce lien devient disponible après publication de ces fichiers
sur la branche `main` de ce dépôt. Les modifications locales ne le publient pas.
Aether préconfigure cette URL une seule fois. L’administrateur peut retirer le
dépôt ; il ne réapparaît pas au redémarrage et les plugins installés sont conservés.

Dans Administration → Extensions → Catalogue, **Actualiser les dépôts**, vérifier
puis approuver la clé publique de l’éditeur `maelmoreau21`, et installer les plugins
souhaités. Chaque installation reste **désactivée**. Dans Installées → Configurer
et gérer : enregistrer les paramètres fournisseur, choisir les bibliothèques,
puis activer. Une clé fournisseur reste propre à chaque serveur ; aucune clé
TMDb/fanart.tv ou clé privée de signature n’est présente dans le catalogue.

Pour vérifier les fichiers sans réseau ni clé privée :

```powershell
node scripts/test-repository.cjs
```

Cette vérification est également dans la CI. Elle contrôle signatures Ed25519,
SHA-256 des paquets et des modules, concordance des manifestes, URL et tailles.
La clé publique publiée peut être comparée avec la copie locale conservée par
l’auteur. Aucune confiance éditeur ni autorisation de bibliothèque n’est implicite.

## Mettre à jour le catalogue

Conserver la clé privée de signature hors du dépôt. La compilation/signature
utilise les outils du Core voisin. Exemple de préparation initiale d’une **nouvelle**
clé (ne pas recréer la clé si elle existe déjà) :

```powershell
$signingDirectory = Join-Path $env:LOCALAPPDATA 'MediaExtensionSigning'
New-Item -ItemType Directory -Force $signingDirectory | Out-Null
$signingKey = Join-Path $signingDirectory 'catalog-v1.pk8'
$publicKeyPath = Join-Path $signingDirectory 'catalog-v1.public.txt'
# Seulement si cette clé n’existe pas : keygen refuse tout écrasement.
$publicKey = & ..\Aether\target\debug\media-extension-pack.exe keygen $signingKey
if ($LASTEXITCODE -ne 0) { throw 'Création de clé échouée.' }
[IO.File]::WriteAllText($publicKeyPath, ($publicKey -join '').Trim(), [Text.UTF8Encoding]::new($false))
```

La clé `catalog-v1.pk8` de la préparation locale du 23 octobre est conservée dans
ce dossier privé. Sauvegardez-la ; les fichiers `repository/` ne permettent pas
de signer une prochaine version. Aucun secret n’a été configuré dans GitHub.

Avec la paire déjà créée :

```powershell
.\scripts\Build-Repository.ps1 -KeyPath $signingKey -PublicKeyPath $publicKeyPath
```

Le script détecte les crates `aether-plugin-*` qui exposent un exécutable de
manifeste et compile leurs modules Wasm. Il produit tous les paquets signés et
leur manifeste commun dans un **nouveau** dossier `artifacts/repository-DATE`.
Les paquets ont un nom versionné (par exemple `tmdb-0.1.0.package.json`) ;
conserver les anciennes versions si elles restent référencées dans un catalogue.
Le générateur vérifie aussi les signatures à la fin. Le générateur individuel accepte aussi les nouveaux noms de plugins ; les
permissions par défaut des nouveaux fournisseurs sont celles des fournisseurs
actuels. Un nouveau type d’extension doit déclarer les droits métier appropriés
avant publication. Les fichiers existants ne sont pas écrasés. Après contrôle,
remplacer explicitement les paquets, le manifeste et la clé publique dans
`repository/`, puis lancer `node scripts/test-repository.cjs`. Ne publier ni
`*.pk8` ni les dossiers de compilation. Le script ne pousse rien sur GitHub.

Pour mettre à jour une installation existante, le Core demande encore une
désinstallation/réinstallation et de nouveaux consentements : pas de mise à jour
silencieuse. Une rotation de clé exige un nouvel identifiant d’éditeur.

## Guide local détaillé

Ce dépôt contient les sources Rust des fournisseurs TMDb, IMDb, fanart.tv, OMDb, MusicBrainz, Discogs, LRCLIB et des
avatars. Le serveur Aether reste dans le dépôt voisin `Aether`. Les dépendances
locales supposent cette disposition :

```text
GitHub/
  Aether/
  Aether-Plugins/
```

Le serveur charge des **paquets JSON signés contenant un module WebAssembly**.
Les exécutables natifs servent aux outils/tests ; les DLL Jellyfin, exécutables,
manifestes seuls et dossiers de sources ne sont pas des paquets installables.

## 1. Démarrer Aether

Suivre [le guide local Windows](../Aether/docs/INSTALLATION_LOCALE.md) : compilation
Web/Core, SQLite, création interactive du premier administrateur, démarrage sur
`http://127.0.0.1:3000/app/login/`. Le serveur et son worker doivent provenir de la
même compilation. Aucune extension n’est nécessaire pour utiliser les médias locaux.

Les fournisseurs sont des extensions séparées : aucune intégration TMDb/IMDb/fanart.tv
n'est compilée dans le Core. Leurs manifestes déclarent `httpHosts`, domaines HTTPS
autorisés couverts par la signature du paquet et affichés avant activation. La
configuration et l'authentification fournisseur restent dans chaque extension.
`Build-LocalPackage.ps1` copie ces domaines et demande `metadata.read`/`metadata.write`/`images`
pour les fournisseurs, ou seulement `avatars` pour `-Plugin avatar`.
Après mise à jour du Core, recréez et réinstallez les anciens paquets réseau pour
déclarer ces droits ; ne modifiez jamais un paquet déjà signé. Aucun domaine n'est
autorisé automatiquement d'après l'identifiant d'une extension.

Le Core propose une recherche suivie du choix explicite du résultat, des détails
et de leur application. L’année, le type film/série et les identifiants existants
viennent du catalogue. fanart.tv nécessite un identifiant TMDb/IMDb pour les films
ou TVDB pour les séries ; il ne recherche pas par titre. Les anciens paquets
doivent être reconstruits et signés pour recevoir la permission `images`.
Les packs d’avatars activés sont disponibles dans les paramètres du compte.
Le Core transmet une variante explicite : le module Wasm ne dépend pas de
l’horloge système pour générer l’avatar.

## 2. Préparer sa clé de signature

Dans PowerShell, avec Rust/Cargo et les deux dépôts présents :

```powershell
Set-Location C:\Users\Mael\Documents\GitHub\Aether
cargo build --locked -p media-poc --bin media-extension-pack --bin media-extension-worker -j 1
$signingDirectory = Join-Path $env:LOCALAPPDATA 'MediaExtensionSigning'
New-Item -ItemType Directory -Force $signingDirectory | Out-Null
$signingKey = Join-Path $signingDirectory 'local.pk8'
.\target\debug\media-extension-pack.exe keygen $signingKey
```

`keygen` affiche uniquement la **clé publique Base64** : gardez-la pour l’interface.
Le fichier `local.pk8` est privé, à conserver sous votre compte Windows et à ne
jamais envoyer à Aether, inclure dans un paquet ou committer. L’outil refuse de
l’écraser. Si cette clé existe déjà, réutilisez-la et sa clé publique conservée.
Vous signez ici votre propre compilation avec l’identifiant d’éditeur `local`.

## 3. Compiler et signer un paquet

```powershell
rustup target add wasm32-unknown-unknown
Set-Location C:\Users\Mael\Documents\GitHub\Aether-Plugins
.\scripts\Build-LocalPackage.ps1 -Plugin tmdb -KeyPath $signingKey -PublisherId local
```

Le script compile le module Wasm optimisé pour la taille, extrait le manifeste,
calcule son empreinte et appelle l’outil officiel de signature. Il crée
`artifacts/tmdb.package.json`. Pour IMDb ou fanart.tv, remplacer `tmdb` par `imdb`, `fanart`, `omdb`, `musicbrainz`, `discogs`, `lrclib` ou `avatar`. Les clés des fournisseurs ne sont jamais demandées par le script et
ne figurent pas dans le paquet. En cas de recompilation, fournir un nouveau
`-OutputDirectory` : un paquet existant n’est pas écrasé.

La signature valide aussi les limites du serveur, dont un module de 256 Kio au
maximum. Une compilation réussie ne prouve pas qu’un fournisseur distant répond
ni qu’un navigateur peut lire un média.

## 4. Installer dans l’interface

Utiliser le catalogue décrit au début du document. Pour un paquet préparé
localement mais non publié, l’import côté serveur reste documenté dans
`Aether/docs/EXTENSIONS_API.md` (`MEDIA_EXTENSION_IMPORT_DIR`). L’interface normale
se concentre sur le catalogue, les dépôts et la configuration des installations.

## Publier son catalogue de paquets

Le Core propose les onglets **Installées**, **Catalogue** et **Dépôts**. Pour
préparer un manifeste de dépôt à partir de paquets déjà signés :

```powershell
.\scripts\New-RepositoryManifest.ps1 -PackageDirectory .\artifacts\local-20261020 -PublicKeyPath .\artifacts\local-20261020\publisher-public.txt -BaseUrl https://exemple.org/packages/ -OutputPath .\artifacts\manifest.json
```

Hébergez ensuite explicitement les paquets à l’URL choisie et `manifest.json` sur
votre serveur HTTPS. Le script ne publie rien et ne contient aucune clé privée.
Les paquets de ce dossier doivent être signés par l’éditeur correspondant à la
clé publique indiquée. Le fichier de sortie ne doit pas exister ; pour une
actualisation, préparez une nouvelle sortie et vérifiez-la avant publication.

Dans Aether → Extensions → Dépôts, ajoutez le nom et l’URL du manifeste, puis
choisissez une version dans Catalogue. La clé de l’auteur doit être vérifiée et
approuvée explicitement avant installation. L’activation ne nécessite pas de
redémarrage. [Contrat, limites réseau et format](../Aether/docs/EXTENSION_REPOSITORIES.md).

Aucun dépôt officiel distant n’est configuré tant qu’une URL réellement hébergée
n’a pas été ajoutée. Les manifestes Jellyfin ne sont pas compatibles.

## Les clés et leurs usages

| Clé | Où la renseigner | Usage |
| --- | --- | --- |
| API Aether | Administration → Clés API, puis application cliente | Lire le catalogue autorisé |
| API du fournisseur | Paramètres de l’extension installée | Appeler TMDb, OMDb, Discogs, fanart.tv |
| Signature Ed25519 | Clé privée sur le PC ; clé publique dans Éditeurs approuvés | Vérifier l’auteur et l’intégrité du paquet |

Un choix « sans expiration » sur une clé Aether ne supprime pas les quotas,
restrictions ou conditions d’un fournisseur tiers. Une clé Aether ne remplace
jamais une clé TMDb. Les secrets de configuration sont stockés dans la base
applicative ; protéger l’accès au compte administrateur, à cette base et aux
sauvegardes.

## Développement et validation

```powershell
cargo test -j 1
cargo fmt --all --check
cargo clippy --all-targets -j 1 -- -D warnings
```

Le SDK officiel est `../Aether/crates/extension-sdk`; `aether-plugin-api` le
réexporte et fournit le contrat des fournisseurs. Le broker du Core autorise
uniquement les hôtes HTTPS prévus pour ces fournisseurs. Les tests locaux/mocks
ne constituent pas une validation réelle des clés ou comptes externes.
