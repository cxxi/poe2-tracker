# Jouer avec le jeu en français

État : **étude, pas commencé**. Voir aussi [ROADMAP](ROADMAP.md).

## Le but

Aujourd'hui le jeu doit tourner en anglais : Ctrl+C, Ctrl+Alt+C et le journal (`Client.txt`)
sont lus en anglais, et tout ce qui en dépend (trade, poe.ninja, comparaisons, équipement suivi,
route, mapping) travaille sur l'anglais. L'affichage, lui, est déjà en français ou en anglais
(réglage « Langue », Alt maintenu pour l'autre).

L'idée : accepter un jeu en français en **convertissant en anglais tout ce qui entre**, au
moment où ça entre. Le reste de l'app ne change pas : elle continue de recevoir de l'anglais,
et l'affichage repasse en français comme aujourd'hui.

- Langue de chaque copie détectée à son en-tête (« Item Class: » ou « Classe d'objet »), sans
  réglage : on peut changer la langue du jeu à tout moment.
- Une copie anglaise reste telle quelle.

## Ce qu'il faut convertir

### Texte copié (Ctrl+C, Ctrl+Alt+C) — `capture.rs` → `item::parse`

| Partie | Source de la traduction |
|---|---|
| En-tête (« Classe d'objet », « Rareté », « Niveau de l'objet »…) | `clientstrings.datc64` (identifiants `ItemDisplay*`, `ItemPopup*`) |
| Classe d'objet (« Bagues ») | `itemclasses.datc64`, colonne 8 |
| Type de base, gemme, monnaie, rune | `baseitemtypes.datc64`, colonne 32 |
| Nom d'unique | `words.datc64` : anglais colonne 4, français colonne 48 du fichier français |
| Propriétés (défenses, dégâts, attaques par seconde, charges, durée) | `clientstrings` (`ItemDisplayWeapon*`, `ItemDisplayArmour*`, `ItemDisplayFlask*`…) |
| Lignes de mods | descriptions de stats (`.csd`), gabarits dans l'autre sens |
| Pierres de téléportation (onglet Maps) | `mapping::Waystone::parse` : mêmes sources |

Les tables sont lues comme pour l'affichage (`src/gametext.rs`) : fichier anglais
`data/balance/X.datc64`, fichier français `data/balance/french/X.datc64`, mêmes lignes.

### Journal du jeu — `logwatch.rs`

Le journal est écrit dans la langue du jeu. Déjà compris en français : mort (« vient de
mourir »), AFK, arrivée et départ d'un joueur du groupe. À ajouter :

- montée de niveau (personnage, classe, niveau) ;
- entrée dans une zone (nom de zone en français : `worldareas.datc64`, colonne 8) ;
- récompenses de quête reçues (texte des bonus : lignes de mods) ;
- messages privés ;
- passifs de l'Atlas (le nom du passif est déjà lu en français dans le journal).

Le code de zone (« Generating level 16 area "G2_town" ») ne dépend pas de la langue.

## Comment

Le module `gametext` sait déjà passer de l'anglais au français :

- noms : table anglais → français ;
- lignes de mods : la ligne devient un squelette (nombres remplacés par `#`), le squelette
  retrouve le gabarit anglais, les valeurs vont aux mêmes places dans le gabarit français.

Il faut la même chose dans l'autre sens : un index des squelettes **français** vers les gabarits
anglais, et une table français → anglais pour les noms. Les deux sont construits à partir des
mêmes fichiers et peuvent partager le cache (`gametext_fr_vN.json`).

## Difficultés connues

- **Gabarits ambigus** : deux mods anglais peuvent avoir la même phrase française. Trancher avec
  les plages de valeurs, puis avec la classe d'objet ; sinon garder la ligne telle quelle.
- **Noms accordés en genre** : certains textes français portent un balisage
  `<if:MS>{…}<elif:FS>{…}…` (morceaux de noms de rares surtout). À retirer ou à résoudre.
- **Objets magiques** : le nom mêle base et affixes (« Bague de rubis de la Lueur ») ; il faut
  y retrouver le type de base (le plus long nom de base contenu dans la ligne).
- **Noms de rares** : tirés de morceaux de mots ; inutile de les convertir (affichage seulement).
- **Format exact du client français** : à vérifier sur de vraies copies — virgule ou point
  décimal, espace insécable avant « : », suffixes (« (implicite) », « (augmenté) »…), lignes
  longues coupées.
- **Collisions de noms** : un nom français partagé par deux noms anglais (rare) ; préférer la
  table la plus spécifique (base, unique, zone).

## À rassembler avant de commencer

- Une dizaine de copies faites avec le jeu en français :
  - un rare, un magique, un unique ;
  - une arme, une armure, un flacon, un charme ;
  - une monnaie, une gemme, une pierre de téléportation ;
  - le même objet en Ctrl+C et en Ctrl+Alt+C.
- Un extrait de `Client.txt` d'une courte session en français : entrée en zone, montée de
  niveau, mort, récompense de quête, message privé.

Ces exemples deviennent les tests de la conversion. Puis, comme pour l'affichage, mesurer la
part des lignes converties sur de vrais objets (test ignoré `gametext_from_game`, variable
`POE2_ITEMS`).
