use std::cell::RefCell;
use hyper::client::{Client, Response};
use hyper::header::{Headers, SetCookie, Cookie, UserAgent, ContentLength};
use hyper::status::StatusCode;

const USER_AGENT: &'static str = concat!("Mozilla/5.0 (X11; Linux x86_64) ",
                                         "AppleWebKit/537.36 (KHTML, like Gecko) ",
                                         "Chrome/52.0.2743.85 Safari/537.36");
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

    pub fn get(&self, path: &str) -> Response {
        self.request(path, None, Headers::new())
    }

    pub fn get_with_headers(&self, path: &str, headers: Headers) -> Response {
        self.request(path, None, headers)
    }

    pub fn post(&self, path: &str, body: &str) -> Response {
        self.request(path, Some(body), Headers::new())
    }

    pub fn post_with_headers(&self, path: &str, body: &str, headers: Headers) -> Response {
        self.request(path, Some(body), headers)
    }

    fn request(&self, path: &str, body: Option<&str>, mut headers: Headers) -> Response {
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

        let response = builder.headers(headers).send().unwrap();

        assert_eq!(response.status, StatusCode::Ok);

        let cookies = response.headers.get::<SetCookie>()
            .map_or_else(Vec::new, |c| c.0.clone());

        *self.cookie.borrow_mut() = Cookie(cookies);

        response
    }
}
