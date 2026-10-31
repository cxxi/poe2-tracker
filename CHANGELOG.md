# Nouveautés

## 0.6.0

- Talents : ton arbre réel, lu dans le journal du jeu, face à celui du build. Pris en doré, à prendre en pointillé doré, pris hors build en rouge foncé ; en haut, où tu en es (« 117 / 139 du build ») et un clic pour aller aux talents à prendre ou hors build. Un talent que le build met sur un set d'armes compte pour ce set.
- Caractéristiques calculées comme en jeu : base du niveau et de la classe, vie par Force et mana par Intelligence, arbre de talents, récompenses de quête (résistances, esprit, vie, Kaom's Lesson…) et pourcentages augmentés (vie, mana, esprit, défenses, attributs). Au survol d'un total, chaque source et le calcul.
- Résistances affichées comme en jeu : effective, puis entre parenthèses le total avant plafond.
- Joyaux : Ctrl+Alt+C sur un joyau serti l'ajoute aux joyaux suivis, comptés dans les caractéristiques ; les Time-Lost agissent sur les talents pris dans leur rayon. Dans l'arbre, clic droit sur une châsse pour dire quel joyau y est, et une pastille sur chaque joyau conseillé par le build : copié ou oublié.
- Exporter le personnage (menu) : équipement, caractéristiques et leurs sources, talents et écart avec le build, joyaux, Atlas, maps et quêtes, au choix, en Markdown à coller dans une conversation avec une IA.
- Diagnostic : l'image d'un objet magique dont le suffixe porte le nom d'une autre base (« of Convalescence ») est la bonne.

## 0.5.1

- Diagnostic : l'image de certaines pièces ne grossit et ne rétrécit plus en boucle (affichage qui clignotait).

## 0.5.0

- Onglet Guide : les textes de l'auteur du build Mobalytics (présentation et description de la variante de l'étape), avec titres, listes, liens (YouTube en rouge, Twitch en violet) et pastilles du jeu. Au survol d'une gemme, d'une pièce ou d'un passif, sa fiche. Les textes sont en anglais ; mets ton build à jour depuis le gestionnaire pour les récupérer.
- L'étape se choisit seulement dans la barre du haut : les onglets Caractéristiques, Aptitudes, Talents et Guide la suivent. La liste des ligues passe entre le personnage et le build.
- Seuil du marché en ex, chaos ou div, dans un champ unique (montant et devise) ; les listes des réglages prennent toute la largeur.
- Textes du jeu en français plus complets : effets propres à chaque aptitude et descriptions des gemmes, aussi au survol avec Alt maintenu.

## 0.4.0

- Langue de l'interface au choix dans les réglages : français ou anglais (par défaut celle du système). Tout ce que l'app affiche suit ce choix ; le jeu reste lu en anglais (Ctrl+C, trade, poe.ninja).
- En français, les textes du jeu sont traduits avec les traductions officielles lues dans les fichiers du jeu : noms des objets, uniques, gemmes, runes, monnaies, zones, classes et ascendances, et lignes de mods (diagnostic, détail des cases, annonces, recommandations du build, caractéristiques, pierres de téléportation, passifs). Une ligne non reconnue reste en anglais.
- Alt maintenu dans l'app : tout passe dans l'autre langue le temps de l'appui. La barre du bas rappelle ce raccourci, à part de ceux du jeu.
- Vocabulaire aligné sur la version française du jeu : Bague, Flacon, Main secondaire, Armure, Score d'Évasion, Aptitudes, Pierre de téléportation, Noyaux d'âme, Gemmes brutes.
- Route de leveling, arbre de l'Atlas et nouveautés dans les deux langues ; dates, pourcentages et nombres décimaux écrits selon la langue.

## 0.3.0

- Diagnostic refait pour répondre à « je l'équipe ? » : la pièce entière (agrandie selon la place), et en haut à droite la réponse (mieux, moins bien, à voir, unique du build) avec l'écart de score face à ta pièce. Deux bandeaux, « Comparer à l'équipé » et « Comparer au build », ouvrent la comparaison ligne à ligne en grand, sur toute la fenêtre.
- Anneaux, charmes et armes comparés à chaque emplacement : gauche / droite, gauche / centre / droite, set I / set II. Une puce par emplacement dans les bandeaux, la meilleure place dans la réponse, chaque case colorée selon sa réponse.
- Onglet Uniques remplacé par le détail d'une case : clic sur une pièce de l'équipement pour la voir face à ce que recommande le build (✓ ~ ✗ ligne à ligne, score de correspondance, runes, rareté recommandée lue sur Mobalytics, uniques avec toutes leurs lignes, prix et recherche sur le trade). La fenêtre grandit avec la place disponible.
- Fiche au survol d'une case : la même présentation (mods visés en blanc, les autres en gris, score). L'étoile d'un unique attendu montre sa fiche et son prix.
- Lignes d'arme comme en jeu (dégâts colorés par élément), avec le DPS au bout de chaque ligne de dégâts et le total (dont le chaos) après les attaques par seconde. Niveau requis en rouge quand il dépasse celui du personnage. Propriétés des fioles et charmes (vie, mana, durée, charges), valeurs changées par les mods en bleu.
- Armes du set inactif en petit aux extrémités de la grille, avec leurs runes, comme en jeu ; changer de set les échange avec les grandes cases.
- Monnaies, runes, omens et autres objets du marché d'échange : un écran de détail à la place du diagnostic, avec le prix en grand en ex, chaos et div, la courbe du cours (7 jours, 30 jours ou tout), le volume échangé et les liens vers le trade et poe.ninja. Le même écran s'ouvre au clic sur une ligne de l'onglet Marché.
- Détail de l'estimation : ta pièce complète à gauche ; chaque annonce avec ses lignes d'arme et son DPS (écart face à ton arme), ses défenses et ses implicites en bleu (écart face aux tiens). Écarts écrits avec un vrai signe moins et une virgule. Réestime un objet pour voir ces détails sur ses annonces déjà lues.
- Talents et Atlas en plein écran, avec un bouton pour recentrer (sur l'ascendance pour les talents, sans changer le zoom). L'ascendance est au centre de l'arbre avec son illustration : l'onglet Ascendances disparaît. Joyaux recommandés par le build dans leurs emplacements (mets ton build à jour pour les avoir). Infobulles des nœuds de taille fixe quel que soit le zoom, icônes rondes.
- Route : les récompenses de quête choisies sont retrouvées dans le journal du jeu et montrées en vert ; celles qu'on peut changer ont un libellé court en bout de ligne et un petit choix. Les bonus de quête comptent (résistances, emplacement de charme).
- Onglet Sorts : le set des gemmes en badge en bout de ligne.

## 0.2.9

- Mapping, en bas à gauche une fois en endgame (bouton « Route » pour revoir la route de leveling) : la map en cours avec son chronomètre, qui compte le temps passé dans la map hors repaire et AFK et se met en pause au retour en ville ; morts et retours en ville. Dessous, la séance en tableau : maps, temps de jeu, temps moyen, maps par heure, morts et retours.
- Onglet Maps à droite : chiffres clés en cartes, filtres par tier et solo / en groupe, mods de waystone les plus mortels, historique de toutes les maps (image du waystone du tier, date, morts, retours, durée ; le détail et le waystone au survol de la ligne). Un waystone copié (Ctrl+C) est associé à la prochaine map ouverte.
- Solo ou en groupe : une map est en groupe quand on y entre sans avoir lancé le device, ou quand un autre joueur y entre ou en sort. En groupe, le temps affiché est le sien propre, pas celui de tout le groupe.
- Onglet Atlas : l'arbre de l'Atlas du jeu, navigable comme l'arbre des talents, avec les passifs posés allumés (d'après le journal du jeu) et leurs effets au survol.
- Onglet Boss (expérimental, à activer dans les réglages) : les boss d'endgame affrontés, avec passages, morts et temps moyen.
- Route : ce qui est déjà fait est regroupé par acte.
- Durées en minutes et secondes.
- Interface : titres de section et boutons plus grands, textes du mapping plus lisibles, fondu en bas des listes Marché et Maps, champ de recherche du Marché à la hauteur des listes, listes du haut toutes de la même couleur, en-tête de l'activité raccourci (classe, niveau, zone).

## 0.2.8

- Réglages : une page de réglages dans le menu, enregistrés avec « Enregistrer ». Dossier du jeu, estimation sur le trade, lancement de l'estimation au Ctrl+C (automatique par défaut) ou au bouton, ouverture du détail au Ctrl+C ou historique, seuil du marché, recherche automatique des mises à jour.
- Estimation des rares refaite : chaque mod a un poids selon ce qu'il vaut sur le marché, l'objet et chaque annonce ont une note. Le prix vient de la tendance des 30 annonces lues, chacune pesant d'autant plus que sa note est proche de celle de l'objet et qu'elle est récente. Confiance selon le nombre d'annonces proches et récentes, et l'éparpillement des prix ; fourchette d'abord quand la confiance est faible. Mods lâchés d'abord les moins utiles.
- Écran d'estimation (bouton « Détails ») : prix, recherche et critères, source, et les annonces lues en cartes (image, prix, ancienneté, note, part dans le prix, mods comparés aux tiens). Réestimer depuis le diagnostic ou le détail, l'ancienne estimation restant affichée jusqu'à la nouvelle.
- Diagnostic : carte « Estimation » en bas, prix en haut sous le verdict, fil d'Ariane Historique › Diagnostic › Estimation. Prix poe.ninja (uniques, monnaies) présentés de la même façon, avec un lien vers leur page poe.ninja.
- Onglet Caractéristiques : les bonus de l'équipement porté additionnés, face à ce que vise le build à l'étape choisie (implicites compris), pour le set d'armes choisi. Résistances avec la pénalité de la progression et le plafond de 75 %.
- Onglet Marché à droite : cours de poe.ninja par catégorie, monnaies dans leur ordre logique, courbe sur 7 jours, recherche, choix de la ligue, rafraîchi toutes les 15 minutes ou à la demande.
- Historique : bouton « Estimer », prix coloré selon la confiance, type de pièce sur la ligne du nom quand la place le permet, lignes alternées.
- Barre du haut : lien vers la page du build, ouverte sur l'étape affichée.
- Fiche au survol d'une pièce : image de la pièce recommandée par le build ; étoile des uniques avec image et prix au marché.
- Ctrl+C ramène sur l'onglet Diagnostic, Ctrl+Alt+C sur l'onglet Équipement.

## 0.2.7

- Icône de l'application, dans la fenêtre, la barre des tâches et le menu des applications (posée par l'application elle-même sous Linux).
- Mise à jour : une nouvelle version est cherchée toutes les 5 minutes, le bouton apparaît sans relancer.

## 0.2.6

- Onglet Sorts : le set d'armes où placer chaque sort (Set I, Set II ou les deux), dans la couleur du set de l'onglet Talents. Donné par les builds Mobalytics : mets ton build à jour depuis le gestionnaire pour l'avoir.
- Verdict du diagnostic (mieux, moins bien…) plus gros et plus visible.

## 0.2.5

- Images des uniques : elles manquaient sur une nouvelle installation (l'index des images était construit avant de connaître la ligue, puis gardé une semaine). Il est maintenant refait dès que la ligue est connue, et par « Mettre à jour les catalogues ».

## 0.2.4

- Builds ajoutés par le lien de leur page, sur Mobalytics ou Kami-labs : les étapes, les runes recommandées et les récompenses de quête choisies. Gestionnaire de builds pour en ajouter, les mettre à jour depuis leur page, les réordonner, choisir celui qui est suivi ou les supprimer. Le glisser-déposer de fichiers disparaît.
- Uniques recommandés par le build : onglet Uniques (prix, recherche sur le trade, variantes qui les utilisent), étoile sur les cases concernées, badge au diagnostic. Une case vide montre en grisé l'unique recommandé.
- Deux sets d'armes : sélecteur Set I / Set II sous les charmes, partagé avec l'onglet Talents où les passifs propres à chaque set sont entourés de leur couleur.
- Châsses et augments : runes posées affichées sur les pièces et reconnues par leur nom, châsses vides, runes recommandées par le build, conseils pour les châsses vides, runes perdues signalées au diagnostic.
- Fiche au survol en deux colonnes : la pièce, et ce que le build recommande à cet emplacement (type de base, défenses de base, implicites, lignes visées marquées ✓ ~ ✗, runes). Les défenses des pièces d'armure sont comparées en total.
- Sélecteur d'étape : toutes les étapes du build, campagne comprise ; une autre que l'actuelle s'affiche en aperçu.
- Diagnostic en trois parties : comparé à ta pièce, selon le build, puis conseil, prix et confiance toujours visibles en bas. Verdict en haut à droite, défenses de base et implicites de la pièce affichés.
- La base compte : une pièce dont la base ne convient pas au build (armure pour un build bouclier d'énergie, autre classe d'arme, bâton d'un autre élément) n'est jamais recommandée, et la raison est donnée en tête ; une base hybride est signalée. Les implicites de la base visée comptent, les dégâts d'une arme sont pesés selon le type de dégâts du build, et sur une arme de caster les niveaux et dégâts des sorts priment.
- Fioles : la quantité rendue décide du verdict.
- Estimation du prix plus juste : annonces en achat immédiat seulement, prix aberrants écartés, tous les mods de l'objet cherchés. S'il le faut, les plus faibles sont lâchés mais leur place reste exigée : un objet avec un mod faible ne vaut pas le prix d'un objet avec une place libre. Les mods sont cherchés dans une fourchette autour de leur valeur, et une arme de caster sur le même type de base. Sans mod reconnu, c'est le prix de la base qui est estimé.
- Estimation plus économe : deux recherches au plus par objet, rythme réglé sur les limites annoncées par le serveur (autres programmes compris), estimations gardées 24 h d'une session à l'autre.
- Prix affichés dans la devise la plus parlante (divines, chaos ou exalts), avec les conversions entre parenthèses, une fourchette et une confiance en pourcentage colorée.
- Conseil, prix et confiance présentés avec leur libellé. Un objet « à vendre au PNJ » qui a un prix au marché l'explique : il vaut trop peu pour la peine de le mettre en vente (seuil de 10 ex), et son prix est en gris.
- Historique : type de l'objet, verdict en icône devant le nom, prix et confiance alignés à droite, moins de couleurs ; au survol, une fiche lisible avec le verdict, le conseil, le prix dans les trois devises et la confiance.
- Route : une ligne par étape, détails au survol (textes du jeu en anglais), récompenses de quête choisies par le build, étapes faites cochées en vert.
- Onglet Sorts : description et effets des sorts et des supports au survol, type et niveau de la gemme non taillée, gemmes spéciales (lignée) en couleur, supports tous à l'horizontale ou tous à la verticale.
- Bouton « Regex marchands » : copie une recherche faite d'après les mods que vise le build à l'étape suivie ; collée dans la recherche d'un marchand en jeu, elle allume les objets utiles.
- Croix pour ne plus suivre une pièce, au survol de sa case.
- Menu repensé : gestionnaire de builds, dossier du jeu, export de l'équipement (presse-papier), mise à jour des catalogues, nouveautés, vider l'historique. Cette fenêtre de nouveautés s'ouvre après une mise à jour.
- Nouveau look : onglets, cartes pour la route et l'activité, barre du haut alignée sur le cadre et sélecteurs qui se partagent la largeur, barres de défilement homogènes, fondus en haut et en bas des listes, raccourcis en touches de clavier.
- Images des uniques en pleine résolution.
- Fenêtre : taille minimale de 1200 × 1000.

## 0.2.3

- Fiches des pièces au survol plus rapides, avec un indicateur le temps qu'elles s'ouvrent.

## 0.2.2

- Mise à jour : le redémarrage après installation fonctionne aussi sous Linux.

## 0.2.1

- Licence MIT.

## 0.2.0

- Première version publiée : diagnostic des objets copiés, équipement suivi, route de leveling, guide de build et mises à jour depuis GitHub.
