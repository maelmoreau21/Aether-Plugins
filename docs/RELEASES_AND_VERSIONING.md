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
| `intro-skipper` | `aether-plugin-intro-skipper` | `Aether-Plugins-IntroSkipper` | `intro-skipper` |
| `poster-rotator` | `aether-plugin-poster-rotator` | `Aether-Plugins-PosterRotator` | `poster-rotator` |

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
catalogue vérifié, et non de l'identifiant du plugin. Par exemple, `fanart-tv`
publie `fanart-0.1.0.package.json` sous `fanart-v0.1.0`. Une entrée dont le
paquet local manque fait échouer la publication au lieu d'être ignorée.
Après un échec entre la création du tag et celle de la Release, une relance
reprend la création de la Release sans recréer le tag. Le helper partagé
`scripts/publish-plugin-releases.cjs` vérifie aussi le fichier attaché : une
Release existante sans paquet reprend l'upload ; un paquet existant doit avoir
exactement la taille et le SHA-256 du fichier signé local. Aucun asset ou tag
existant n'est écrasé. Un tag sans Release pointant vers un autre commit bloque
la publication plutôt que d'être déplacé.

Les lectures temporairement indisponibles ne sont jamais assimilées à une
absence. Après un POST dont la réponse est ambiguë (erreur réseau, délai ou
HTTP 5xx), le helper recherche son effet avant toute nouvelle tentative. Les
retries sont bornés à cinq, avec attente exponentielle ; les erreurs permanentes
restent bloquantes. Un asset incomplet laissé par GitHub est signalé sans être
supprimé automatiquement.

---

## 4. Cycle de développement sur `develop`

Après une CI réussie sur un push `develop`, `plan-auto-release.py` compare les
sources au dernier commit du catalogue. Les plugins modifiés reçoivent une
version numérique distincte, sont compilés/signés puis enregistrés dans le
catalogue de `develop`. Le suffixe `-beta` appartient au tag GitHub (par exemple
`intro-skipper-v0.1.2-beta`), pas au manifeste signé. Le catalogue de `main`
conserve sa procédure de PR contrôlée.

La réconciliation des releases bêta s'exécute même sans nouveau paquet à
compiler : elle reprend les entrées actives du catalogue déjà signé, sans
incrémenter les versions ni signer de nouveau. Une nouvelle release pointe vers
le commit ayant enregistré son paquet, même si la reprise est déclenchée par
un commit ultérieur. Les signatures sont vérifiées avant cette réconciliation.

Ce parcours permet notamment de reprendre une panne GitHub après le push des
paquets et avant la fin des releases. Une relance historique sur un ancien SHA
ne remplace pas les versions déjà publiées ; pour utiliser un correctif du
workflow, déclencher la CI depuis le dernier `develop` contenant ce correctif.

---

## 5. Compatibilité Aether / Plugins

Chaque plugin déclare des métadonnées de compatibilité strictes dans son manifeste :
- `apiVersion` : Version du protocole de plugin (actuellement `1`).
- `targetAbi` : Version d'ABI WebAssembly ciblée (actuellement `1.0`).
- `minServerVersion` : Version minimale du serveur Aether requise (ex : `0.1.0`).
- `maxServerVersion` : Version maximale supportée (ex : `1.0.0`).

Une mise à jour d'Aether Core n'impose pas la recompilation de tous les plugins tant que l'ABI reste compatible.
