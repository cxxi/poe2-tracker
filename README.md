# poe2-tracker

Compagnon de bureau pour **Path of Exile 2** (Windows et Linux X11) : diagnostic des objets
copiés en jeu, suivi visuel de l'équipement porté, route de leveling et guide de build.

> Projet personnel, non affilié à Grinding Gear Games. L'outil ne touche jamais au jeu : il lit
> seulement son journal (`logs/Client.txt`) et le texte que tu copies avec Ctrl+C.

## Installation

Télécharge la dernière version dans [Releases](https://github.com/cxxi/poe2-tracker/releases/latest) :

- **Windows** : `poe2-tracker-windows.zip`. Extrais `poe2-tracker.exe` dans un dossier fixe et
  lance-le. Le programme n'étant pas signé, Windows peut afficher « Windows a protégé votre
  ordinateur » : *Informations complémentaires* → *Exécuter quand même*.
- **Linux** : `poe2-tracker-linux-x86_64`, à rendre exécutable (`chmod +x`) et à placer par
  exemple dans `~/.local/bin/poe2-tracker`.

Au premier lancement, un écran d'accueil cherche le jeu tout seul (bibliothèques Steam,
Proton, Flatpak, version autonome Windows) ou te laisse indiquer son dossier.

### Mises à jour

Au lancement puis toutes les 6 heures, poe2-tracker regarde s'il existe une version plus
récente sur GitHub. Si oui, un bouton apparaît dans la barre du bas : un clic télécharge la
nouvelle version, vérifie son empreinte SHA-256, remplace l'exécutable, puis propose de
redémarrer. Rien ne s'installe sans clic. `POE2_TRACKER_NO_UPDATE=1` désactive la vérification.

## En jeu

- **Ctrl+C** sur un objet : diagnostic.
  - Comparaison avec la pièce suivie au même emplacement (▲ MIEUX / ▼ MOINS BIEN /
    = ÉQUIVALENT), écarts de stats, case comparée allumée dans la grille. Pour les armes
    d'attaque, les **DPS de l'arme** passent avant tout le reste. Avec un build, une pièce
    dont la **base** ne lui convient pas (armure pour un build bouclier d'énergie, autre
    classe d'arme, bâton d'un autre élément) n'est jamais recommandée.
  - Conseil (garder, marché, PNJ), prix et confiance : poe.ninja pour devises et uniques ;
    pour les rares et magiques, **annonces comparables** en achat immédiat via l'API de trade
    officielle (même catégorie, tous les mods de l'objet entre 85 % et 150 % de leur valeur,
    au moins autant de mods, entre 85 % et 150 % des DPS ou de la défense de base). Deux
    recherches au plus : s'il y a trop peu d'annonces, les mods les plus faibles sont
    lâchés, leur place restant exigée. La valeur estimée est le prix médian des 5 annonces
    les moins chères, prix aberrants écartés ; les estimations sont gardées 24 h.
- **Regex marchands** (barre du bas) : copie une recherche faite d'après les mods que vise le
  build ; collée dans la recherche d'un marchand en jeu, elle allume les objets utiles.
- **Ctrl+Alt+C** sur une pièce portée : elle rejoint l'équipement suivi, disposé comme l'écran
  de personnage du jeu (survol = fiche, clic droit = ne plus suivre).
- **Route de leveling** (en bas) : contenus facultatifs de la campagne à ne pas rater
  (points de passif, résistances, esprit…), d'après `data/route.json`. Le journal du jeu
  annonce chaque récompense permanente : elle est cochée toute seule. Le reste se coche à la
  main ; ce qui a été oublié dans un acte précédent reste en rouge.
- **Fil d'événements** : niveaux, étapes du guide, morts, messages privés. Les coéquipiers
  (vus « rejoindre la zone ») ne sont jamais pris pour ton personnage.

Le diagnostic lit le texte copié par le jeu **en anglais**.

## Personnages et ligue

Le journal du jeu ne donne le nom du personnage qu'à une montée de niveau, une récompense ou
une mort : il apparaît dans la barre du haut dès la première de ces lignes. Chaque personnage
a son équipement suivi, son historique et son build. Le journal ne dit pas dans quelle ligue
il joue : elle se choisit par personnage (par défaut, la première ligue en cours).

## Builds

⚙ Menu → Gestionnaire de builds : colle le lien de la page du build, sur **Mobalytics** ou
**Kami-labs**. Le tracker récupère d'abord les fichiers `.build` (une étape par variante),
puis ce que le site donne en plus (runes recommandées) : si cette partie cesse de marcher,
les étapes restent. « Mettre à jour » relit la page (les guides évoluent).
Étapes reconnues : par acte (`Act 1`… `Endgame Step N`), par tranche de niveaux
(`1 - 21 - …`, `22 - End of Campaign - …`) et variantes d'endgame nommées (`BUDGET - …`),
au choix une fois la campagne finie. Avec un build, les verdicts suivent les objectifs de
l'étape en cours, et les onglets Sorts, Talents et Ascendances s'ouvrent. Sans build, le
diagnostic compare simplement à tes pièces.

## Images du jeu

Le fond de l'écran d'équipement et les icônes de l'arbre de talents sont **extraits de ton
installation du jeu** (`Bundles2` chez Steam, `Content.ggpk` en version autonome), en lecture
seule, puis gardés en cache. Ces images appartiennent à Grinding Gear Games : elles ne sont ni
dans ce dépôt ni dans les versions publiées.

## Confidentialité

- Lu sur ta machine : `logs/Client.txt` du jeu, et le presse-papiers **uniquement** dans la
  seconde qui suit un Ctrl+C. Pas de lecture mémoire du jeu, rien d'injecté.
- Réseau : poe.ninja (prix), API de trade officielle (annonces, liste des ligues), RePoE
  (illustrations des objets), GitHub (mises à jour), et le site d'un build quand tu en
  importes un.

## Données

| | Linux | Windows |
|---|---|---|
| Réglages | `~/.config/poe2-tracker/config.json` | `%APPDATA%\poe2-tracker\config` |
| Équipement, historique, builds | `~/.local/share/poe2-tracker/` | `%APPDATA%\poe2-tracker\data` |
| Cache (images, prix) | `~/.cache/poe2-tracker/` | `%LOCALAPPDATA%\poe2-tracker\cache` |

`config.json` : `{"league": "", "game_dir": "", "log_path": "", "market_min_ex": 10}`
(chemins vides = recherche automatique).

## Développement

```sh
cargo test
cargo build --release
```

Outils en ligne de commande : `--status` (état du guide), `--price <fichier>` (estimation d'un
objet copié dans un fichier), `--price-all <history.json>` (tout un historique, au rythme de
l'API), `--compare <fichier>`, `--vendor-regex` (regex marchands de chaque étape),
`--assets <filtre>` et
`--extract <chemin.dds> sortie.png` (archives du jeu).

Publier une version : mettre à jour `version` dans `Cargo.toml`, puis pousser un tag
`vX.Y.Z`. Le workflow `release.yml` compile et teste sous Linux et Windows, puis publie les
binaires, leurs empreintes et l'archive Windows.

## Licence

Code sous licence [MIT](LICENSE). Les images et données du jeu restent la propriété de
Grinding Gear Games.
