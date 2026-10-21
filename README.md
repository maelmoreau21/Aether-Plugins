# Aether-Plugins — installation locale

Ce dépôt contient les sources Rust des fournisseurs TMDb, IMDb, fanart.tv et des
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
`artifacts/tmdb.package.json`. Pour IMDb ou fanart.tv, remplacer `tmdb` par `imdb`
ou `fanart`. Les clés des fournisseurs ne sont jamais demandées par le script et
ne figurent pas dans le paquet. En cas de recompilation, fournir un nouveau
`-OutputDirectory` : un paquet existant n’est pas écrasé.

La signature valide aussi les limites du serveur, dont un module de 256 Kio au
maximum. Une compilation réussie ne prouve pas qu’un fournisseur distant répond
ni qu’un navigateur peut lire un média.

## 4. Installer dans l’interface

1. Administration → Extensions → Éditeurs approuvés : identifiant `local`, puis
   clé publique Base64 de l’étape 2. Approuver uniquement la clé de sa propre
   compilation ou celle d’un éditeur vérifiée par un canal de confiance.
2. « Installer un paquet local » : choisir `artifacts/tmdb.package.json`, puis
   « Vérifier et installer ». Le paquet est installé **désactivé**.
3. « Gérer cette extension » → « Paramètres du plugin » : renseigner la clé
   TMDb du fournisseur puis enregistrer.
4. Cocher les bibliothèques autorisées et « Enregistrer et activer ».
5. Rechercher une œuvre et l’exécuter pour obtenir une proposition conservée
   dans l’historique.

Le parcours manuel actuel lance `metadata.propose`. TMDb/IMDb implémentent cette
opération ; fanart.tv expose des opérations images/détails qui demandent leur
propre parcours Core. Le module Avatar expose des opérations d’avatars ; installer
un module ne suffit pas à ajouter automatiquement ces écrans ou changer les photos
des utilisateurs. Les mises à jour en place et les dépôts Jellyfin ne sont pas
disponibles. Aucun téléchargement massif d’affiches ni scan
n’est déclenché par l’installation.

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
| API du fournisseur | Paramètres de l’extension installée | Appeler TMDb/fanart.tv |
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
