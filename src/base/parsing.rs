use kuchiki::{NodeRef, NodeDataRef, ElementData};
use kuchiki::iter::{Select, Elements, Descendants};

use base::error::{Result, Error};

pub trait NodeRefExt {
    fn query(&self, &str) -> Result<NodeDataRef<ElementData>>;
    fn query_all(&self, &str) -> Result<Select<Elements<Descendants>>>;
}

impl NodeRefExt for NodeRef {
    fn query(&self, selector: &str) -> Result<NodeDataRef<ElementData>> {
        let mut iter = try!(self.query_all(selector));
        iter.next().ok_or_else(|| Error::from(format!("No result for {}", selector)))
    }

    fn query_all(&self, selector: &str) -> Result<Select<Elements<Descendants>>> {
        self.select(selector).map_err(|_| {
            let message = format!("Strange error while searching for {}", selector);
            Error::from(message)
        })
    }
}

pub trait ElementDataExt {
    fn get_attr(&self, &str) -> Result<String>;
}

impl ElementDataExt for ElementData {
    fn get_attr(&self, name: &str) -> Result<String> {
        self.attributes.borrow().get(name)
            .map(String::from)
            .ok_or_else(|| {
                let message = format!("There is no \"{}\" attibute on <{}>", name, self.name.local);
                Error::from(message)
            })
    }
}
