use std::str::FromStr;
use tide::http::{mime, Mime};

pub(crate) fn html() -> Mime {
    Mime::from_str("text/html; charset=utf-8").unwrap()
}

pub(crate) fn css() -> Mime {
    Mime::from_str("text/css; charset=utf-8").unwrap()
}

pub(crate) fn js() -> Mime {
    Mime::from_str("text/javascript; charset=utf-8").unwrap()
}

pub(crate) fn json() -> Mime {
    mime::JSON
}
