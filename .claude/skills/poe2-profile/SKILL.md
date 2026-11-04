---
name: poe2-profile
description: Lire le profil public d'un personnage Path of Exile 2 publié par poe2-tracker (équipement, caractéristiques face au build, aptitudes, talents, joyaux, Atlas, maps, quêtes). À utiliser dès qu'on parle du personnage d'un joueur (le sien, celui d'un ami), de son équipement, de son build ou de ce qu'il doit améliorer, ou qu'un lien poe2-tracker.cxxi.io/v1/profiles/… est donné.
---

# Profil public poe2-tracker

L'app poe2-tracker (Rust, github.com/cxxi/poe2-tracker) publie, quand le joueur coche « Profil
public » dans ses réglages, le même contenu que son « Exporter le personnage » sur un serveur. Le
profil se met à jour tout seul tant que l'app tourne (au plus toutes les 30 secondes, quand il
change).

## Trouver le profil

1. **Regarde d'abord ta mémoire persistante** : un souvenir `poe2-profiles` liste les profils
   déjà connus (joueur, personnage, lien). Si le personnage demandé y est, utilise ce lien sans
   le redemander.
2. Sinon, demande au joueur le lien affiché dans les réglages de l'app (section « Profil
   public », bouton « Copier »).
3. **Dès qu'un nouveau lien t'est donné, enregistre-le dans ce souvenir `poe2-profiles`**
   (crée-le s'il n'existe pas, type `reference`, avec son pointeur dans `MEMORY.md`) : une ligne
   par personnage, `joueur — personnage (classe) — lien`. Mets-le à jour si un lien change, et
   retire une ligne dont le profil répond 404 de façon durable.

Un lien a la forme `https://poe2-tracker.cxxi.io/v1/profiles/<id>.md`, où `<id>` fait
26 caractères (minuscules a–z et chiffres 2–7). Un identifiant seul suffit à refaire le lien.

## Lire le profil

- **Commande `poe2-profile` (à préférer, si elle existe : `command -v poe2-profile`)** :
  `poe2-profile <id>` rend l'export entier, `poe2-profile <id> <section>` une seule section (clés
  plus bas : `gear`, `stats`, `passives`…), précédés de la date de mise à jour. Elle est autorisée
  sans demander : lance-la **seule**, telle quelle, sans tube (`|`), redirection ni autre commande
  dans le même appel, sinon chaque lecture demande une permission au joueur.
- **Markdown** : `https://poe2-tracker.cxxi.io/v1/profiles/<id>.md`, l'export complet (environ
  30 Ko).
  - Si tu as un shell avec `curl`, `curl -s <lien>` rend le texte exact. Seul, sans le passer à
    `python3` ou `jq` : lis la sortie toi-même.
  - Sinon, WebFetch : il répond par un petit modèle qui résume et refuse de tout recopier. Pose-
    lui une question précise qui sert la demande (« les mods exacts du casque », « le tableau
    porté / build des résistances », « les talents à prendre »), une par section utile, plutôt que
    de demander tout le texte. Pour un chiffre qui compte, redemande la ligne exacte.
- **JSON** : `https://poe2-tracker.cxxi.io/v1/profiles/<id>` →
  `{ id, updated_at, profile }`. `updated_at` : dernière mise à jour, en secondes Unix.
  `profile` : `format`, `lang` (« fr » ou « en » : langue de l'app du joueur), `app_version`,
  `character` (`name`, `class`, `level`, `progress`, `league`, `build` : `name`, `url`, `step`),
  `markdown` (l'export entier) et `sections` (`key`, `title`, `markdown`). Utile pour la date de
  mise à jour ou une seule section ; il contient l'export deux fois, il est donc plus long.

## Chercher sur le trade

Pour proposer des objets à acheter, la commande `poe2-trade` (si `command -v poe2-trade` la
trouve) interroge le site d'échange officiel en lecture seule, sans demander de permission :

- `poe2-trade stats <texte>` : identifiants des stats dont le texte contient `<texte>`
  (ex. `poe2-trade stats "also applies to elemental"`).
- `poe2-trade search '<requête JSON>' [n] [ligue]` : les `n` annonces les moins chères (6 par
  défaut, 10 au plus) de la requête du site (`{"query":{…},"sort":{"price":"asc"}}`), avec prix,
  propriétés et mods. Ligue par défaut : Forbidden Rites.

Mêmes règles que `poe2-profile` : lance-la **seule** (pas de `cd … &&`, de tube, de `sleep` ni
d'autre commande dans le même appel) et n'écris pas de script Python pour le faire. Espace les
recherches de quelques secondes : le site limite le débit. On ne fait que chercher : jamais
d'achat ni de message à un vendeur.

Lis le profil **à chaque question** sur le personnage : il change pendant que le joueur joue.
Ne garde pas son contenu en mémoire, seulement le lien.

## Ce que contient l'export

L'en-tête : classe, niveau, avancement (acte, interludes, endgame), ligue, build suivi (nom,
étape, lien de sa page). Puis les sections (`key` entre parenthèses) :

- **Équipement** (`gear`) : chaque pièce suivie, texte tel que copié en jeu (en anglais).
- **Caractéristiques** (`stats`) : porté face au build à l'étape affichée (attributs, vie, mana,
  esprit, défenses, résistances effectives avec la pénalité de l'acte), sources de chaque total,
  autres mods additionnés.
- **Aptitudes** (`skills`) : gemmes du build, ✓ si le joueur l'a, ✗ sinon. S'il n'a encore rien
  coché dans l'app, tout est ✗ : ne conclus pas qu'il n'a aucune gemme.
- **Talents** (`passives`) : son arbre réel (lu dans le journal du jeu ou déclaré à la main) par
  genre, et l'écart avec le build : à prendre, pris hors build. « Arbre réel inconnu » : l'app
  n'a pas son arbre complet.
- **Joyaux** (`jewels`), **Atlas** (`atlas`), **Maps** (`maps` : résumé et dernières maps),
  **Quêtes** (`quests` : récompenses obtenues et choix faits).

## Règles

- Lecture seule. N'appelle jamais les routes `/v1/me/…` (écriture, jeton secret de l'app) et ne
  demande jamais ce jeton.
- 404 : profil introuvable. Soit le lien est faux, soit le joueur a décoché « Profil public ».
  Dis-le, et demande-lui de le recocher ou de renvoyer le lien.
- Profil ancien (`updated_at` de plusieurs heures ou jours) : précise que l'app n'a peut-être pas
  tourné depuis, et que l'état réel peut différer.
- Ne devine pas ce qui manque. Une section vide ou absente, dis-le plutôt que d'inventer.
- Les textes du jeu sont en anglais (comme copiés en jeu) ; les textes de l'app suivent `lang`.
  Réponds dans la langue de ton interlocuteur.
