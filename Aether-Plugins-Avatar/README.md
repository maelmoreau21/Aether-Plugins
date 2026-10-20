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
