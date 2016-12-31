use std::ops::Deref;
use std::hash::{BuildHasher, Hasher, Hash};
use std::collections::hash_map::RandomState;
use parking_lot::{Mutex, MutexGuard};

use markets::Offer;
use arbitrer::matcher;
use arbitrer::MarkedOffer;


pub struct Table {
    rand_state: RandomState,
    entries: Box<[Mutex<Entry>]>
}

type Entry = Vec<Bucket>;

struct Bucket {
    badge: Offer,
    market: Vec<MarkedOffer>
}

pub struct MarketGuard<'a> {
    guard: MutexGuard<'a, Entry>,
    index: usize
}

impl<'a> Deref for MarketGuard<'a> {
    type Target = [MarkedOffer];

    #[inline]
    fn deref(&self) -> &[MarkedOffer] {
        &self.guard[self.index].market
    }
}

pub struct Iter<'a> {
    table: &'a Table,
    entry_index: usize,
    market_index: usize
}

impl<'a> Iterator for Iter<'a> {
    type Item = MarketGuard<'a>;

    fn next(&mut self) -> Option<MarketGuard<'a>> {
        while let Some(entry) = self.table.entries.get(self.entry_index) {
            let entry = entry.lock();

            if self.market_index >= entry.len() {
                self.entry_index += 1;
                self.market_index = 0;
            } else {
                let guard = MarketGuard {
                    guard: entry,
                    index: self.market_index
                };

                self.market_index += 1;

                return Some(guard);
            }
        }

        None
    }
}

impl Table {
    pub fn new(capacity: usize) -> Table {
        Table {
            rand_state: RandomState::new(),
            entries: (0..capacity)
                .map(|_| Mutex::new(Vec::new()))
                .collect::<Vec<_>>()
                .into_boxed_slice()
        }
    }

    pub fn get_market(&self, offer: &Offer) -> Option<MarketGuard> {
        let entry = self.get_entry(offer);
        let index = entry.iter().position(|bucket| matcher::compare_offers(offer, &bucket.badge));

        index.map(|index| MarketGuard { guard: entry, index: index })
    }

    pub fn update_offer(&self, marked: MarkedOffer) -> usize {
        let mut entry = self.get_entry(&marked.1);

        if let Some(bucket) = entry.iter_mut().find(|b| matcher::compare_offers(&marked.1, &b.badge)) {
            let market_len = bucket.market.len();
            debug_assert!(market_len > 0);

            if let Some(stored) = bucket.market.iter_mut().find(|stored| stored.0 == marked.0) {
                debug!("{} by {} is updated", marked.1, marked.0.host);
                *stored = marked;

                return market_len;
            }

            debug!("{} by {} is added", marked.1, marked.0.host);
            bucket.market.push(marked);

            return market_len + 1;
        }

        debug!("Market [{} by {}] is added", marked.1, marked.0.host);

        entry.push(Bucket {
            badge: marked.1.clone(),
            market: vec![marked]
        });

        1
    }

    pub fn remove_offer(&self, marked: &MarkedOffer) {
        let mut entry = self.get_entry(&marked.1);

        let market_index = match entry.iter().position(|b| matcher::compare_offers(&marked.1, &b.badge)) {
            Some(index) => index,
            None => {
                warn!("Cannot remove non-existent offer {} by {}: no suitable market",
                      marked.1, marked.0.host);
                return;
            }
        };

        let remove_market = {
            let market = &mut entry[market_index].market;

            let index = match market.iter().position(|stored| stored.0 == marked.0) {
                Some(index) => index,
                None => {
                    warn!("Cannot remove non-existent offer {} by {}", marked.1, marked.0.host);
                    return;
                }
            };

            debug!("{} by {} is removed", marked.1, marked.0.host);

            if market.len() > 1 {
                market.swap_remove(index);
                false
            } else {
                true
            }
        };

        if remove_market {
            debug!("Market [{}] is removed", entry[market_index].badge);
            entry.remove(market_index);
        }
    }

    pub fn iter(&self) -> Iter {
        Iter {
            table: self,
            entry_index: 0,
            market_index: 0
        }
    }

    fn get_entry(&self, offer: &Offer) -> MutexGuard<Entry> {
        let state = &mut self.rand_state.build_hasher();

        matcher::round_date(offer.date).hash(state);
        offer.game.hash(state);
        offer.kind.hash(state);
        offer.outcomes.len().hash(state);

        let hash = state.finish();

        self.entries[hash as usize % self.entries.len()].lock()
    }
}
