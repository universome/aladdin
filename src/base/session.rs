use std::io::Read;
use std::time::Duration;
use std::sync::Mutex;
use url::form_urlencoded::Serializer as UrlSerializer;
use hyper::client::{Client, RedirectPolicy, Response};
use hyper::header::{Headers, SetCookie, Cookie, UserAgent, Accept, ContentType, qitem};
use kuchiki;
use kuchiki::NodeRef;
use kuchiki::traits::ParserExt;
use serde::{Serialize, Deserialize};
use serde_json as json;

header! { (XRequestedWith, "X-Requested-With") => [String] }

use base::error::Result;

const USER_AGENT: &str = concat!("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_11_5) ",
                                 "AppleWebKit/537.36 (KHTML, like Gecko) ",
                                 "Chrome/52.0.2743.116 Safari/537.36");
pub struct Session {
    client: Client,
    base_url: String,
    cookie: Mutex<Cookie>
}

impl Session {
    pub fn new(base_url: &str) -> Session {
        let mut client = Client::new();
        client.set_read_timeout(Some(Duration::from_secs(25)));
        client.set_write_timeout(Some(Duration::from_secs(25)));
        client.set_redirect_policy(RedirectPolicy::FollowNone);

        Session {
            client: client,
            base_url: base_url.to_string(),
            cookie: Mutex::new(Cookie(vec![]))
        }
    }

    pub fn get(&self, path: &str, headers: Headers) -> Result<Response> {
        self.request(path, None, headers)
    }

    pub fn post(&self, path: &str, body: &str, headers: Headers) -> Result<Response> {
        self.request(path, Some(body), headers)
    }

    pub fn get_text(&self, path: &str) -> Result<String> {
        let mut headers = Headers::new();
        headers.set(Accept(vec![qitem(mime!(Text/Plain))]));

        let mut response = try!(self.get(path, headers));

        let mut string = String::new();
        try!(response.read_to_string(&mut string));

        Ok(string)
    }

    pub fn get_html(&self, path: &str) -> Result<NodeRef> {
        let mut headers = Headers::new();
        headers.set(Accept(vec![qitem(mime!(Text/Html))]));

        let response = try!(self.get(path, headers));
        Ok(try!(kuchiki::parse_html().from_http(response)))
    }

    pub fn get_raw_html(&self, path: &str) -> Result<String> {
        let mut headers = Headers::new();
        headers.set(Accept(vec![qitem(mime!(Text/Html))]));

        let mut response = try!(self.get(path, headers));
        let mut string = String::new();

        try!(response.read_to_string(&mut string));

        Ok(string)
    }

    pub fn get_json<T: Deserialize>(&self, path: &str) -> Result<T> {
        let mut headers = Headers::new();
        headers.set(Accept(vec![qitem(mime!(Application/Json))]));

        let response = try!(self.get(path, headers));

        Ok(try!(json::from_reader(response)))
    }

    pub fn get_raw_json(&self, path: &str) -> Result<json::Value> {
        let mut headers = Headers::new();
        headers.set(Accept(vec![qitem(mime!(Application/Json))]));

        let mut string = String::new();
        let mut response = try!(self.get(path, headers));

        try!(response.read_to_string(&mut string));

        Ok(try!(json::from_str(&string)))
    }

    pub fn post_form(&self, path: &str, data: &[(&str, &str)]) -> Result<Response> {
        let encoded = UrlSerializer::new(String::new())
            .extend_pairs(data)
            .finish();

        let mut headers = Headers::new();
        headers.set(ContentType(mime!(Application/WwwFormUrlEncoded)));
        headers.set(Accept(vec![qitem(mime!(Application/Json))]));
        headers.set(XRequestedWith("XMLHttpRequest".to_owned()));

        self.post(path, &encoded, headers)
    }

    pub fn post_json<T: Serialize>(&self, path: &str, body: T) -> Result<Response> {
        // TODO(loyd): what about using `json::to_write()`?
        let encoded_body = try!(json::to_string(&body));
        let mut headers = Headers::new();

        headers.set(ContentType(mime!(Application/Json)));
        headers.set(Accept(vec![qitem(mime!(Application/Json))]));

        self.post(path, &encoded_body, headers)
    }

    fn request(&self, path: &str, body: Option<&str>, mut headers: Headers) -> Result<Response> {
        let mut url = self.base_url.clone();
        url.push_str(path);

        debug!("{} {}", if body.is_some() { "POST" } else { "GET" }, url);

        let builder = match body {
            Some(body) => self.client.post(&url).body(body),
            None => self.client.get(&url)
        };

        let cookie = self.cookie.lock().unwrap().clone();

        headers.set(cookie);
        headers.set(UserAgent(USER_AGENT.to_owned()));

        let response = try!(builder.headers(headers).send());

        if !response.status.is_success() && !response.status.is_redirection() {
            return Err(From::from(response.status));
        }

        if let Some(cookies) = response.headers.get::<SetCookie>() {
            let mut stored = self.cookie.lock().unwrap();

            for c in &cookies.0 {
                let option = stored.iter().position(|x| c.name == x.name && c.domain == x.domain);

                if let Some(index) = option {
                    stored[index] = c.clone();
                } else {
                    stored.push(c.clone());
                }
            }
        }

        Ok(response)
    }
}
