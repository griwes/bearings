use serde::{Deserialize, Serialize};
use serde_with::serde;

pub type Result<T, U> = std::result::Result<T, Error<U>>;

#[derive(Debug, Serialize, Deserialize)]
pub struct UserErrorHolder<T> {
    inner: T,
}

fn never_invoke<T, U>(_: U) -> T {
    panic!()
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Error<T: std::fmt::Debug> {
    UuidAlreadyInUse,

    #[serde(
        serialize_with = "serde_with::rust::display_fromstr::serialize",
        deserialize_with = "never_invoke"
    )]
    IoError(std::io::Error),
    #[serde(
        serialize_with = "serde_with::rust::display_fromstr::serialize",
        deserialize_with = "never_invoke"
    )]
    JsonError(serde_json::Error),

    Stringized(String),
    UserErrorEnum(UserErrorHolder<T>),
}

impl<T: std::fmt::Debug> Error<T> {
    pub fn to_user(&self) -> Option<&T> {
        match self {
            UserErrorEnum(ref user) => Some(&user.inner),
            _ => None,
        }
    }
}

pub fn as_error<T: std::fmt::Debug>(val: T) -> Error<T> {
    UserErrorEnum(UserErrorHolder { inner: val })
}

use Error::*;

impl<T: std::fmt::Debug + std::fmt::Display> std::fmt::Display for Error<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match *self {
            UuidAlreadyInUse => write!(f, "the service already serves an object with the same uuid as the object you tried to add"),

            IoError(ref err) => err.fmt(f),
            JsonError(ref err) => err.fmt(f),

            Stringized(ref string) => string.fmt(f),
            UserErrorEnum(ref err) => std::fmt::Display::fmt(&err.inner, f)
        }
    }
}

impl<T: 'static + std::fmt::Debug + std::fmt::Display + std::error::Error> std::error::Error
    for Error<T>
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            UuidAlreadyInUse => None,

            IoError(ref err) => Some(err),
            JsonError(ref err) => Some(err),

            Stringized(_) => None,
            UserErrorEnum(ref err) => Some(&err.inner),
        }
    }
}

impl<T: std::fmt::Debug> std::convert::From<std::io::Error> for Error<T> {
    fn from(err: std::io::Error) -> Self {
        IoError(err)
    }
}

impl<T: std::fmt::Debug> std::convert::From<serde_json::Error> for Error<T> {
    fn from(err: serde_json::Error) -> Self {
        JsonError(err)
    }
}
