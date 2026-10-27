# Guide du versionnement et des releases — Aether-Plugins

Ce document décrit le cycle de vie, la signature cryptographique Ed25519, le versionnement indépendant et la gestion sécurisée des releases pour les extensions d'**Aether**.

---

## 1. Principes et indépendance des plugins

Chaque plugin possède son **propre cycle de versions indépendant** :
- Une modification dans `tmdb` incrémente uniquement la version de TMDb (ex : `0.1.0` → `0.2.0`). Les autres plugins (`imdb`, `omdb`, `intro-skipper`, etc.) conservent rigoureusement leur version.
- **Tags Git propres au plugin** : format `<tag_prefix>-v<version>`, par exemple `tmdb-v0.2.0` ou `intro-skipper-v0.1.0-beta.1`.
- **Catalogue de production** : `repository/manifest.json` référence l'ensemble des extensions disponibles et leurs paquets signés dans `repository/packages/`.

Tableau des correspondances :

| Identifiant | Crate Rust | Répertoire | Préfixe Tag |
|---|---|---|---|
| `tmdb` | `aether-plugin-tmdb` | `Aether-Plugins-TMDb` | `tmdb` |
| `imdb` | `aether-plugin-imdb` | `Aether-Plugins-IMDb` | `imdb` |
| `omdb` | `aether-plugin-omdb` | `Aether-Plugins-OMDb` | `omdb` |
| `fanart-tv` | `aether-plugin-fanart` | `Aether-Plugins-fanart.tv` | `fanart` |
| `musicbrainz` | `aether-plugin-musicbrainz` | `Aether-Plugins-MusicBrainz` | `musicbrainz` |
| `discogs` | `aether-plugin-discogs` | `Aether-Plugins-Discogs` | `discogs` |
| `lrclib` | `aether-plugin-lrclib` | `Aether-Plugins-LrcLib` | `lrclib` |
| `avatars` | `aether-plugin-avatar` | `Aether-Plugins-Avatar` | `avatar` |
| `intro-skipper` | `aether-plugin-intro-skipper` | `Aether-Plugins-IntroSkipper` | `intro-skipper` |

---

## 2. Détection fine des changements (`scripts/detect_changed_plugins.py`)

L'outil d'analyse détecte avec précision les composants réellement impactés :
- **Fichiers sources du plugin** (`src/`, `Cargo.toml`, etc.) : déclenche la détection pour ce plugin uniquement.
- **Fichiers purement documentaires** (`.md`, `.txt`, dossier `docs/`) : ignorés, ne provoquent aucune release inutile.
- **Crate partagé `crates/aether-plugin-api/`** : si l'API commune est modifiée, seuls les plugins impactés sont reconstruits.
- **Immutabilité des paquets** : si un paquet existe déjà dans `repository/packages/`, tout écrasement conflictuel est rejeté.

---

## 3. Branches protégées et procédure de release sécurisée

Les branches `main` et `develop` sont protégées contre tout push direct. Le catalogue de production ne peut donc pas être modifié directement par un workflow de release sur `main`.

Le processus s'organise en deux étapes complémentaires :

### Étape A : Préparation via Pull Request (`prepare-plugin-release.yml`)
1. L'administrateur déclenche le workflow avec l'identifiant du plugin et l'incrément souhaité (`PATCH`, `MINOR`, `MAJOR`).
2. Le workflow s'exécute dans un environnement sécurisé avec accès au secret `ED25519_PRIVATE_KEY`.
3. Il crée une branche dédiée : `release/<plugin_id>-v<next_version>`.
4. Il compile le WebAssembly, optimise le Wasm, vérifie l'ABI et génère le manifeste.
5. Il signe le paquet avec la clé Ed25519 et le dépose dans `repository/packages/<short>-<version>.package.json`.
6. Il met à jour `repository/manifest.json` et vérifie son intégrité.
7. Il pousse la branche et ouvre automatiquement une **Pull Request contrôlée** ciblant `main`.

### Étape B : Publication après fusion (`release-plugins.yml`)
1. La Pull Request est revue et fusionnée dans `main`.
2. Le workflow sur `main` détecte les paquets intégrés dans le commit fusionné.
3. Il vérifie la validité des signatures et l'absence de tag Git existant.
4. Il crée le tag Git immutable sur le commit exact de fusion : `git tag -a tmdb-v0.2.0`.
5. Il crée la GitHub Release et y attache le fichier `.package.json` signé téléchargeable.
6. **Aucun `git push origin main` n'est exécuté** : toutes les modifications de fichiers ont été fusionnées préalablement par la PR !

Le fichier attaché et le préfixe du tag sont déduits de `packageUrl` dans le
catalogue vérifié, et non de l'identifiant du plugin. Par exemple, `avatars`
publie `avatar-0.1.0.package.json` sous `avatar-v0.1.0`, et `fanart-tv`
publie `fanart-0.1.0.package.json` sous `fanart-v0.1.0`. Une entrée dont le
paquet local manque fait échouer la publication au lieu d'être ignorée.
Après un échec entre la création du tag et celle de la Release, une relance
reprend la création de la Release existante sans recréer le tag.

---

## 4. Cycle de développement sur `develop`

À chaque fusion de PR sur `develop` :
- `detect_changed_plugins.py --channel develop` identifie les plugins modifiés.
- Le numéro de version bêta de chaque plugin modifié est incrémenté : par exemple `intro-skipper-v0.1.0-beta.0` → `intro-skipper-v0.1.0-beta.1`.
- Le tag Git correspondant est posé sur le commit exact.
- Les versions stables et le catalogue `repository/manifest.json` ne sont pas modifiés par les bêtas de développement.

---

## 5. Compatibilité Aether / Plugins

Chaque plugin déclare des métadonnées de compatibilité strictes dans son manifeste :
- `apiVersion` : Version du protocole de plugin (actuellement `1`).
- `targetAbi` : Version d'ABI WebAssembly ciblée (actuellement `1.0`).
- `minServerVersion` : Version minimale du serveur Aether requise (ex : `0.1.0`).
- `maxServerVersion` : Version maximale supportée (ex : `1.0.0`).

Une mise à jour d'Aether Core n'impose pas la recompilation de tous les plugins tant que l'ABI reste compatible.
