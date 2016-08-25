use std::io::Read;
use std::cell::RefCell;
use url::form_urlencoded::Serializer;
use hyper::client::{Client, Response};
use hyper::header::{Headers, SetCookie, Cookie, UserAgent, ContentLength, Accept, ContentType, qitem};
use hyper::status::StatusCode;
use kuchiki;
use kuchiki::NodeRef;
use kuchiki::traits::ParserExt;
use rustc_serialize::json::{self, Json};
use rustc_serialize::{Decodable, Encodable};

use base::Prime;

const USER_AGENT: &'static str = concat!("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_11_5) ",
                                         "AppleWebKit/537.36 (KHTML, like Gecko) ",
                                         "Chrome/52.0.2743.116 Safari/537.36");
pub struct Session {
    client: Client,
    base_url: String,
    cookie: RefCell<Cookie>
}

impl Session {
    pub fn new(base_url: &str) -> Session {
        Session {
            client: Client::new(),
            base_url: base_url.to_string(),
            cookie: RefCell::new(Cookie(vec![]))
        }
    }

    pub fn get(&self, path: &str, headers: Headers) -> Prime<Response> {
        self.request(path, None, headers)
    }

    pub fn post(&self, path: &str, body: &str, headers: Headers) -> Prime<Response> {
        self.request(path, Some(body), headers)
    }

    pub fn get_html(&self, path: &str) -> Prime<NodeRef> {
        let mut headers = Headers::new();
        headers.set(Accept(vec![qitem(mime!(Text/Html))]));

        let response = try!(self.get(path, headers));
        Ok(try!(kuchiki::parse_html().from_http(response)))
    }

    pub fn get_json<T: Decodable>(&self, path: &str) -> Prime<T> {
        let mut headers = Headers::new();
        headers.set(Accept(vec![qitem(mime!(Application/Json))]));

        let mut response = try!(self.get(path, headers));
        let mut payload = String::new();

        try!(response.read_to_string(&mut payload));

        Ok(try!(json::decode(&payload)))
    }

    pub fn get_raw_json(&self, path: &str) -> Prime<Json> {
        let mut headers = Headers::new();
        headers.set(Accept(vec![qitem(mime!(Application/Json))]));

        let mut response = try!(self.get(path, headers));
        Ok(try!(Json::from_reader(&mut response)))
    }

    pub fn post_form(&self, path: &str, data: &[(&str, &str)]) -> Prime<Response> {
        let encoded = Serializer::new(String::new())
            .extend_pairs(data)
            .finish();

        let mut headers = Headers::new();
        headers.set(ContentType(mime!(Application/WwwFormUrlEncoded)));
        headers.set(Accept(vec![qitem(mime!(Application/Json))]));

        self.post(path, &encoded, headers)
    }

    pub fn post_json<T: Encodable>(&self, path: &str, body: T) -> Prime<Response> {
        let encoded_body = try!(json::encode(&body));
        let mut headers = Headers::new();

        headers.set(ContentType(mime!(Application/Json)));
        headers.set(Accept(vec![qitem(mime!(Application/Json))]));

        self.post(path, &encoded_body, headers)
    }

    fn request(&self, path: &str, body: Option<&str>, mut headers: Headers) -> Prime<Response> {
        let mut url = self.base_url.clone();
        url.push_str(path);

        let builder = match body {
            Some(body) => self.client.post(&url).body(body),
            None => self.client.get(&url)
        };

        headers.set(self.cookie.borrow().clone());
        headers.set(UserAgent(USER_AGENT.to_owned()));

        if let Some(body) = body {
            headers.set(ContentLength(body.len() as u64));
        }

        let response = try!(builder.headers(headers).send());

        if response.status != StatusCode::Ok {
            return Err(From::from(format!("Bad status: {}", response.status)));
        }

        let cookies = response.headers.get::<SetCookie>()
            .map_or_else(Vec::new, |c| c.0.clone());

        *self.cookie.borrow_mut() = Cookie(cookies);

        Ok(response)
    }
}
