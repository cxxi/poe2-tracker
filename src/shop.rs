//! Boutique du joueur : ses annonces sur le site de trade (recherche par compte vendeur), lues
//! de temps en temps et gardées sur le disque, par compte et par ligue. D'une lecture à l'autre :
//! les nouvelles annonces, les changements de prix, et celles qui ont disparu (vendues ou
//! retirées : le site ne dit pas lequel, le joueur peut le préciser).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::item::Item;
use crate::trade::{Listing, ShopSeen};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    /// En vente à la dernière lecture.
    #[default]
    Listed,
    /// Plus en vente : vendue ou retirée, on ne sait pas.
    Gone,
    /// Plus en vente, vendue d'après le joueur.
    Sold,
    /// Plus en vente, retirée d'après le joueur.
    Withdrawn,
}

/// Prix demandé à un moment (« 70 exalted »).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PricePoint {
    pub at: i64,
    pub amount: f64,
    pub currency: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub class: String,
    pub stash: String,
    pub listing: Listing,
    /// Mise en vente : la date la plus ancienne donnée par le site pour cette annonce (le site
    /// la change quand l'annonce est modifiée).
    pub listed_at: Option<i64>,
    /// Dernière date du site (dernière modification de l'annonce).
    pub indexed: Option<i64>,
    /// Prix successifs, le plus ancien d'abord.
    pub prices: Vec<PricePoint>,
    /// Dernière lecture où elle était en vente.
    pub last_seen: i64,
    pub status: Status,
    /// Première lecture où elle n'était plus en vente.
    pub gone_at: Option<i64>,
    /// Vente déjà envoyée au serveur de poe2-tracker.
    #[serde(default)]
    pub reported: bool,
}

impl Entry {
    /// L'annonce en objet, comme copié en jeu : icône, type, estimation.
    pub fn item(&self) -> Option<Item> {
        // Classe revue : celles gardées avant que les bases d'uniques (« Ezomyte Boots ») soient
        // reconnues.
        self.listing.as_item(&crate::trade::class_of(&self.class))
    }

    /// Le prix actuel (le dernier connu).
    pub fn price(&self) -> Option<&PricePoint> {
        self.prices.last()
    }

    /// Temps en vente : jusqu'à maintenant, ou jusqu'à sa disparition.
    pub fn listed_for(&self, now: i64) -> Option<i64> {
        let end = self.gone_at.unwrap_or(now);
        self.listed_at.map(|t| (end - t).max(0))
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    entries: Vec<Entry>,
    refreshed: Option<i64>,
    #[serde(default)]
    incomplete: bool,
}

pub struct Shop {
    path: PathBuf,
    pub account: String,
    pub league: String,
    pub entries: Vec<Entry>,
    /// Dernière lecture réussie (secondes Unix).
    pub refreshed: Option<i64>,
    /// Dernière lecture partielle (trop d'annonces pour les recherches du site) : rien n'a été
    /// marqué disparu.
    pub incomplete: bool,
}

/// Morceau de nom de fichier sûr (« cxxi#2432 » → « cxxi_2432 »).
fn file_part(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

impl Shop {
    pub fn open(data_dir: &Path, account: &str, league: &str) -> Shop {
        let path = data_dir.join("shop").join(format!("{}-{}.json", file_part(account), file_part(league)));
        let saved: Saved = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Shop { path, account: account.to_string(), league: league.to_string(), entries: saved.entries, refreshed: saved.refreshed, incomplete: saved.incomplete }
    }

    fn save(&self) {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let saved = Saved { entries: self.entries.clone(), refreshed: self.refreshed, incomplete: self.incomplete };
        let _ = std::fs::write(&self.path, serde_json::to_string(&saved).unwrap_or_default());
    }

    /// Une lecture du site : annonces nouvelles ou modifiées, et (lecture complète) celles qui
    /// ont disparu depuis la précédente, dont les noms sont rendus.
    pub fn apply(&mut self, seen: Vec<ShopSeen>, complete: bool, now: i64) -> Vec<String> {
        let mut gone = Vec::new();
        let ids: std::collections::HashSet<String> = seen.iter().map(|s| s.id.clone()).collect();
        for s in seen {
            let point = PricePoint { at: now, amount: s.listing.amount, currency: s.listing.currency.clone() };
            match self.entries.iter_mut().find(|e| e.id == s.id) {
                Some(e) => {
                    if e.prices.last().is_none_or(|p| p.amount != point.amount || p.currency != point.currency) {
                        e.prices.push(point);
                    }
                    e.listed_at = match (e.listed_at, s.indexed) {
                        (Some(a), Some(b)) => Some(a.min(b)),
                        (a, b) => a.or(b),
                    };
                    e.indexed = s.indexed.or(e.indexed);
                    e.class = s.class;
                    e.stash = s.stash;
                    e.listing = s.listing;
                    e.last_seen = now;
                    // Remise en vente après une disparition (ou un classement à la main).
                    e.status = Status::Listed;
                    e.gone_at = None;
                }
                None => self.entries.push(Entry {
                    id: s.id,
                    class: s.class,
                    stash: s.stash,
                    listed_at: s.indexed,
                    indexed: s.indexed,
                    prices: vec![PricePoint { at: s.indexed.unwrap_or(now), ..point }],
                    listing: s.listing,
                    last_seen: now,
                    status: Status::Listed,
                    gone_at: None,
                    reported: false,
                }),
            }
        }
        if complete {
            for e in self.entries.iter_mut().filter(|e| e.status == Status::Listed && !ids.contains(&e.id)) {
                e.status = Status::Gone;
                e.gone_at = Some(now);
                gone.push(if e.listing.name.is_empty() { e.listing.base.clone() } else { e.listing.name.clone() });
            }
        }
        self.incomplete = !complete;
        self.refreshed = Some(now);
        self.save();
        gone
    }

    /// Le joueur classe une annonce disparue : vendue, retirée, ou de nouveau à classer.
    pub fn set_status(&mut self, id: &str, status: Status) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id && e.status != Status::Listed) {
            e.status = status;
            self.save();
        }
    }

    /// Disparues, pas encore classées (vendues ou retirées ?).
    pub fn to_sort(&self) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.status == Status::Gone).collect()
    }

    /// Classées vendues ou retirées.
    pub fn history(&self) -> Vec<&Entry> {
        self.entries.iter().filter(|e| matches!(e.status, Status::Sold | Status::Withdrawn)).collect()
    }

    /// La vente est (ou n'est plus) sur le serveur.
    pub fn set_reported(&mut self, id: &str, reported: bool) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.reported = reported;
            self.save();
        }
    }

    /// Prochain envoi au serveur : une vente pas encore envoyée, ou une envoyée qui n'en est plus
    /// une (le joueur a changé d'avis).
    pub fn sale_to_sync(&self) -> Option<(String, Option<poe2_shared::Sale>)> {
        self.entries.iter().find_map(|e| match (e.status == Status::Sold, e.reported) {
            (true, false) => Some((e.id.clone(), Some(self.sale(e)))),
            (false, true) => Some((e.id.clone(), None)),
            _ => None,
        })
    }

    fn sale(&self, e: &Entry) -> poe2_shared::Sale {
        let l = &e.listing;
        let price = e.price();
        poe2_shared::Sale {
            league: self.league.clone(),
            item_class: e.class.clone(),
            rarity: l.rarity.clone(),
            name: l.name.clone(),
            base: l.base.clone(),
            item_level: l.item_level,
            properties: l.properties.clone(),
            implicits: l.implicits.clone(),
            mods: l.mods.iter().filter(|m| !l.implicits.contains(m)).cloned().collect(),
            amount: price.map_or(l.amount, |p| p.amount),
            currency: price.map_or(l.currency.clone(), |p| p.currency.clone()),
            listed_at: e.listed_at,
            indexed: e.indexed,
            gone_at: e.gone_at.unwrap_or(e.last_seen),
        }
    }

    /// En vente, les plus anciennes d'abord (celles qui ne partent pas).
    pub fn listed(&self) -> Vec<&Entry> {
        let mut v: Vec<&Entry> = self.entries.iter().filter(|e| e.status == Status::Listed).collect();
        v.sort_by_key(|e| e.listed_at.unwrap_or(i64::MAX));
        v
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(id: &str, amount: f64, indexed: i64) -> ShopSeen {
        ShopSeen { id: id.into(), indexed: Some(indexed), class: "Rings".into(), stash: "7".into(), listing: Listing { name: "Glyph Loop".into(), base: "Sapphire Ring".into(), amount, currency: "exalted".into(), ..Default::default() } }
    }

    #[test]
    fn trade_classes() {
        use crate::trade::class_of;
        assert_eq!(class_of("Body Armour"), "Body Armours");
        assert_eq!(class_of("Quarterstaff"), "Quarterstaves");
        assert_eq!(class_of("Boots"), "Boots");
        // Bases d'uniques : la classe qui termine le nom.
        assert_eq!(class_of("Ezomyte Boots"), "Boots");
        assert_eq!(class_of("Ezomyte Belt"), "Belts");
        assert_eq!(class_of("Vaal Spear"), "Spears");
    }

    #[test]
    fn listings_come_and_go() {
        let dir = std::env::temp_dir().join(format!("poe2-shop-test-{}", std::process::id()));
        let mut shop = Shop::open(&dir, "cxxi#2432", "Forbidden Rites");
        assert!(shop.apply(vec![seen("a", 70.0, 100), seen("b", 3.0, 200)], true, 1000).is_empty());
        assert_eq!(shop.listed().len(), 2);
        // Prix baissé (le site change sa date), « b » disparu.
        assert_eq!(shop.apply(vec![seen("a", 50.0, 900)], true, 2000), ["Glyph Loop"]);
        let a = &shop.listed()[0];
        assert_eq!((a.listed_at, a.prices.len(), a.price().unwrap().amount), (Some(100), 2, 50.0));
        assert_eq!(shop.to_sort().len(), 1);
        assert_eq!(shop.to_sort()[0].gone_at, Some(2000));
        assert_eq!(shop.to_sort()[0].listed_for(5000), Some(1800));
        // Lecture partielle : rien ne disparaît.
        assert!(shop.apply(vec![], false, 3000).is_empty());
        assert_eq!(shop.listed().len(), 1);
        shop.set_status("b", Status::Sold);
        assert!(shop.to_sort().is_empty());
        // Relu depuis le disque.
        let again = Shop::open(&dir, "cxxi#2432", "Forbidden Rites");
        assert_eq!(again.history()[0].status, Status::Sold);
        assert!(again.incomplete);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
