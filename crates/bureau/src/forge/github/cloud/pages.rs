use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{Error, Response, request};

pub(super) const PAGE_SIZE: usize = 100;
pub(super) const MAX_PAGES: u32 = 100;
const MAX_READ_BYTES: usize = 64 * 1024 * 1024;

pub(super) enum Continuation {
    Link,
    ShortPage,
}

fn is_next(attribute: &str) -> bool {
    let Some((name, value)) = attribute.trim().split_once('=') else {
        return false;
    };
    name.eq_ignore_ascii_case("rel")
        && value
            .trim()
            .trim_matches('"')
            .split_whitespace()
            .any(|part| part == "next")
}

fn next_target(link: &str) -> Result<Option<&str>, Error> {
    let mut next = None;
    for part in link.split(',') {
        let mut attributes = part.split(';');
        let target = attributes
            .next()
            .ok_or_else(|| Error::Incomplete("empty Link entry".to_owned()))?;
        if attributes.any(is_next) && next.replace(target.trim()).is_some() {
            return Err(Error::Incomplete("multiple next-page links".to_owned()));
        }
    }
    Ok(next)
}

fn query(url: &reqwest::Url) -> Result<BTreeMap<String, String>, Error> {
    let mut pairs = BTreeMap::new();
    for (key, value) in url.query_pairs() {
        if pairs.insert(key.into_owned(), value.into_owned()).is_some() {
            return Err(Error::Incomplete(
                "duplicate continuation query parameter".to_owned(),
            ));
        }
    }
    Ok(pairs)
}

fn check_target(target: &str, expected: &reqwest::Url) -> Result<(), Error> {
    let target = target
        .strip_prefix('<')
        .and_then(|value| value.strip_suffix('>'))
        .ok_or_else(|| Error::Incomplete("malformed next-page link".to_owned()))?;
    let target = reqwest::Url::parse(target)
        .map_err(|_| Error::Incomplete("invalid next-page URL".to_owned()))?;
    let scope_matches = target.origin() == expected.origin()
        && target.path() == expected.path()
        && target.username().is_empty()
        && target.password().is_none()
        && target.fragment().is_none();
    if !scope_matches || query(&target)? != query(expected)? {
        return Err(Error::Incomplete(
            "next-page link changes scope or page sequence".to_owned(),
        ));
    }
    Ok(())
}

fn linked(response: &Response, base: &reqwest::Url, number: u32) -> Result<bool, Error> {
    let Some(link) = response.headers.get("link") else {
        return Ok(false);
    };
    let Some(target) = next_target(link)? else {
        return Ok(false);
    };
    check_target(target, &request::page(base, number + 1))?;
    Ok(true)
}

pub(super) struct Endpoint {
    pub(super) url: reqwest::Url,
    pub(super) field: &'static str,
    pub(super) continuation: Continuation,
}

impl Endpoint {
    pub(super) fn more(
        &self,
        response: &Response,
        number: u32,
        count: usize,
    ) -> Result<bool, Error> {
        if count > PAGE_SIZE {
            return Err(Error::Incomplete(
                "response exceeded the requested page size".to_owned(),
            ));
        }
        match self.continuation {
            Continuation::Link => linked(response, &self.url, number),
            Continuation::ShortPage => Ok(count == PAGE_SIZE),
        }
    }
}

pub(super) struct Collection<T> {
    pub(super) items: Vec<T>,
    pub(super) total: Option<u32>,
    bytes: usize,
}

impl<T: DeserializeOwned> Collection<T> {
    pub(super) const fn new() -> Self {
        Self {
            items: Vec::new(),
            total: None,
            bytes: 0,
        }
    }

    fn check_size(&mut self, bytes: usize) -> Result<(), Error> {
        if bytes > MAX_READ_BYTES.saturating_sub(self.bytes) {
            return Err(Error::Incomplete(
                "paginated read exceeds the 64 MiB client limit".to_owned(),
            ));
        }
        self.bytes += bytes;
        Ok(())
    }

    pub(super) fn extend(&mut self, response: &Response, field: &str) -> Result<usize, Error> {
        self.check_size(response.body.len())?;
        let mut value: Value = serde_json::from_slice(&response.body)
            .map_err(|error| Error::Response(error.to_string()))?;
        let object = value
            .as_object_mut()
            .ok_or_else(|| Error::Response("expected an object page".to_owned()))?;
        let items = object
            .remove(field)
            .ok_or_else(|| Error::Response(format!("missing `{field}` array")))?;
        let items: Vec<T> =
            serde_json::from_value(items).map_err(|error| Error::Response(error.to_string()))?;
        let total =
            serde_json::from_value::<Option<u32>>(object.remove("total").unwrap_or(Value::Null))
                .map_err(|error| Error::Response(error.to_string()))?;
        self.total = total.or(self.total);
        let count = items.len();
        self.items.extend(items);
        Ok(count)
    }
}

fn insert_unique<T: PartialEq>(
    seen: &mut BTreeMap<String, usize>,
    unique: &mut Vec<T>,
    key: String,
    item: T,
) -> Result<(), Error> {
    if let Some(index) = seen.get(&key) {
        return if unique[*index] == item {
            Ok(())
        } else {
            Err(Error::Incomplete(
                "an identity changed while pages were being read".to_owned(),
            ))
        };
    }
    seen.insert(key, unique.len());
    unique.push(item);
    Ok(())
}

pub(super) fn unique<T: PartialEq>(
    items: Vec<T>,
    key: impl Fn(&T) -> &str,
) -> Result<Vec<T>, Error> {
    let mut seen = BTreeMap::new();
    let mut unique = Vec::new();
    for item in items {
        insert_unique(&mut seen, &mut unique, key(&item).to_owned(), item)?;
    }
    Ok(unique)
}
