# Guide du versionnement et des releases — Aether-Plugins

La publication suit le principe des dépôts de plugins Jellyfin : **un seul
manifeste JSON** liste chaque extension avec un tableau `versions` contenant
toutes ses versions publiées. Une version précédente reste donc installable
depuis Aether, sans rien modifier au dépôt.

---

## 1. Le catalogue versionné (`repository/manifest.json`)

```json
{
  "apiVersion": 2,
  "plugins": [
    {
      "id": "tmdb",
      "name": "TMDb",
      "description": "…",
      "owner": "Aether Team",
      "category": "Metadata",
      "imageUrl": "https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/main/Aether-Plugins-TMDb/assets/banner.svg",
      "publisherId": "maelmoreau21",
      "publicKey": "0vhJ/lyWfViSATLvtFKhocq+bYoyUJuz58EsQvq5LXc=",
      "versions": [
        {
          "version": "0.1.3",
          "targetAbi": "1.0",
          "minServerVersion": "0.1.0",
          "maxServerVersion": "1.0.0",
          "sourceUrl": "https://github.com/maelmoreau21/Aether-Plugins/releases/download/tmdb-v0.1.3-beta/tmdb-0.1.3.package.json",
          "checksum": "<SHA-256 du fichier sourceUrl>",
          "timestamp": "2026-11-04T18:47:50Z",
          "changelog": "- …",
          "permissions": ["metadata.read", "metadata.write", "images"],
          "httpHosts": ["api.themoviedb.org"]
        }
      ]
    }
  ]
}
```

- Les versions sont triées de la plus récente à la plus ancienne et ne sont jamais
  retirées ni remplacées : une version publiée est immuable.
- `checksum` est le **SHA-256** (hexadécimal) du fichier téléchargé. Le MD5 des
  manifestes Jellyfin n'est pas accepté.
- `targetAbi` est l'ABI minimale requise (`1.0` aujourd'hui), comparée
  numériquement par le Core.
- `permissions` et `httpHosts` sont une copie d'affichage : le Core refuse
  l'installation s'ils ne correspondent pas au manifeste signé du paquet.
- `publisherId`/`publicKey` peuvent être répétés dans une version (rotation de clé ;
  c'est le cas de `poster-rotator@0.1.0`).
- La confiance vient uniquement de la signature Ed25519 du paquet.

Toutes les URL sont absolues et immuables, donc valables quelle que soit la
branche lue par le serveur : il n'y a plus de réécriture `main`/`develop`.

## 2. Publier une version

1. Modifier le plugin et augmenter sa version dans son `Cargo.toml`
   (`0.1.3` → `0.1.4`). Commit et push habituels (PR si la branche est protégée).
2. GitHub → **Actions → Release plugin → Run workflow** :
   - `plugin` : l'identifiant (`tmdb`, `fanart-tv`, …) ;
   - `version` : optionnel, sert de garde-fou (doit égaler `Cargo.toml`) ;
   - `changelog` : texte affiché dans Aether ;
   - `prerelease` : marque la release GitHub comme pré-version.

   Variante bêta : pousser le tag annoté `<préfixe>-v<x.y.z>-beta` sur le commit
   `develop` (`git tag -a tmdb-v0.1.4-beta -m "- Corrige …"`, puis
   `git push origin tmdb-v0.1.4-beta`). Le message du tag devient le changelog.
   Le tag sans suffixe déclenche une publication stable.
3. Le workflow `release.yml` :
   - compile le module Wasm, l'optimise et vérifie l'ABI ;
   - signe le paquet avec `scripts/sign-package.cjs` (aucune compilation du Core
     n'est nécessaire) et vérifie la signature avec `repository/publisher-public.txt` ;
   - crée la release `<préfixe>-v<version>[-beta]` avec l'asset `<préfixe>-<version>.package.json`
     (un asset existant n'est jamais remplacé : une relance le réutilise) ;
   - ajoute la version à `repository/manifest.json` sur `develop` pour une bêta,
     sur `main` pour une version stable (sauf variable `CATALOGUE_BRANCH` explicite), avec
     `scripts/add-catalogue-version.cjs`, en conservant toutes les versions.
     En cas de push concurrent, il repart de la branche cible et réessaie (5 fois).

Relancer le workflow pour une version déjà au catalogue ne fait rien.

## 3. Préfixes de tags et d'assets

| Identifiant | Répertoire | Préfixe |
|---|---|---|
| `tmdb` | `Aether-Plugins-TMDb` | `tmdb` |
| `tvdb` | `Aether-Plugins-TVDB` | `tvdb` |
| `imdb` | `Aether-Plugins-IMDb` | `imdb` |
| `omdb` | `Aether-Plugins-OMDb` | `omdb` |
| `fanart-tv` | `Aether-Plugins-fanart.tv` | `fanart` |
| `musicbrainz` | `Aether-Plugins-MusicBrainz` | `musicbrainz` |
| `discogs` | `Aether-Plugins-Discogs` | `discogs` |
| `lrclib` | `Aether-Plugins-LrcLib` | `lrclib` |
| `opensubtitles` | `Aether-Plugins-OpenSubtitles` | `opensubtitles` |
| `intro-skipper` | `Aether-Plugins-IntroSkipper` | `intro-skipper` |
| `media-requests` | `Aether-Plugins-Requests` | `media-requests` |
| `media-reports` | `Aether-Plugins-Reports` | `media-reports` |
| `poster-rotator` | `Aether-Plugins-PosterRotator` | `poster-rotator` |

Le préfixe est déduit des versions déjà publiées (`scripts/release-plan.cjs`).

## 4. Installer une version précédente

Dans Aether : **Administration → Extensions → Catalogue**, ouvrir l'extension :
la liste des versions affiche date, changelog et un bouton **Installer cette
version**. Pour une extension installée, une version plus ancienne passe par le
retour arrière confirmé : l'extension est désactivée, la version courante est
gardée comme sauvegarde restaurable, et le retour est refusé si l'extension
détient des données privées (`storage.private`) ou si les paramètres enregistrés
ne sont pas compatibles avec l'ancien schéma.

## 5. Vérifications locales

```powershell
node scripts/test-repository.cjs            # catalogue + paquets présents localement
node scripts/test-repository.cjs --online   # télécharge et vérifie chaque version
node scripts/test-manifests.cjs --allow-unpublished
node --test scripts/test-catalogue.cjs scripts/test-repository-history.cjs
```

`scripts/migrate-catalogue-v2.cjs` a produit le catalogue versionné à partir des
paquets historiques de `repository/packages/` et des releases existantes. Ces
paquets restent dans le dépôt pour mémoire ; les nouvelles versions n'y sont plus
committées.

## 6. Héritage

`prepare-plugin-release.yml`, `publish-develop-plugins.yml` et
`release-plugins.yml` ont été supprimés, ainsi que
`discover-plugin-releases.cjs` et `publish-plugin-releases.cjs` qui ne servaient
qu'à eux. Ils écrivaient le catalogue `apiVersion: 1` (dernière version
uniquement), committaient dans `develop` et ouvraient des PR avec `GITHUB_TOKEN`.
Ils restent consultables dans l'historique Git. `scripts/detect_changed_plugins.py`
et `plan-auto-release.py` sont conservés pour les tests de politique de version.
