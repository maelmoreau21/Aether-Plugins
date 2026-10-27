# Poster Rotator pour Aether

Extension inspirée des réglages de [Poster Rotator pour Jellyfin](https://github.com/maelmoreau21/jellyfin-plugin-poster-rotator). Aether conserve les images dans son stockage applicatif ; cette extension active et configure la sélection d'affiches par bibliothèque. Elle ne modifie pas les dossiers multimédias.

Après installation d'un paquet **signé**, ouvrir **Administration → Extensions → Poster Rotator**, choisir les bibliothèques, activer l'extension et enregistrer les paramètres. Les films et séries peuvent être activés séparément. Le choix peut être aléatoire ou séquentiel, avec délai minimal, taille de pool et préférences de langues.

Les images doivent déjà être présentes dans le catalogue. La collecte de nouvelles images reste l'action **Modifier les images → Enrichir les images**, avec TMDb/fanart.tv installés et configurés. Ce premier jalon ne porte pas encore les tâches de téléchargement automatique, de nettoyage et de purge de l'extension Jellyfin.

L'extension est dans le workspace, mais n'apparaîtra dans le catalogue public d'Aether qu'après compilation Wasm, signature et publication explicites. Pour un paquet local, utiliser `scripts/Build-LocalPackage.ps1 -Plugin poster-rotator -KeyPath <clé privée>` puis importer le paquet dans l'administration.
