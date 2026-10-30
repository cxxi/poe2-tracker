# Feuille de route

Ce qui est prévu ou envisagé, sans date. Ce qui est livré part dans le
[CHANGELOG](../CHANGELOG.md) ; une idée qui demande une étude a sa page dans `docs/`.

## Prévu

- **Jouer avec le jeu en français** : convertir en anglais les copies (Ctrl+C, Ctrl+Alt+C) et le
  journal du jeu quand le jeu est en français. Étude : [jeu-en-francais.md](jeu-en-francais.md).

## Idées

- **Alt + clic sous Linux** : certains bureaux (Deepin, KDE…) réservent Alt + clic au
  déplacement des fenêtres, ce qui bloque la navigation pendant qu'Alt est maintenu pour
  l'autre langue. Piste : accepter aussi Maj maintenu (« [Alt] / [Maj] maintenu »).
- **Textes du jeu encore en anglais en français** : descriptions des monnaies et des omens,
  propriétés des gemmes (« Cost Multiplier », « Category »), certains textes d'ambiance.
- **Nombres décimaux des textes du jeu** : vérifier si le client français écrit « 1,5 » ou
  « 1.5 » et s'aligner.
- **Devises en majuscules partout** (« 10 EX »), comme dans le champ du seuil du marché.

## À vérifier

- **Focus après Ctrl+C** : la fenêtre de l'app prend parfois le focus après un Ctrl+C en jeu,
  ce qui empêche d'enchaîner les raccourcis. Le code ne demande jamais le focus ; piste : le
  gestionnaire de fenêtres ou le presse-papier.
