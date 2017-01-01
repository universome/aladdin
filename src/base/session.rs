#![allow(dead_code)]

use std::io::Read;
use std::time::Duration;
use parking_lot::RwLock;
use time;
use url::form_urlencoded::Serializer as UrlSerializer;
use hyper::error::{Error as HyperError, Result as HyperResult};
use hyper::client::{Client, RedirectPolicy, Response};
use hyper::header::{Headers, SetCookie, Cookie, UserAgent, Accept, ContentType, qitem, CookiePair};
use kuchiki;
use kuchiki::NodeRef;
use kuchiki::traits::ParserExt;
use serde::{Serialize, Deserialize};
use serde_json as json;
use hyper::mime::Mime;

use base::error::{Result, Error};

header! { (XRequestedWith, "X-Requested-With") => [String] }

const MAX_ATTEMPTS: u32 = 3;
const READ_TIMEOUT: u64 = 20;   // We should set large timeout due to the long-polling.
const WRITE_TIMEOUT: u64 = 5;

const USER_AGENT: &str = "Lynx/2.8.8rel.2 libwww-FM/2.14 SSL-MM/1.4.1 OpenSSL/1.0.2h";

pub struct Session {
    host: String,
    cookie: RwLock<Cookie>,
    client: Client
}

impl Session {
    pub fn new(host: &str) -> Session {
        let mut client = Client::new();

        client.set_read_timeout(Some(Duration::from_secs(READ_TIMEOUT)));
        client.set_write_timeout(Some(Duration::from_secs(WRITE_TIMEOUT)));
        client.set_redirect_policy(RedirectPolicy::FollowNone);

        Session {
            host: host.to_string(),
            client: client,
            cookie: RwLock::new(Cookie(vec![]))
        }
    }

    pub fn get_cookie(&self, cookie_name: &str) -> Option<String> {
        for cookie in self.cookie.read().iter() {
            if cookie.name == cookie_name {
                return Some(cookie.value.clone());
            }
        }

        None
    }

    pub fn request(&self, path: &str) -> RequestBuilder {
        let url = format!("https://{}{}", self.host, path);

        RequestBuilder::new(url, &self)
    }

    pub fn set_cookies(&self, cookies: &[CookiePair]) {
        let mut current = self.cookie.write();

        for c in cookies {
            let mut cookie = c.clone();

            if cookie.max_age.is_some() && cookie.expires.is_none() {
                cookie.expires = Some(time::at_utc(time::Timespec {
                    sec: time::now().to_timespec().sec + (cookie.max_age.unwrap() as i64),
                    nsec: 0
                }));
            }

            let existing = current.iter().position(|x| c.name == x.name && c.domain == x.domain);

            if let Some(index) = existing {
                current[index] = cookie;
            } else {
                current.push(cookie);
            }
        }
    }

    pub fn actualize_cookies(&self) {
        let mut cookies = self.cookie.write();

        cookies.retain(|c| c.expires.map_or(true, |e| e > time::now()));
    }
}

pub enum Type { Json, Form }

impl Into<Mime> for Type {
    #[inline]
    fn into(self) -> Mime {
        match self {
            Type::Json => mime!(Application/Json),
            Type::Form => mime!(Application/WwwFormUrlEncoded)
        }
    }
}

pub struct RequestBuilder<'a> {
    session: &'a Session,
    headers: Headers,
    url: String,
    timeouts: Option<(u64, u64)>,
    follow_redirects: bool
}

impl<'a> RequestBuilder<'a> {
    pub fn new(url: String, session: &Session) -> RequestBuilder {
        let mut headers = Headers::new();

        headers.set(UserAgent(USER_AGENT.to_owned()));
        headers.set(XRequestedWith("XMLHttpRequest".to_owned()));
        headers.set(ContentType(mime!(Application/Json)));

        // Let's accept everything!
        headers.set(Accept(vec![
            qitem(mime!(Text/Plain)),
            qitem(mime!(Text/Html)),
            qitem(mime!(Application/Json)),
            qitem(mime!(_/_))
        ]));

        RequestBuilder {
            url: url,
            session: session,
            headers: headers,
            timeouts: None,
            follow_redirects: false
        }
    }

    #[inline]
    pub fn content_type(mut self, content_type: Type) -> RequestBuilder<'a> {
        self.headers.set(ContentType(content_type.into()));
        self
    }

    #[inline]
    pub fn timeouts(mut self, timeouts: Option<(u64, u64)>) -> RequestBuilder<'a> {
        self.timeouts = timeouts;
        self
    }

    #[inline]
    pub fn headers(mut self, headers: &[(&'static str, &str)]) -> RequestBuilder<'a> {
        for &(name, value) in headers {
            self.headers.set_raw(name, vec![value.as_bytes().to_vec()]);
        }
        self
    }

    #[inline]
    pub fn follow_redirects(mut self, follow_redirects: bool) -> RequestBuilder<'a> {
        self.follow_redirects = follow_redirects;
        self
    }

    #[inline]
    pub fn get<R: Receivable>(&self) -> Result<R> {
        self.send::<R, String>(None)
    }

    #[inline]
    pub fn post<R: Receivable, S: Sendable>(&self, body: S) -> Result<R> {
        self.send(Some(body))
    }

    fn send<R: Receivable, S: Sendable>(&self, body: Option<S>) -> Result<R> {
        let mut attempts = MAX_ATTEMPTS;

        let body = match body {
            Some(body) => Some(try!(body.to_string())),
            None => None
        };

        let body_ref = body.as_ref().map(|body| body.as_str());

        loop {
            attempts -= 1;

            let result = match self.timeouts {
                Some(timeouts) => {
                    let mut client = Client::new();

                    client.set_read_timeout(Some(Duration::from_secs(timeouts.0)));
                    client.set_write_timeout(Some(Duration::from_secs(timeouts.1)));

                    self._send(&client, body_ref)
                },
                None => self._send(&self.session.client, body_ref)
            };

            // Retry if some error occurs.
            if let Err(HyperError::Io(ref io)) = result {
                if attempts > 0 {
                    warn!("Retrying {} due to error {}...", self.url, io);
                    continue;
                }
            }

            let response = try!(result);

            if attempts > 0 && response.status.is_server_error() {
                warn!("Retrying {} due to {}...", self.url, response.status);
                continue;
            }

            // TODO(universome): actually we need to follow redirects when possible.
            // now it's almost always should be error, but cybbet relies on 302.
            if response.status.is_redirection() {
                if !self.follow_redirects {
                    return Err(Error::from("Was redirected, but have no redirect policy"));
                }

                return R::read(response);
            }

            if !response.status.is_success() {
                return Err(Error::from(response.status));
            }

            return R::read(response);
        }
    }

    fn _send(&self, client: &Client, body: Option<&str>) -> HyperResult<Response> {
        trace!("{} {}", if body.is_none() { "GET" } else { "POST" }, self.url);

        let builder = match body {
            Some(body) => client.post(&self.url).body(body),
            None => client.get(&self.url)
        };

        self.session.actualize_cookies();
        let mut headers = self.headers.clone();
        let cookie = self.session.cookie.read().clone();
        headers.set(cookie);

        let response = try!(builder.headers(headers).send());

        if !response.status.is_success() && !response.status.is_redirection() {
            return Ok(response);
        }

        if let Some(cookies) = response.headers.get::<SetCookie>() {
            self.session.set_cookies(&cookies.0);
        }

        Ok(response)
    }
}

pub trait Receivable: Sized {
    fn read(response: Response) -> Result<Self>;
}

impl Receivable for String {
    #[inline]
    fn read(mut response: Response) -> Result<String> {
        let mut string = String::new();
        try!(response.read_to_string(&mut string));

        Ok(string)
    }
}

impl<T: Deserialize> Receivable for T {
    #[inline]
    default fn read(response: Response) -> Result<T> {
        Ok(try!(json::from_reader(response)))
    }
}

impl Receivable for NodeRef {
    #[inline]
    fn read(response: Response) -> Result<NodeRef> {
        Ok(try!(kuchiki::parse_html().from_http(response)))
    }
}

pub trait Sendable {
    fn to_string(&self) -> Result<String>;
}

impl<S: Serialize> Sendable for S {
    #[inline]
    default fn to_string(&self) -> Result<String> {
        Ok(try!(json::to_string(&self)))
    }
}

impl Sendable for String {
    #[inline]
    fn to_string(&self) -> Result<String> {
        Ok(self.to_owned())
    }
}

impl<'a> Sendable for Vec<(&'a str, &'a str)> {
    #[inline]
    fn to_string(&self) -> Result<String> {
        Ok(UrlSerializer::new(String::new()).extend_pairs(self.iter()).finish())
    }
}
