# IntroSkipper — horaires, coût et limites mesurés, 7 novembre 2026

Portée : le plugin `Aether-Plugins-IntroSkipper` (cette PR) et, dans le dépôt Aether, la branche
`fix/intro-skipper-fingerprint-timeline` (PR séparée, commit `7078288`). Aucune modification de l'ABI Wasm v1,
des permissions (`playback.markers`), du schéma de base de données ni des dossiers médias (lecture seule).

## Ce que l'audit a trouvé

Les points 1 à 5 ont été **mesurés** (empreintes Chromaprint réelles, module Wasm réel, runtime wasmi du Core).
Les points 6 à 8 sont des constats de lecture ou de mesure partielle, signalés comme tels.

| # | Constat | Effet mesuré |
|---|---|---|
| 1 | Le Node annonce 8,6133 empreintes/s (11025/1280) ; Chromaprint en produit 8,0769 (11025/1365) | Tous les horaires sont étirés de 6 % : fin d'ouverture 4,2 s trop tôt (médiane), début après un cold open jusqu'à 9,7 s trop tôt. **Aucune** des 47 ouvertures du corpus n'est à moins de 2 s avec l'ancien débit |
| 2 | La fenêtre de queue démarre 200 s avant la fin mais ne garde que les 1500 premières empreintes (185,7 s) | Les 14 s finales ne sont jamais analysées : fin de générique 14,3 s trop tôt (médiane) |
| 3 | Le Core envoie jusqu'à 8 épisodes (167 730 octets) à un invité qui accepte 65 536 octets | `input_too_large` pour toute saison de 4 épisodes ou plus : la détection acoustique ne peut pas aboutir |
| 4 | Trois épisodes complets consomment 299 M de carburant (plafond 200 M) | `execution_trapped` dans le runtime du Core, même avec trois épisodes |
| 5 | Les épisodes qui portent un débit stocké 8,6133 le renvoient tel quel au plugin | Corrigé côté Core (le débit est celui de l'algorithme) et côté plugin (lecture de l'ancienne valeur) |
| 6 | MP4 sans `faststart` : l'extraction échoue (0 octet, « partial file ») car le Node donne à ffmpeg un tube | Mesuré sur un MP4 synthétique de 25 min ; **non corrigé ici** |
| 7 | Une relance qui ne renvoie plus de marqueur d'intro laisse l'ancien marqueur automatique en place | Lecture du code (`persist_timeline_markers` ne fait que des upserts) ; non reproduit par un test ; **non corrigé ici** |
| 8 | Les marqueurs manuels sont dans une table distincte et prioritaire ; les marqueurs automatiques ne sont lus que si l'empreinte d'observation du fichier est inchangée | Conforme aux contrats ; couvert par les tests Core existants (`manual_marker_correction_…`, `marker_reanalysis_…`) |

## Changements

Plugin (cette PR) :
- Débit Chromaprint exact (`11025/1365`) ; l'ancienne valeur 8,6133 est lue comme le débit réel (`normalize_rate`), pour que
  le plugin soit juste avec un hôte non mis à jour.
- Le vote par diagonale utilise un tableau dense au lieu d'une table de hachage ; les seaux d'octets sont deux tableaux
  plats au lieu de 1024 vecteurs qui grandissent. Mêmes résultats sur tout le corpus (comparaison ligne à ligne), carburant ÷ 2,8.
  Les ex æquo sont départagés (diagonale la plus basse) : le résultat ne dépend plus de l'ordre d'itération d'une table de hachage.
- Corpus acoustique, banc de scénarios et vérification du vrai module Wasm.

Aether (PR séparée) :
- Débit exact dans le Node et dans le Core (qui ignore le débit stocké) ; une extraction en cache faite avec l'ancien débit est recalculée.
- Fenêtre de queue de 186 s qui se termine avec le fichier (1500 empreintes).
- Lots de 3 épisodes au plus (la cible puis les numéros les plus proches), bornés par le budget d'octets de la requête.
- Test de bout en bout du Node sur le vrai ffmpeg : la queue se termine à moins de 4,5 s de la fin du fichier (l'ancienne fenêtre : 14 s).

## Corpus

`tests/corpus/build-corpus.mjs` synthétise l'audio (aucun contenu protégé) : un « thème » est une mélodie déterministe, une
« scène » est du bruit rose sur un lit tonal discret de la même famille (négatif difficile). Chaque épisode est transformé comme
le Node le fait (mono 11025 Hz, `-f chromaprint -fp_format raw`, tête coupée à 2500, queue à 1500). Les fichiers `.bin`
(936 Kio) sont la source de vérité : un autre ffmpeg peut produire des bits légèrement différents.

| Scénario | Épisodes | Vérité |
|---|---|---|
| `standard` | 5 | ouverture A (60 s) dès t=0, générique C (60 s) à la fin |
| `cold_open` | 5 | cold open de 20 à 140 s avant l'ouverture |
| `recap` | 5 | récapitulatif de 38 à 70 s, jingle de 6 s partagé, puis ouverture |
| `opening_change` | 8 | ouverture A, puis B (75 s) à partir de l'épisode 5 |
| `no_intro` | 5 | aucune ouverture ; un générique |
| `studio_bumper` | 5 | aucune ouverture, mais un jingle de studio de 20 s commun (limite connue) |
| `special_episode` | 5 | l'épisode 3 est un spécial sans ouverture |
| `noisy` | 5 | ouverture sous un bruit croissant (amplitude 0,1 à 1,0, crête du thème 0,5) |
| `other_encode` | 5 | ouverture ré-encodée en AAC 40 kbit/s dans 3 épisodes sur 5 |
| `truncated_intro` | 5 | l'épisode 3 ne diffuse que 38 s sur 60 |
| `credits_variants` | 5 | générique seul, avec scène post-générique (45 s, 30 s), absent |

## Résultats (7 novembre 2026, Docker `rust:slim-bookworm`, rustc 1.99.1, ffmpeg 5.1.9)

Précision, tolérance 3 s sur les deux bornes (une empreinte résume environ 1,5 s d'audio avant son horodatage) :

| | Avant (débit annoncé 8,6133) | Après |
|---|---|---|
| Ouvertures retrouvées, 47 épisodes | 47 détectées, **0 à moins de 2 s** ; erreur médiane début 0–5,3 s, fin 4,2–8,0 s | **47 sur 47** ; erreur médiane début 0,0–1,5 s, fin 0,4–0,7 s |
| Faux négatifs (ouverture non rendue) | 0 | 0 |
| Faux positifs sans motif partagé (`no_intro`, épisode spécial) | 0 | 0 |
| Faux positifs avec jingle partagé (`studio_bumper`) | 5 sur 5 | 5 sur 5 (limite connue, confiance 0,99) |
| Crédits (22 épisodes avec générique) | début 9,7 s trop tôt, fin 25,9 s trop tôt (tête de queue 200 s) | début 1,1 s, fin 2,6 s trop tôt |

Lots réduits (fenêtres de 2, 3 et 5 épisodes consécutifs de chaque scénario) : ouvertures retrouvées 66/76 (87 %), 84/87 (97 %)
et 57/59 (97 %). Aucun faux positif hors `studio_bumper`. Avec deux épisodes, un spécial ou un changement d'ouverture dans la
fenêtre n'a pas de pair pour confirmer ; une ouverture raccourcie (38 s) ne raccourcit que la comparaison qui la contient.

Coût :

| Mesure | Avant | Après |
|---|---|---|
| Carburant du runtime du Core, 2 épisodes avec queues | 95 M | 36 M |
| Carburant, 3 épisodes avec queues (plafond 200 M) | 299 M (piégé) | **105 M** |
| Carburant, 8 épisodes (si la requête l'acceptait) | non mesuré | non mesuré, extrapolé à plus de 900 M : hors de portée |
| Temps natif release, 5 épisodes / 8 épisodes | 20–32 ms / 63–66 ms | 12–20 ms / 43 ms |
| Mémoire de tas native (pic d'une analyse) | 360 Kio | 80 Kio |
| Module Wasm optimisé | 200 217 octets | 199 044 octets (plafond 262 144) |
| Requête de 3 épisodes complets | 62 915 octets | idem (plafond 65 536) |
| Mémoire linéaire de l'invité, 3 épisodes (V8) | — | 1,4 Mio |

Ces temps sont ceux d'une machine de développement ; le coût sur un NAS n'est pas mesuré. Le plugin ne lance aucun traitement :
il compare des empreintes déjà extraites, et le Core abandonne l'analyse pendant une lecture (`playback_preempted`).

Contrôles : `cargo test -p aether-plugin-intro-skipper` (4 tests de corpus, 18 tests de contrat dont 1 ajouté, 1 banc historique),
`cargo clippy --all-targets -D warnings`, `cargo fmt --check`, `node scripts/test-intro-skipper-wasm.cjs` (vrai module, requête
au format du Core), et, dans Aether, 4 tests ajoutés dont l'essai de bout en bout du Node sur le vrai ffmpeg.

## Ce qui n'est pas établi

- **Aucune série réelle.** L'audio est synthétique (mélodies, bruit rose). Les ouvertures réelles ont des voix, des effets, des
  fondus ; les faux positifs d'une vraie bibliothèque (génériques de studio, musiques d'ambiance récurrentes, publicités) ne sont
  pas mesurés. Les 47 sur 47 ne valent pas une précision de production.
- Le bruit n'a pas été poussé jusqu'à la rupture : tous les niveaux testés (jusqu'à 1,0 contre une crête de thème de 0,5) sont détectés.
- Le temps wasmi est celui du Core de développement ; le coût sur un Node distant et sur un NAS n'est pas mesuré.
- Les empreintes déjà stockées en base gardent une queue arrêtée 14 s avant la fin tant que le fichier n'est pas ré-extrait (le
  débit stocké, lui, est corrigé à la lecture). Aucune ré-extraction massive n'est déclenchée.
- La version déclarée par `manifest()` (`0.1.0`) diffère de celle du paquet (`0.1.3`) : constat, non modifié.
- Aucun paquet signé n'est construit ni publié. Tant qu'une nouvelle version n'est pas publiée et installée, les installations
  gardent l'ancien module ; la valeur `package.sha256` change à ce moment-là, ce qui relance les analyses.

## Suites (hors de ces PR)

1. **MP4 sans faststart et seek par tube** (Node) : donner à ffmpeg une entrée relisible, sans élargir la liste blanche de
   protocoles. Le seek d'un tube lit le fichier jusqu'à la cible : à mesurer sur de gros fichiers distants.
2. **Marqueurs automatiques périmés après relance** (Core) : supprimer ceux qu'une nouvelle détection ne renvoie plus. Il faut
   d'abord stocker la méthode (chapitre ou empreinte) pour qu'une détection acoustique n'efface pas un marqueur de chapitre.
3. **Plus de trois épisodes par lot** : découper la détection en groupes puis fusionner, ou relever le plafond de carburant ;
   la requête de 64 Kio impose de toute façon trois épisodes avec queues.
4. **Corpus réel** : quelques saisons dont les ouvertures ont été annotées à la main, pour mesurer faux positifs et faux
   négatifs hors synthèse (jingles de studio compris).

Rollback : revert des commits ; aucune migration, aucune écriture dans les dossiers médias. Les caches du Node se recalculent seuls.
