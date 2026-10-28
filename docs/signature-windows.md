# Signature de l'exe Windows (à faire plus tard)

## Le problème

L'exe Windows n'est pas signé. Windows Defender l'a mis en quarantaine chez un utilisateur (0.2.7,
27/10/2026) avec la détection `Trojan:Win32/Bearfoos.A!ml`. Le suffixe `!ml` signale une détection
par apprentissage automatique, pas une signature de malware. C'est un faux positif classique pour
un exe Rust non signé, sans réputation (chaque version est un nouveau fichier) et qui se met à jour
lui-même (il télécharge un exe, remplace le sien et se relance, comme un « dropper »). Defender a
bloqué le programme pendant qu'il tournait, puis bloqué le zip dès son téléchargement.

En attendant, à chaque fausse alerte :
- côté utilisateur : Sécurité Windows → Historique de protection → Actions → « Autoriser sur
  l'appareil », et une exclusion sur le dossier de l'application ;
- côté projet : signaler le faux positif sur https://www.microsoft.com/en-us/wdsi/filesubmission
  (« Software developer », joindre l'exe, donner le nom de la détection).

## Les deux options

### SignPath Foundation (gratuit pour l'open source)

https://signpath.org/terms. Le certificat est au nom de SignPath Foundation, qui apparaît comme
éditeur. Il a déjà une réputation chez Microsoft, c'est ce qui calme Defender et SmartScreen.

Conditions :
- Licence OSI (MIT : OK), aucun composant propriétaire, projet maintenu, version déjà publiée.
- Binaire compilé depuis le dépôt par une CI vérifiable (GitHub Actions : OK).
- Double authentification sur GitHub (OK) et sur SignPath pour chaque membre.
- Rôles déclarés : auteurs, relecteurs, approbateurs (une seule personne peut tout tenir).
- Section « Code signing policy » sur la page d'accueil et la page de téléchargement, avec la
  mention de SignPath.io et SignPath Foundation, les rôles et la politique de confidentialité.
- Politique de confidentialité dès que l'application envoie des données vers des serveurs (GitHub
  pour les mises à jour, poe.ninja, API de trade), avec une option pour couper ces appels. Il faut
  une case à cocher dans l'application, pas seulement `POE2_TRACKER_NO_UPDATE`.
- Nom et version du produit cohérents dans les métadonnées de l'exe.
- **Approbation manuelle de chaque demande de signature** : ce n'est pas entièrement automatisable.
  La CI envoie l'exe, attend qu'un approbateur clique sur SignPath, puis récupère l'exe signé.
- Il faut une « réputation vérifiable » pour un exécutable, sans seuil donné. Acceptation à leur
  discrétion : un projet jeune peut être refusé.

Demande : https://signpath.org/apply

### Azure Trusted Signing (payant, entièrement automatisable)

Environ 10 $/mois. Le certificat est au nom du développeur après une vérification d'identité. La
signature se fait en CI sans intervention, avec l'action `azure/trusted-signing-action`. À
vérifier avant de choisir : l'ouverture aux particuliers (et non aux seules entreprises) en France.

## Ce qu'il faudra changer dans `.github/workflows/release.yml`

- Signer l'exe **avant** de calculer son empreinte SHA-256 et de construire le zip. La mise à jour
  intégrée vérifie l'empreinte, qui doit être celle de l'exe signé.
- SignPath : le job Windows envoie l'exe non signé en artefact, puis
  `signpath/github-action-submit-signing-request` (secret `SIGNPATH_API_TOKEN`, organisation,
  projet, politique `release-signing`, `wait-for-completion: true`) récupère l'exe signé une fois
  la demande approuvée. Ensuite viennent l'empreinte, le zip et la release.
- Azure : une étape `azure/trusted-signing-action` sur l'exe juste après `cargo build`, avant
  l'étape « Paquet ».

## Quand le faire

Après le gros chantier prévu sur le fonctionnement de l'application, pour faire la demande sur une
version plus définitive. Prévoir en même temps la page de confidentialité et l'option qui coupe les
appels réseau.
