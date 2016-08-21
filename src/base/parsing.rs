use kuchiki;
use kuchiki::{NodeRef, NodeDataRef, ElementData};
use kuchiki::iter::{Select, Elements, Descendants};
use kuchiki::traits::ParserExt;
use hyper::client::Response;

pub trait ResponseExt {
    fn parse_html(self) -> NodeRef;
}

impl ResponseExt for Response {
    fn parse_html(self) -> NodeRef {
        return kuchiki::parse_html().from_http(self).unwrap();
    }
}

pub trait NodeRefExt {
    fn query(&self, &str) -> Option<NodeDataRef<ElementData>>;
    fn query_all(&self, &str) -> Select<Elements<Descendants>>;
}

impl NodeRefExt for NodeRef {
    fn query(&self, selector: &str) -> Option<NodeDataRef<ElementData>> {
        self.select(selector).unwrap().next()
    }

    fn query_all(&self, selector: &str) -> Select<Elements<Descendants>> {
        self.select(selector).unwrap()
    }
}

pub trait ElementDataExt {
    fn get_attr(&self, &str) -> Option<String>;
}

impl ElementDataExt for ElementData {
    fn get_attr(&self, name: &str) -> Option<String> {
        self.attributes.borrow().get(name).map(String::from)
    }
}
