use hyper::client::{Client, Response};
use hyper::header::{Headers, SetCookie, Cookie, UserAgent};
use hyper::status::StatusCode;

const USER_AGENT: &'static str = concat!("Mozilla/5.0 (X11; Linux x86_64) ",
                                         "AppleWebKit/537.36 (KHTML, like Gecko) ",
                                         "Chrome/52.0.2743.85 Safari/537.36");
pub struct Session {
    client: Client,
    base_url: String,
    cookie: Cookie
}

impl Session {
    pub fn new(base_url: &str) -> Session {
        Session {
            client: Client::new(),
            base_url: base_url.to_string(),
            cookie: Cookie(vec![])
        }
    }

    pub fn get(&mut self, path: &str) -> Response {
        self.request(path, None, Headers::new())
    }

    pub fn get_with_headers(&mut self, path: &str, headers: Headers) -> Response {
        self.request(path, None, headers)
    }

    pub fn post(&mut self, path: &str, body: &str) -> Response {
        self.request(path, Some(body), Headers::new())
    }

    pub fn post_with_headers(&mut self, path: &str, body: &str, headers: Headers) -> Response {
        self.request(path, Some(body), headers)
    }

    fn request(&mut self, path: &str, body: Option<&str>, mut headers: Headers) -> Response {
        let mut url = self.base_url.clone();
        url.push_str(path);

        let builder = match body {
            Some(body) => self.client.post(&url).body(body),
            None => self.client.get(&url)
        };

        headers.set(self.cookie.clone());
        headers.set(UserAgent(USER_AGENT.to_owned()));

        let response = builder.headers(headers).send().unwrap();

        assert_eq!(response.status, StatusCode::Ok);

        let cookies = response.headers.get::<SetCookie>()
            .map_or_else(Vec::new, |c| c.0.clone());

        self.cookie = Cookie(cookies);

        response
    }
}
