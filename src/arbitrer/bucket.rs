use std::ops::Deref;
use std::collections::HashMap;

use markets::Offer;
use arbitrer::MarkedOffer;

pub struct Bucket(HashMap<Offer, Vec<MarkedOffer>>);

impl Bucket {
    pub fn new() -> Bucket {
        Bucket(HashMap::new())
    }

    pub fn get_market(&self, offer: &Offer) -> Option<&[MarkedOffer]> {
        self.0.get(offer).map(Vec::as_slice)
    }

    pub fn update_offer(&mut self, marked: MarkedOffer) {
        if !self.0.contains_key(&marked.1) {
            debug!("Event [{} by {}] is added", marked.1, marked.0.host);
            self.0.insert(marked.1.clone(), vec![marked]);
            return;
        }

        let market = self.0.get_mut(&marked.1).unwrap();
        let index = market.iter().position(|stored| stored.0 == marked.0);

        if let Some(index) = index {
            if marked.1.outcomes.len() != market[index].1.outcomes.len() {
                error!("{} by {} is NOT updated: incorrect dimension", marked.1, marked.0.host);
                return;
            }

            debug!("{} by {} is updated", marked.1, marked.0.host);
            market[index] = marked;
        } else {
            if marked.1.outcomes.len() != market[0].1.outcomes.len() {
                error!("{} by {} is NOT added: incorrect dimension", marked.1, marked.0.host);
                return;
            }

            debug!("{} by {} is added", marked.1, marked.0.host);
            market.push(marked);
        }
    }

    pub fn remove_offer(&mut self, marked: &MarkedOffer) {
        let remove_market = {
            let market = match self.0.get_mut(&marked.1) {
                Some(market) => market,
                None => return
            };

            let index = market.iter().position(|stored| stored.0 == marked.0);

            if let Some(index) = index {
                market.swap_remove(index);
                debug!("{} by {} is removed", marked.1, marked.0.host);
            } else {
                warn!("There is no {} by {}", marked.1, marked.0.host);
            }

            market.is_empty()
        };

        if remove_market {
            debug!("Event [{} by {}] is removed", marked.1, marked.0.host);
            self.0.remove(&marked.1);
        }
    }
}

impl Deref for Bucket {
    type Target = HashMap<Offer, Vec<MarkedOffer>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
