# What's new

## 0.6.2

- Passives: a jewel recommended by the build stays greyed out until something is socketed there. Hovering an allocated socket shows the jewel in it and the right click order to cycle.

## 0.6.1

- Passives: your tracked jewel shows in its socket with its real image, and its card on hover (mods included), whether or not the build recommends a jewel there.

## 0.6.0

- Passives: your actual tree, read from the game log, against the build's. Allocated in gold, to allocate in dashed gold, allocated outside the build in dark red; at the top, where you stand ("117 / 139 of the build") and a click to go to the passives to allocate or outside the build. A passive the build puts on a weapon set counts for that set.
- Stats computed as in game: base from level and class, life from Strength and mana from Intelligence, passive tree, quest rewards (resistances, spirit, life, Kaom's Lesson…) and increased percentages (life, mana, spirit, defences, attributes). Hovering a total shows each source and the calculation.
- Resistances shown as in game: effective, then the total before the cap in brackets.
- Jewels: Ctrl+Alt+C on a socketed jewel adds it to the tracked jewels, counted in the stats; Time-Lost jewels act on the passives allocated in their radius. In the tree, right click a socket to say which jewel is in it, and a badge on each jewel the build recommends: copied or forgotten.
- Export character (menu): gear, stats and their sources, passives and the gap with the build, jewels, Atlas, maps and quests, as you choose, in Markdown to paste into a conversation with an AI.
- Diagnosis: the image of a magic item whose suffix carries the name of another base ("of Convalescence") is the right one.

## 0.5.1

- Diagnosis: the image of some items no longer grows and shrinks in a loop (flickering display).

## 0.5.0

- Guide tab: the Mobalytics build author's texts (overview and the description of the step's variant), with headings, lists, links (YouTube in red, Twitch in purple) and game pills. Hovering a gem, an item or a passive shows its card. The texts are in English; update your build from the manager to get them.
- The step is only chosen in the top bar: the Stats, Skills, Passives and Guide tabs follow it. The league list moves between the character and the build.
- Market threshold in ex, chaos or div, in a single field (amount and currency); the settings lists take the full width.
- More complete French game texts: effects specific to each skill and gem descriptions, also on hover with Alt held.

## 0.4.0

- Interface language in the settings: French or English (the system language by default). Everything the app shows follows this choice; the game is still read in English (Ctrl+C, trade, poe.ninja).
- In French, game texts use the official translations read from the game files: item, unique, gem, rune and currency names, zones, classes and ascendancies, and mod lines (diagnosis, slot detail, listings, build recommendations, stats, waystones, passives). A line that is not recognised stays in English.
- Hold Alt in the app: everything switches to the other language while the key is held. The bottom bar shows this shortcut, apart from the game ones.
- French vocabulary aligned on the French version of the game.
- Levelling route, Atlas tree and what's new available in both languages; dates, percentages and decimal numbers written according to the language.

## 0.3.0

- Diagnosis rebuilt to answer "should I equip it?": the whole item (enlarged to fit the space), and at the top right the answer (better, worse, to check, build unique) with the score gap against your item. Two bands, "Compare to equipped" and "Compare to build", open the line-by-line comparison in large, over the whole window.
- Rings, charms and weapons compared at every slot: left / right, left / centre / right, set I / set II. One chip per slot in the bands, the best slot in the answer, each slot coloured by its answer.
- Uniques tab replaced by the slot detail: click an equipped item to see it against what the build recommends (✓ ~ ✗ line by line, match score, runes, recommended rarity read from Mobalytics, uniques with all their lines, price and trade search). The window grows with the available space.
- Item card on hovering a slot: the same layout (targeted mods in white, the others in grey, score). The star of an expected unique shows its card and price.
- Weapon lines as in game (damage coloured by element), with the DPS at the end of each damage line and the total (chaos included) after the attacks per second. Required level in red when it is above the character's. Flask and charm properties (life, mana, duration, charges), values changed by mods in blue.
- Inactive weapon set shown small at the edges of the grid, with its runes, as in game; switching sets swaps them with the large slots.
- Currency, runes, omens and other exchange market items: a detail screen instead of the diagnosis, with the price in large in ex, chaos and div, the price chart (7 days, 30 days or all), the traded volume and links to the trade site and poe.ninja. The same screen opens when clicking a line in the Market tab.
- Estimate detail: your whole item on the left; each listing with its weapon lines and DPS (gap against your weapon), its defences and its implicits in blue (gap against yours). Gaps written with a real minus sign. Re-estimate an item to see these details on its already read listings.
- Passives and Atlas in full screen, with a button to recentre (on the ascendancy for passives, without changing the zoom). The ascendancy sits at the centre of the tree with its artwork: the Ascendancies tab is gone. Jewels recommended by the build in their sockets (update your build to get them). Node tooltips with a fixed size whatever the zoom, round icons.
- Route: the quest rewards you picked are found in the game log and shown in green; those you can change have a short label at the end of the line and a small picker. Quest bonuses count (resistances, charm slot).
- Skills tab: the gem's weapon set as a badge at the end of the line.

## 0.2.9

- Mapping, at the bottom left once in endgame ("Route" button to go back to the levelling route): the current map with its timer, which counts the time spent in the map without hideout and AFK and pauses when back in town; deaths and town returns. Below, the session as a table: maps, play time, average time, maps per hour, deaths and returns.
- Maps tab on the right: key figures as cards, filters by tier and solo / group, deadliest waystone mods, history of all maps (waystone image of the tier, date, deaths, returns, duration; the detail and the waystone when hovering the line). A copied waystone (Ctrl+C) is linked to the next map opened.
- Solo or group: a map is a group map when you enter it without having used the device, or when another player enters or leaves it. In a group, the time shown is your own, not the whole group's.
- Atlas tab: the game's Atlas tree, browsable like the passive tree, with the allocated passives lit (from the game log) and their effects on hover.
- Bosses tab (experimental, to enable in the settings): the endgame bosses fought, with runs, deaths and average time.
- Route: what is already done is grouped by act.
- Durations in minutes and seconds.
- Interface: larger section titles and buttons, more readable mapping texts, fade at the bottom of the Market and Maps lists, Market search field at the height of the lists, top lists all the same colour, shorter activity header (class, level, zone).

## 0.2.8

- Settings: a settings page in the menu, saved with "Save". Game folder, trade estimate, estimate started on Ctrl+C (automatic by default) or with the button, detail or history opened on Ctrl+C, market threshold, automatic update check.
- Rare estimates rebuilt: each mod has a weight based on its market value, the item and each listing get a rating. The price comes from the trend of the 30 listings read, each weighing more the closer its rating is to the item's and the more recent it is. Confidence from the number of close and recent listings and the price spread; the range first when confidence is low. The least useful mods are dropped first.
- Estimate screen ("Details" button): price, search and criteria, source, and the listings read as cards (image, price, age, rating, share in the price, mods compared to yours). Re-estimate from the diagnosis or the detail, the previous estimate staying on screen until the new one.
- Diagnosis: "Estimate" card at the bottom, price at the top under the verdict, breadcrumb History › Diagnosis › Estimate. poe.ninja prices (uniques, currency) presented the same way, with a link to their poe.ninja page.
- Character tab: the bonuses of the worn gear added up, against what the build targets at the chosen step (implicits included), for the chosen weapon set. Resistances with the progression penalty and the 75% cap.
- Market tab on the right: poe.ninja prices by category, currency in its logical order, 7-day chart, search, league choice, refreshed every 15 minutes or on demand.
- History: "Estimate" button, price coloured by confidence, item type on the name line when there is room, alternating lines.
- Top bar: link to the build page, opened at the displayed step.
- Item card on hover: image of the item recommended by the build; unique star with image and market price.
- Ctrl+C brings back the Diagnosis tab, Ctrl+Alt+C the Equipment tab.

## 0.2.7

- Application icon, in the window, the taskbar and the applications menu (installed by the application itself on Linux).
- Update: a new version is checked every 5 minutes, the button appears without restarting.

## 0.2.6

- Skills tab: the weapon set to put each skill in (Set I, Set II or both), in the colour of the set in the Passives tab. Given by Mobalytics builds: update your build from the manager to get it.
- Diagnosis verdict (better, worse…) larger and more visible.

## 0.2.5

- Unique images: they were missing on a new install (the image index was built before the league was known, then kept for a week). It is now rebuilt as soon as the league is known, and by "Update catalogues".

## 0.2.4

- Builds added from the link of their page, on Mobalytics or Kami-labs: the steps, the recommended runes and the chosen quest rewards. Build manager to add them, update them from their page, reorder them, pick the followed one or delete them. Drag and drop of files is gone.
- Uniques recommended by the build: Uniques tab (price, trade search, variants using them), star on the slots concerned, badge in the diagnosis. An empty slot shows the recommended unique greyed out.
- Two weapon sets: Set I / Set II selector under the charms, shared with the Passives tab where each set's own passives are circled in its colour.
- Sockets and augments: socketed runes shown on items and recognised by name, empty sockets, runes recommended by the build, advice for empty sockets, lost runes flagged in the diagnosis.
- Two-column hover card: the item, and what the build recommends in that slot (base type, base defences, implicits, targeted lines marked ✓ ~ ✗, runes). The defences of armour pieces are compared as a total.
- Step selector: all the build steps, campaign included; another step than the current one is shown as a preview.
- Diagnosis in three parts: compared to your item, according to the build, then advice, price and confidence always visible at the bottom. Verdict at the top right, base defences and implicits of the item shown.
- The base counts: an item whose base does not suit the build (armour for an energy shield build, another weapon class, a staff of another element) is never recommended, and the reason comes first; a hybrid base is flagged. The implicits of the targeted base count, weapon damage is weighed by the build's damage type, and on a caster weapon spell levels and damage come first.
- Flasks: the amount recovered decides the verdict.
- More accurate price estimate: instant buyout listings only, outlier prices discarded, all the item's mods searched. If needed, the weakest are dropped but their slot is still required: an item with a weak mod is not worth the price of an item with an open slot. Mods are searched in a range around their value, and a caster weapon on the same base type. Without a recognised mod, the base price is estimated.
- More economical estimates: at most two searches per item, pace set on the limits announced by the server (other programs included), estimates kept 24 h from one session to the next.
- Prices shown in the most telling currency (divines, chaos or exalts), with the conversions in brackets, a range and a coloured confidence percentage.
- Advice, price and confidence shown with their label. An item "to sell to a vendor" that has a market price explains it: it is worth too little to be worth listing (10 ex threshold), and its price is in grey.
- History: item type, verdict as an icon before the name, price and confidence aligned right, fewer colours; on hover, a readable card with the verdict, the advice, the price in the three currencies and the confidence.
- Route: one line per step, details on hover (game texts in English), quest rewards chosen by the build, done steps ticked in green.
- Skills tab: description and effects of skills and supports on hover, type and level of the uncut gem, special gems (lineage) in colour, supports all horizontal or all vertical.
- "Vendor regex" button: copies a search made from the mods the build targets at the followed step; pasted into a vendor's search in game, it highlights the useful items.
- Cross to stop tracking an item, when hovering its slot.
- Menu redesigned: build manager, game folder, gear export (clipboard), catalogue update, what's new, clear history. This what's new window opens after an update.
- New look: tabs, cards for the route and the activity, top bar aligned on the frame with selectors sharing the width, consistent scrollbars, fades at the top and bottom of lists, shortcuts drawn as keyboard keys.
- Unique images in full resolution.
- Window: minimum size of 1200 × 1000.

## 0.2.3

- Faster item cards on hover, with an indicator while they open.

## 0.2.2

- Update: restarting after the install also works on Linux.

## 0.2.1

- MIT licence.

## 0.2.0

- First public version: diagnosis of copied items, tracked gear, levelling route, build guide and updates from GitHub.
