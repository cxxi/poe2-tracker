# Feuille de route

Ce qui est prévu ou envisagé, sans date. Ce qui est livré part dans le
[CHANGELOG](../CHANGELOG.md) ; une idée qui demande une étude a sa page dans `docs/`.

## Prévu

- **Jouer avec le jeu en français** : convertir en anglais les copies (Ctrl+C, Ctrl+Alt+C) et le
  journal du jeu quand le jeu est en français. Étude : [jeu-en-francais.md](jeu-en-francais.md).

## Chantier : serveur centralisé

Un petit serveur à nous, que l'app appelle pour ce qu'elle ne peut pas faire seule.

- **Textes des builds en français** : récupérer les blocs « Build Overview » et « Build
  Variants » de chaque build Mobalytics (l'import ne les lit pas encore), les afficher dans
  l'app, et les traduire en français. Le serveur garde la clé de l'API de traduction (DeepL de
  préférence) et met chaque traduction en cache, partagé entre tous : un texte n'est traduit
  qu'une fois, puis à nouveau seulement si l'auteur le modifie.
- **Maps en groupe synchronisées** : aujourd'hui chacun ne voit que son propre temps dans une map
  en groupe (l'app n'a que son journal). Avec le serveur, les apps d'un même groupe partagent la
  map en cours, ses temps et ses morts.

À trancher avant de commencer :

- hébergement et coût (fonctions serverless, petit VPS…) ;
- protection contre les abus sans compte (limite par installation, signature des requêtes) ;
- ce qui est envoyé et pourquoi : à dire dans le README (section Confidentialité), et l'app doit
  rester pleinement utilisable sans le serveur ;
- groupes : comment les apps d'un même groupe se reconnaissent (noms des coéquipiers vus dans
  le journal, code de groupe à partager…).

## Idées

- **Alt + clic sous Linux** : certains bureaux (Deepin, KDE…) réservent Alt + clic au
  déplacement des fenêtres, ce qui bloque la navigation pendant qu'Alt est maintenu pour
  l'autre langue. Piste : accepter aussi Maj maintenu (« [Alt] / [Maj] maintenu »).
- **Textes du jeu encore en anglais en français** : descriptions des monnaies et des omens,
  propriétés des gemmes (« Cost Multiplier », « Category »), certains textes d'ambiance.
- **Nombres décimaux des textes du jeu** : vérifier si le client français écrit « 1,5 » ou
  « 1.5 » et s'aligner.
- **Devises en majuscules partout** (« 10 EX »), comme dans le champ du seuil du marché.
