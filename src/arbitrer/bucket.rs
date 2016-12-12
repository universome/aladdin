use std::collections::HashMap;
use std::mem;
use std::collections::hash_map::Iter;

use markets::Offer;
use arbitrer::{MarkedOffer, HashableOffer};

pub struct Bucket(HashMap<HashableOffer, Vec<MarkedOffer>>);

impl Bucket {
    pub fn new() -> Bucket {
        Bucket(HashMap::new())
    }

    pub fn get_market(&self, offer: &Offer) -> Option<&[MarkedOffer]> {
        self.0.get(offer.as_ref()).map(Vec::as_slice)
    }

    pub fn update_offer(&mut self, marked: MarkedOffer) {
        if !self.0.contains_key(marked.1.as_ref()) {
            debug!("Event [{} by {}] is added", marked.1, marked.0.host);
            self.0.insert(HashableOffer(marked.1.clone()), vec![marked]);
            return;
        }

        let market = self.0.get_mut(marked.1.as_ref()).unwrap();
        let index = market.iter().position(|stored| stored.0 == marked.0);

        if let Some(index) = index {
            debug!("{} by {} is updated", marked.1, marked.0.host);
            market[index] = marked;
        } else {
            debug!("{} by {} is added", marked.1, marked.0.host);
            market.push(marked);
        }
    }

    pub fn remove_offer(&mut self, marked: &MarkedOffer) {
        let remove_market = {
            let market = match self.0.get_mut(marked.1.as_ref()) {
                Some(market) => market,
                None => return
            };

            let index = market.iter().position(|stored| stored.0 == marked.0);

            if let Some(index) = index {
                market.swap_remove(index);
                debug!("{} by {} is removed", marked.1, marked.0.host);
            } else {
                warn!("Can't remove non-existent offer {} by {}", marked.1, marked.0.host);
            }

            market.is_empty()
        };

        if remove_market {
            debug!("Event [{} by {}] is removed", marked.1, marked.0.host);
            self.0.remove(marked.1.as_ref());
        }
    }

    pub fn is_empty(&self) -> bool {
       self.0.is_empty()
    }

    pub fn iter(&self) -> Iter<Offer, Vec<MarkedOffer>> {
        unsafe { mem::transmute(self.0.iter()) }
    }
}
