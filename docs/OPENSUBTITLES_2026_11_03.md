# OpenSubtitles — premier fournisseur manuel, 3 novembre 2026

## Besoin confirmé et périmètre

Avant ce lot, aucun crate OpenSubtitles ni opération de sous-titres n'était
présent dans le workspace. Les options de bibliothèque étaient déclaratives.
Ce lot ajoute le fournisseur ; le parcours Core/interface/données est développé
séparément dans Aether. Il ne constitue pas à lui seul un parcours utilisateur complet.

## Contrats et fichiers

`Aether-Plugins-OpenSubtitles/{Cargo.toml,src/lib.rs,src/main.rs,README.md}` :
manifestes, configuration privée et opérations `subtitles.search` / `subtitles.download`.
Le README définit les champs JSON exacts. ABI Wasm v1 et révision SDK
`133896830c609a177e4c4c611084154d7e4aab5d` conservées.

`Cargo.toml` / `Cargo.lock` : nouveau membre et ses dépendances existantes.
`scripts/test-wasm.cjs`, `scripts/test-manifests.cjs` : inclusion du nouveau
fournisseur, explicitement en attente de première publication signée.
`scripts/test-subtitles-wasm.cjs` et CI : exécution du vrai guest avec broker synthétique.
Les modifications préexistantes SDK/collections et autres fournisseurs sont conservées.

## Comportement

Recherche IMDb, puis TMDb, puis titre/année ; filtres de langue, forced et SDH,
pagination bornée. Chaque fichier est un candidat distinct. Aucun hash simulé.
`identity` indique le critère de recherche, sans promettre la synchronisation.
Login facultatif, hôte API normal/VIP validé ; identifiants privés non renvoyés.
Le téléchargement retourne un lien temporaire privé : le Core doit l'autoriser,
le récupérer sans en-têtes fournisseur et conserver le dérivé hors médias.
Quotas remontés sans valeur gratuite ou illimitée présumée. Aucun retry automatique
d'une requête consommant du quota ; redirections et hôtes inconnus refusés.

## Vérifications locales réellement exécutées

- `cargo test --locked -p aether-plugin-opensubtitles` : 5 tests réussis.
- `cargo clippy --locked -p aether-plugin-opensubtitles --all-targets -- -D warnings` : réussi.
- Build release Wasm, optimisation Binaryen puis smoke ABI/limites/imports : réussi.
- `node scripts/test-subtitles-wasm.cjs` : recherche, login/VIP, téléchargement,
  quotas, 401/403/406/429/503, redirection, paramètres invalides et destination refusée réussis.
- `node scripts/test-manifests.cjs --allow-unpublished`, format ciblé et `git diff --check` : réussis.

Ces contrôles sont hors ligne. Aucun compte, accès service réel, signature de
paquet, publication, NAS ou déploiement validé. La taille est celle d'un build
local Windows, sans mesure de gain de temps ni benchmark comparatif.

## Limites, retour arrière et prochaine priorité

Le login est limité à l'invocation ; il n'est pas conservé dans le stockage privé
de l'extension. Le host doit dédupliquer avant consommation du quota, associer
le candidat au média autorisé et gérer les erreurs de téléchargement ambiguës.
L'exactitude de synchronisation exige une vérification de lecture réelle.
Première priorité : finir et vérifier le parcours Core/fiche média/lecteur.
Rollback local : retirer seulement ce crate, ses entrées workspace/lock et ses
nouveaux contrôles CI ; aucune migration ou écriture de média n'est créée ici.

Conditions consultées : [référence REST officielle](https://ai.opensubtitles.com/api-docs),
[démarrage](https://opensubtitles.tawk.help/article/getting-started),
[offres développeur](https://opensubtitles.tawk.help/article/pro-packages).
