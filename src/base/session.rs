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
use hyper::mime::Mime;

use base::error::{Result, Error};

header! { (XRequestedWith, "X-Requested-With") => [String] }

const MAX_RETRIES: u32 = 2;
const READ_TIMEOUT: u64 = 20;   // We should set large timeout due to the long-polling.
const WRITE_TIMEOUT: u64 = 5;

const USER_AGENT: &str = "Lynx/2.8.8rel.2 libwww-FM/2.14 SSL-MM/1.4.1 OpenSSL/1.0.2h";

pub struct Session {
    host: String,
    cookie: Mutex<Cookie>,
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
            cookie: Mutex::new(Cookie(vec![]))
        }
    }

    pub fn get_cookie(&self, cookie_name: &str) -> Option<String> {
        for cookie in self.cookie.lock().unwrap().iter() {
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
}

pub enum Type { Json, Form }

impl Into<Mime> for Type {
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
        let cookie = session.cookie.lock().unwrap().clone();

        headers.set(cookie);
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

    pub fn content_type(mut self, content_type: Type) -> RequestBuilder<'a> {
        self.headers.set(ContentType(content_type.into()));
        self
    }

    pub fn timeouts(mut self, timeouts: Option<(u64, u64)>) -> RequestBuilder<'a> {
        self.timeouts = timeouts;
        self
    }

    pub fn headers(mut self, headers: &[(&'static str, &str)]) -> RequestBuilder<'a> {
        for &(name, value) in headers {
            self.headers.set_raw(name, vec![value.as_bytes().to_vec()]);
        }
        self
    }

    pub fn follow_redirects(mut self, follow_redirects: bool) -> RequestBuilder<'a> {
        self.follow_redirects = follow_redirects;
        self
    }

    pub fn get<R: Receivable>(&self) -> Result<R> {
        self.send::<R, String>(None)
    }

    pub fn post<R: Receivable, S: Sendable>(&self, body: S) -> Result<R> {
        self.send(Some(body))
    }

    fn send<R: Receivable, S: Sendable>(&self, body: Option<S>) -> Result<R> {
        let mut retries = MAX_RETRIES;
        let body = match body {
            Some(body) => Some(try!(body.to_string())),
            None => None
        };

        loop {
            let result = match self.timeouts {
                Some(timeouts) => {
                    let mut client = Client::new();

                    client.set_read_timeout(Some(Duration::from_secs(timeouts.0)));
                    client.set_write_timeout(Some(Duration::from_secs(timeouts.1)));

                    self._send(&client, &body)
                },
                None => self._send(&self.session.client, &body)
            };

            // Check the timeout.
            if let Err(HyperError::Io(ref io)) = result {
                let kind = io.kind();

                if retries > 0 && (kind == WouldBlock || kind == TimedOut) {
                    warn!("Retrying {}...", self.url);
                    retries -= 1;
                    continue;
                }
            }

            let response = try!(result);

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

    fn _send(&self, client: &Client, body: &Option<String>) -> HyperResult<Response> {
        let builder = match *body {
            Some(ref body) => client.post(&self.url).body(body.as_str()),
            None => client.get(&self.url)
        };

        let response = try!(builder.headers(self.headers.clone()).send());

        if !response.status.is_success() && !response.status.is_redirection() {
            return Ok(response);
        }

        if let Some(cookies) = response.headers.get::<SetCookie>() {
            let mut stored = self.session.cookie.lock().unwrap();

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

pub trait Receivable: Sized {
    fn read(response: Response) -> Result<Self>;
}

impl Receivable for String {
    fn read(mut response: Response) -> Result<String> {
        let mut string = String::new();
        try!(response.read_to_string(&mut string));

        Ok(string)
    }
}

impl<T: Deserialize> Receivable for T {
    default fn read(response: Response) -> Result<T> {
        Ok(try!(json::from_reader(response)))
    }
}

impl Receivable for NodeRef {
    fn read(response: Response) -> Result<NodeRef> {
        Ok(try!(kuchiki::parse_html().from_http(response)))
    }
}

pub trait Sendable {
    fn to_string(&self) -> Result<String>;
}

impl<S: Serialize> Sendable for S {
    default fn to_string(&self) -> Result<String> {
        Ok(try!(json::to_string(&self)))
    }
}

impl Sendable for String {
    fn to_string(&self) -> Result<String> {
        Ok(self.to_owned())
    }
}

impl<'a> Sendable for Vec<(&'a str, &'a str)> {
    fn to_string(&self) -> Result<String> {
        Ok(UrlSerializer::new(String::new()).extend_pairs(self.iter()).finish())
    }
}
