# Extension Aether : Aether Avatars

Extension Rust officielle fournissant la gestion des packs d'avatars de profils, l'attribution aléatoire et les politiques administratives (inspirée de `jellyfin-avatars` et `jellyfin-plugin-GetAvatar`).

## Fonctionnalités
- **Packs d'avatars intégrés** :
  - `personas` : portraits vectoriels modernes avec styles et accessoires variés (10 avatars).
  - `geometric` : compositions abstraites et minimalistes aux dégradés soignés (10 avatars).
  - `retro` : avatars style pixel-art et nostalgie 8-bit (10 avatars).
  - `botts` : robots compagnons colorés (10 avatars).
- **Attribution aléatoire** :
  - Génération déterministe par utilisateur (seed) ou aléatoire.
  - Option d'attribution automatique obligatoire lors de la création d'un utilisateur.
- **Politique administrative** :
  - Option `disallow_custom_avatars` permettant à l'administrateur de verrouiller l'importation de photos personnalisées par les utilisateurs, pour garantir l'utilisation exclusive des packs officiels du serveur.
- **Support des dossiers personnalisés** :
  - L'administrateur peut pointer vers un dossier local contenant ses propres images PNG/SVG.

## Configuration
- `pack_name` (défaut `personas`) : Pack actif (`personas`, `geometric`, `retro`, `botts`, `all`).
- `force_random_on_creation` (défaut `true`) : Attribution aléatoire automatique aux nouveaux comptes.
- `disallow_custom_avatars` (défaut `false`) : Interdire aux utilisateurs d'importer leur propre image.
- `custom_pack_path` (optionnel) : Dossier contenant des avatars personnalisés.

## Commandes CLI
```powershell
# Manifeste
./target/release/aether-plugin-avatar.exe --manifest

# Lister les packs
./target/release/aether-plugin-avatar.exe packs

# Avatars d'un pack
./target/release/aether-plugin-avatar.exe avatars --pack geometric

# Obtenir un avatar aléatoire pour un utilisateur
./target/release/aether-plugin-avatar.exe random --pack personas --seed "user_123"
```

## Administration simplifiée dans le Core

La configuration présente trois options : avatar aléatoire pour les nouveaux
comptes, autoriser Gravatar et verrouiller les imports utilisateurs. Les images
se gèrent dans la galerie du Core : import depuis l’ordinateur ou depuis
https://github.com/cedev-1/jellyfin-avatars, aperçu, renommage des collections,
suppression individuelle et attribution aux comptes sans photo. Le choix d’un
pack génératif n’est plus présenté dans les réglages ni dans le profil Web.
Les anciennes opérations Wasm restent disponibles pour compatibilité.
Le Core applique ces préférences ; une publication signée est nécessaire pour
mettre à jour le schéma du catalogue distant.
