extern crate chrono;
extern crate dirs;
extern crate env_logger;
#[macro_use]
extern crate log;
extern crate reqwest;
extern crate serde;
extern crate serde_json;
extern crate toml;
extern crate cheap_alerts;
use serde::Deserialize;

mod response;

use chrono::{
    Local,
    Duration
};
use dirs::home_dir;

use cheap_alerts::{
    Carrier,
    Destination,
    Sender,
};
use reqwest::get;

#[tracing::instrument]
fn main() -> Result<(), Error> {
    init_logging();
    let mut config = get_config()?;
    let mut users = Vec::with_capacity(0);
    ::std::mem::swap(&mut config.users, &mut users);
    let mut consecutive_errors = 0;
    loop {
        let mut errored = false;
        for user in users.iter_mut() {
            for address in &config.locations {
                let change = match update_order(user, address) {
                    Ok(change) => {
                        info!("Successfully updated order for {}, found {:?}", user.name, change);
                        change
                    },
                    Err(e) => {
                        errored = true;
                        error!("Error updating orders for {}: {:?}", user.name, e);
                        None
                    },
                };
                if let Some(change) = change {
                    info!("Sending update");
                    match send_update(&format!("{}", change), &config.from_addr, &user.as_dest()?) {
                        Ok(()) => info!("Successfully sent update"),
                        Err(e) => {
                            error!("Error sending updates to {}: {}", &user.name, e);
                            errored = true;
                        }
                    }
                }
            }
        }
        if errored {
            consecutive_errors += 1
        } else {
            consecutive_errors = 0;
        }
        if consecutive_errors > config.consecutive_errors_limit {
            error!("Too many consecutive errors, exiting program");
            break;
        }
        ::std::thread::sleep(::std::time::Duration::from_millis(config.check_interval as u64))
    }
    Ok(())
}

#[tracing::instrument]
fn get_config() -> Result<Config, Error> {
    trace!("get config");
    let mut config_path = home_dir().ok_or_else(|| Error::other("Unable to get home directory"))?;
    config_path.push(".pizza_freak");
    let config_text = ::std::fs::read_to_string(config_path)?;
    let config = toml::from_str(&config_text)?;
    trace!("{:#?}", config);
    Ok(config)
}

#[tracing::instrument]
fn init_logging() {
    let mut b = env_logger::Builder::from_default_env();
    b.target(env_logger::Target::Stdout);
    b.init();
}

#[tracing::instrument]
fn update_order(user: &mut User, loc: &Location) -> Result<Option<response::Status>, Error> {
    debug!("updating orders for {} at {}", user.name, loc.name);
    if let Some(order) = user.order.as_mut() {
        let dur = Local::now().signed_duration_since(order.first_seen);
        if dur > Duration::hours(12) {
            user.order = None;
            return Ok(None)
        }
        let result = get_order(&loc.url, &user.phone_number.dashes_string())?;
        debug!("got orders from phone numbers");
        match result {
            response::Response::NoOrder(_r) => {
                info!("no order found");
                Ok(None)
            }
            response::Response::Order(new) => {
                if order.status != new.status {
                    order.status = new.status;
                    Ok(Some(new.status))
                } else {
                    Ok(None)
                }
            }
        }
    } else {
        let result = get_order(&loc.url, &user.phone_number.dashes_string())?;
        debug!("got orders from phone numbers");
        match result {
            response::Response::NoOrder(_r) => {
                info!("no order found");
                Ok(None)
            }
            response::Response::Order(new) => {
                let status = new.status;
                user.order = Some(new);
                Ok(Some(status))
            }
        }
    }
}

#[cfg(feature = "email")]
#[tracing::instrument]
fn send_update(msg: &str, from_addr: &str, dest: &Destination) -> Result<(), Error> {
    debug!("sending update: {}", msg);
    let mut sender = Sender::builder()
        .address(from_addr)
        .smtp_unencrypted_localhost()?;
    sender.send_to(&dest, msg)?;
    Ok(())
}

#[cfg(not(feature = "email"))]
#[tracing::instrument]
fn send_update(msg: &str, from_addr: &str, dest: &Destination) -> Result<(), Error> {
    let mut sender = Sender::builder()
        .address(from_addr)
        .stdout()?;
    sender.send_to(&dest, msg)?;
    Ok(())
}

#[tracing::instrument]
fn get_order(url_base: &str, phone_number: &str) -> Result<response::Response, Error> {
    let url = format!("{}{}", url_base, phone_number);
    trace!("{}", url);
    let mut res = get(&url)?;
    let text = res.text()?;
    trace!("json text:\n{:?}", text);
    let ret = serde_json::from_str(&text).map_err(|e| {
        error!("failed to deserialize json: {:?}", text);
        e
    })?;
    Ok(ret)
}


impl ::std::fmt::Display for response::Status {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            response::Status::Deferred => write!(f, "Deferred, The store might not be open?"),
            response::Status::Reviewing => write!(f, "Reviewing, Management is checking things over apparently"),
            response::Status::Cooking => write!(f, "The cooks are working on your order now!"),
            response::Status::OnTheWay => write!(f, "The driver is heading to your house!"),
            response::Status::Delivered => write!(f, "You are eating pizza!"),
            _ => write!(f, "I don't know what status this is, are you sure you ordered a pizza?"),
        }
    }
}
#[tracing::instrument]
pub fn escape_default(s: &str) -> String {
    s.chars().flat_map(|c| c.escape_default()).collect()
}

#[derive(Deserialize, Debug)]
struct Config {
    pub check_interval: usize,
    pub users: Vec<User>,
    pub locations: Vec<Location>,
    pub consecutive_errors_limit: usize,
    pub from_addr: String,
}
#[derive(Deserialize, Debug)]
struct Location {
    pub name: String,
    pub url: String,
}
#[derive(Deserialize, Clone, Debug)]
struct User {
    pub name: String,
    pub carrier: String,
    pub phone_number: PhoneNumber,
    pub order: Option<response::Order>
}

impl User {
#[tracing::instrument]
pub fn as_dest(&self) -> Result<Destination, Error> {
        use std::str::FromStr;
        let carrier = Carrier::from_str(&self.carrier)?;
        let dest = Destination::new(&self.phone_number.to_string(), &carrier);
        Ok(dest)
    }
}

#[derive(Clone, Debug)]
struct PhoneNumber {
    area_code: String,
    prefix: String,
    suffix: String,
}

impl PhoneNumber {
#[tracing::instrument]
    fn try_parse(ph: &str) -> Result<PhoneNumber, Error> {
        if ph.len() < 10 {
            return Err(Error::Other(format!("Phone numbers must be at least 10 digits found {}", ph.len())));
        }
        let area_code = String::from(&ph[0..3]);
        let mut prefix_start = 4;
        if &ph[4..4] == "-" || &ph[4..4] == "." {
            prefix_start += 1;
            if ph.len() < 11 {
                return Err(Error::other("Phone number not long enough after area code"));
            }
        }
        let prefix = String::from(&ph[prefix_start..prefix_start + 3]);
        let mut suffix_start = prefix_start + 4;
        if &ph[suffix_start..suffix_start] == "-"
            || &ph[suffix_start..suffix_start] == "." {
            suffix_start += 1;
            if ph.len() < 12 {
                return Err(Error::other("Phone number not long enough after prefix"));
            }
        }
        let suffix = String::from(&ph[suffix_start..suffix_start + 4]);
        Ok(PhoneNumber {
            area_code,
            prefix,
            suffix,
        })
    }

#[tracing::instrument]
    fn dashes_string(&self) -> String {
        format!("{}-{}-{}", self.area_code, self.prefix, self.suffix)
    }

#[tracing::instrument]
    fn to_string(&self) -> String {
        format!("{}{}{}", self.area_code, self.prefix, self.suffix)
    }
}

impl<'de> serde::de::Deserialize<'de> for PhoneNumber {
    fn deserialize<D>(deserializer: D) -> Result<PhoneNumber, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        PhoneNumber::try_parse(&s).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug)]
enum Error {
    Json(serde_json::Error),
    Reqwest(reqwest::Error),
    Time(chrono::ParseError),
    Other(String),
    Parse(::std::num::ParseIntError),
    Cheap(cheap_alerts::Error),
    Io(::std::io::Error),
    Toml(toml::de::Error),
}

impl ::std::fmt::Display for Error {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            Error::Json(ref e) => e.fmt(f),
            Error::Reqwest(ref e) => e.fmt(f),
            Error::Time(ref e) => e.fmt(f),
            Error::Other(ref e) => e.fmt(f),
            Error::Parse(ref e) => e.fmt(f),
            Error::Io(ref e) => e.fmt(f),
            Error::Cheap(ref e) => e.fmt(f),
            Error::Toml(ref e) => e.fmt(f),
        }
    }
}

impl ::std::error::Error for Error {}

impl From<reqwest::Error> for Error {
#[tracing::instrument]
    fn from(other: reqwest::Error) -> Self {
        Error::Reqwest(other)
    }
}

impl From<serde_json::Error> for Error {
#[tracing::instrument]
    fn from(other: serde_json::Error) -> Self {
        Error::Json(other)
    }
}

impl From<chrono::ParseError> for Error {
#[tracing::instrument]
    fn from(other: chrono::ParseError) -> Self {
        Error::Time(other)
    }
}

impl From<::std::num::ParseIntError>  for Error {
#[tracing::instrument]
    fn from(other: ::std::num::ParseIntError) -> Self {
        Error::Parse(other)
    }
}

impl From<::std::io::Error> for Error {
#[tracing::instrument]
    fn from(other: ::std::io::Error) -> Self {
        Error::Io(other)
    }
}

impl From<cheap_alerts::Error> for Error {
#[tracing::instrument]
    fn from(other: cheap_alerts::Error) -> Self {
        Error::Cheap(other)
    }
}

impl From<toml::de::Error> for Error {
#[tracing::instrument]
    fn from(other: toml::de::Error) -> Self {
        Error::Toml(other)
    }
}

impl Error {
#[tracing::instrument]
    fn other(s: &str) -> Self {
        Error::Other(s.into())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn order_deserialize() {
        let no_order = include_str!("../ph-example-empty.json");
        let order = include_str!("../ph-example-with.json");
        let _: response::Response = serde_json::from_str(&no_order).unwrap();
        let _: response::Response = serde_json::from_str(&order).unwrap();
    }
}