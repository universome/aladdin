use std::io::Read;
use std::io::ErrorKind::{WouldBlock, TimedOut};
use std::time::Duration;
use std::sync::Mutex;
use url::form_urlencoded::Serializer as UrlSerializer;
use hyper::error::{Error as HyperError, Result as HyperResult};
use hyper::client::{Client, RedirectPolicy, Response};
use hyper::header::{Headers, SetCookie, Cookie, UserAgent, Accept, ContentType, qitem};
use kuchiki;
use kuchiki::NodeRef;
use kuchiki::traits::ParserExt;
use serde::{Serialize, Deserialize};
use serde_json as json;

use base::error::{Result, Error};

header! { (XRequestedWith, "X-Requested-With") => [String] }

const MAX_RETRIES: u32 = 2;
const READ_TIMEOUT: u64 = 20;   // We should set large timeout due to the long-polling.
const WRITE_TIMEOUT: u64 = 5;

const USER_AGENT: &str = concat!("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_11_5) ",
                                 "AppleWebKit/537.36 (KHTML, like Gecko) ",
                                 "Chrome/52.0.2743.116 Safari/537.36");
pub struct Session {
    client: Client,
    host: String,
    cookie: Mutex<Cookie>
}

impl Session {
    pub fn new(host: &str) -> Session {
        let mut client = Client::new();
        client.set_read_timeout(Some(Duration::from_secs(READ_TIMEOUT)));
        client.set_write_timeout(Some(Duration::from_secs(WRITE_TIMEOUT)));
        client.set_redirect_policy(RedirectPolicy::FollowNone);

        Session {
            client: client,
            host: host.to_string(),
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

    pub fn post_form(&self, path: &str, data: &[(&str, &str)],
                     addl_headers: &[(&'static str, &str)]) -> Result<Response>
    {
        let encoded = UrlSerializer::new(String::new())
            .extend_pairs(data)
            .finish();

        let mut headers = Headers::new();
        headers.set(ContentType(mime!(Application/WwwFormUrlEncoded)));
        headers.set(Accept(vec![qitem(mime!(Application/Json))]));
        headers.set(XRequestedWith("XMLHttpRequest".to_owned()));

        for &(name, value) in addl_headers {
            headers.set_raw(name, vec![value.as_bytes().to_vec()]);
        }

        self.post(path, &encoded, headers)
    }

    pub fn post_json<T: Serialize>(&self, path: &str, body: T) -> Result<Response> {
        // TODO(loyd): what about using `json::to_write()`?
        let encoded_body = try!(json::to_string(&body));
        let mut headers = Headers::new();

        headers.set(ContentType(mime!(Application/Json)));
        headers.set(Accept(vec![qitem(mime!(Application/Json))]));
        headers.set(XRequestedWith("XMLHttpRequest".to_owned()));

        self.post(path, &encoded_body, headers)
    }

    pub fn post_as_json(&self, path: &str, body: &str) -> Result<Response> {
        let mut headers = Headers::new();

        headers.set(ContentType(mime!(Application/Json)));
        headers.set(Accept(vec![qitem(mime!(Application/Json))]));
        headers.set(XRequestedWith("XMLHttpRequest".to_owned()));

        self.post(path, body, headers)
    }

    pub fn get_cookie(&self, cookie_name: &str) -> Option<String> {
        for cookie in self.cookie.lock().unwrap().iter() {
            if cookie.name == cookie_name {
                return Some(cookie.value.clone());
            }
        }

        None
    }

    fn request(&self, path: &str, body: Option<&str>, headers: Headers) -> Result<Response> {
        let mut retries = MAX_RETRIES;

        loop {
            let result = self._request(path, body, headers.clone());

            // Check the timeout.
            if let Err(HyperError::Io(ref io)) = result {
                let kind = io.kind();

                if retries > 0 && (kind == WouldBlock || kind == TimedOut) {
                    warn!("Retrying {}...", path);
                    retries -= 1;
                    continue;
                }
            }

            // Check the status.
            if let Ok(ref response) = result {
                // TODO(loyd): actually we need to follow redirects when possible.
                // now it's almost always should be error, but cybbet relies on 302.
                if !response.status.is_success() && !response.status.is_redirection() {
                    return Err(Error::from(response.status));
                }
            }

            return result.map_err(Error::from);
        }
    }

    fn _request(&self, path: &str, body: Option<&str>, mut headers: Headers) -> HyperResult<Response> {
        let url = format!("https://{}{}", self.host, path);

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
            return Ok(response);
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
