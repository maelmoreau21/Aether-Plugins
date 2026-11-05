# TheTVDB — fournisseur optionnel API v4

Le fournisseur utilise le parcours existant `metadata.search`, `metadata.details`
et `metadata.images`, avec les permissions signées `metadata.read`,
`metadata.write` et `images`. Il n'est ni installé ni activé par défaut.
La bibliothèque décide de ses autorisations et de sa priorité. Le Core reste
responsable de l'application explicite et de la protection des NFO/corrections.

## Accès et configuration

- `api_key` : clé de **projet** API v4 autorisée par TheTVDB, secrète et requise.
- `access_model` : `licensed` pour une licence de projet approuvée, ou
  `user_supported` si le projet est autorisé pour ce modèle.
- `subscriber_pin` : PIN personnel requis dans le second modèle, secret.
- `fallback_language` : langue de repli, `en` par défaut. Vide supprime ce
  second appel ; la valeur de base renvoyée par l'API demeure le dernier repli.
- `timeout_secs` : délai réseau borné à 1–6 secondes par appel, compatible avec
  le broker Wasm. Le Core possède aussi une limite d'exécution globale.

La langue préférée vient de la bibliothèque via le Core. Les variantes régionales
sont ramenées au code ISO 639-3 attendu par l'API (`fr-FR` → `fra`). La traduction
du titre et celle du résumé peuvent utiliser des replis différents. Le nom de base
ne constitue pas une preuve du titre original et n'est pas enregistré comme tel.

Aucune clé ou licence n'est intégrée. Un PIN d'abonné ne remplace pas la clé de
projet. Il n'autorise pas implicitement son partage entre les utilisateurs d'un
serveur : vérifier avec TheTVDB le modèle adapté à l'installation, notamment
multi-utilisateur. Le choix du formulaire n'accorde aucune licence.

## Identité et capacités retenues

L'identifiant TVDB fourni est prioritaire. À défaut, une identité IMDb exacte
utilise `/search/remoteid/{id}` avec filtrage strict du type demandé ; plusieurs
identités distinctes sont refusées. Les détails doivent confirmer l'identité TVDB
et, lorsqu'elle a servi de contexte, l'identité IMDb. Un identifiant TMDb seul
n'est jamais traité comme un identifiant TVDB.

Films et séries peuvent utiliser une recherche titre/année explicite, sans
appliquer le premier résultat automatiquement. Les épisodes sont pris en charge
par identifiant TVDB ou IMDb exact uniquement : le contrat SDK courant ne fournit
pas le contexte série/saison nécessaire à une résolution approchée fiable.
Le code ne crée ni Work ni fusion par titre.

Les détails retournent les champs disponibles (titre, résumé, date/année, durée,
genres, crédits et identités), sans inventer les champs absents. La popularité
TVDB `score` n'est pas une note sur dix. Les images sont sélectionnées selon
les types retournés par `/artwork/types`, la langue et les limites du SDK ;
seul `artworks.thetvdb.com` est accepté pour leurs URL. Leur import reste explicite
et contrôlé par le Core. Aucun téléchargement n'est écrit dans une bibliothèque.

## Réseau, cache et limites

L'authentification `/login` produit un bearer uniquement pour l'invocation. Il
n'est pas renvoyé ni conservé par le plugin. Aucun réseau hors des hôtes signés
`api4.thetvdb.com` et `artworks.thetvdb.com` ; pas de redirection ni proxy implicite.
Les réponses HTTP sont bornées à 256 Kio et les recherches à 20 candidats puis
60 Kio sérialisés. Les résultats du Core peuvent être mis en cache par son
pipeline existant selon opération/configuration/génération ; le plugin ne crée
pas un second cache ni une limite de quota inventée.

401/403, 404, 429/503, réponses partielles/invalides et ambiguïtés produisent des
codes bornés. Les erreurs brutes du service, clés, PIN et bearer ne sont pas
exposés. Les limites et délais effectifs du service demeurent ceux de la licence.
Les traductions 404 autorisent le repli ; une erreur d'authentification ou de
quota interrompt l'opération au lieu de se transformer en résultat vide.

## Validation et distribution

Voir [rapport local](../docs/TVDB_2026_11_03.md). Les tests utilisent des fixtures
hors ligne et le vrai guest Wasm ; ils ne prouvent pas l'accès au service avec une
licence réelle. Aucun paquet signé n'est publié par ce lot. Une distribution
nécessite reconstruction, signature, installation puis activation explicite.

Références officielles consultées le 3 novembre 2026 :
[API v4](https://github.com/thetvdb/v4-api),
[OpenAPI](https://github.com/thetvdb/v4-api/blob/main/docs/swagger.yml),
[modèles d'accès](https://support.thetvdb.com/kb/faq.php?id=81),
[PIN personnel](https://support.thetvdb.com/kb/faq.php?id=82),
[conditions](https://www.thetvdb.com/tos).
