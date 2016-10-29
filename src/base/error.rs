use std::convert::From;
use std::fmt::{Display, Formatter};
use std::fmt::Result as FmtResult;
use std::error::Error as StdError;
use std::result::Result as StdResult;
use std::sync::PoisonError;
use std::io::Error as IoError;
use std::num::{ParseIntError, ParseFloatError};
use std::str::ParseBoolError;
use hyper::Error as HyperError;
use time::ParseError as TimeParseError;
use serde_json::Error as JsonError;
use hyper::status::StatusCode;
use url::ParseError as UrlParseError;
use websocket::result::WebSocketError;

use self::Error::*;

pub type BoxedError = Box<StdError + Send + Sync>;

pub type Result<T> = StdResult<T, Error>;

#[derive(Debug)]
pub enum Error {
    Network(BoxedError),
    Status(StatusCode),
    Unexpected(BoxedError)
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        match *self {
            Network(ref e) => write!(f, "Network error: {}", e),
            Status(ref e) => write!(f, "Bad status code: {}", e),
            Unexpected(ref e) => write!(f, "Unexpected error: {}", e)
        }
    }
}

impl StdError for Error {
    fn description(&self) -> &str {
        match *self {
            Network(ref err) => err.description(),
            Status(ref code) => code.canonical_reason().unwrap_or("Strange status code"),
            Unexpected(ref err) => err.description()
        }
    }
}

impl From<StatusCode> for Error {
    fn from(code: StatusCode) -> Error {
        Status(code)
    }
}

impl From<JsonError> for Error {
    fn from(err: JsonError) -> Error {
        match err {
            JsonError::Io(err) => Network(From::from(err)),
            err => Unexpected(From::from(err))
        }
    }
}

impl<T> From<PoisonError<T>> for Error {
    fn from(_: PoisonError<T>) -> Error {
        Error::from("Poison error")
    }
}

impl<'a> From<&'a str> for Error {
    fn from(err: &str) -> Error {
        Unexpected(From::from(err))
    }
}

// TODO(loyd): make better after stabilization of impl specialization.
macro_rules! impl_boxed {
    ($variant:ident, $err:ty) => {
        impl From<$err> for Error {
            fn from(err: $err) -> Error {
                $variant(From::from(err))
            }
        }
    }
}

impl_boxed!(Network, HyperError);
impl_boxed!(Network, IoError);
impl_boxed!(Unexpected, ParseIntError);
impl_boxed!(Unexpected, ParseFloatError);
impl_boxed!(Unexpected, ParseBoolError);
impl_boxed!(Unexpected, TimeParseError);
impl_boxed!(Unexpected, UrlParseError);
impl_boxed!(Unexpected, WebSocketError);
impl_boxed!(Unexpected, String);
