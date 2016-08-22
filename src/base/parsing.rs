use kuchiki::{NodeRef, NodeDataRef, ElementData};
use kuchiki::iter::{Select, Elements, Descendants};

use base::Prime;

pub trait NodeRefExt {
    fn query(&self, &str) -> Prime<NodeDataRef<ElementData>>;
    fn query_all(&self, &str) -> Select<Elements<Descendants>>;
}

impl NodeRefExt for NodeRef {
    fn query(&self, selector: &str) -> Prime<NodeDataRef<ElementData>> {
        Ok(try!(self.select(selector).unwrap().next().ok_or(selector)))
    }

    fn query_all(&self, selector: &str) -> Select<Elements<Descendants>> {
        self.select(selector).unwrap()
    }
}

pub trait ElementDataExt {
    fn get_attr(&self, &str) -> Prime<String>;
}

impl ElementDataExt for ElementData {
    fn get_attr(&self, name: &str) -> Prime<String> {
        Ok(try!(self.attributes.borrow().get(name).map(String::from).ok_or(name)))
    }
}
