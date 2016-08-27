use std::convert::From;
use std::fmt::{Display, Formatter};
use std::fmt::Result as FmtResult;
use std::error::Error as StdError;
use std::result::Result as StdResult;
use std::io::Error as IoError;
use std::num::{ParseIntError, ParseFloatError};
use std::str::ParseBoolError;
use hyper::Error as HyperError;
use chrono::ParseError as ChronoParseError;
use rustc_serialize::json::DecoderError as JsonDecoderError;
use rustc_serialize::json::EncoderError as JsonEncoderError;
use rustc_serialize::json::ParserError as JsonParserError;
use hyper::status::StatusCode;

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
            Network(ref e) => e.fmt(f),
            Status(ref e) => e.fmt(f),
            Unexpected(ref e) => e.fmt(f)
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

impl From<JsonParserError> for Error {
    fn from(err: JsonParserError) -> Error {
        match err {
            JsonParserError::IoError(err) => Network(From::from(err)),
            err => Unexpected(From::from(err))
        }
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
impl_boxed!(Unexpected, JsonDecoderError);
impl_boxed!(Unexpected, JsonEncoderError);
impl_boxed!(Unexpected, ChronoParseError);
impl_boxed!(Unexpected, String);
