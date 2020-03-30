extern crate chrono;
extern crate dirs;
extern crate env_logger;
#[macro_use]
extern crate log;
extern crate reqwest;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate toml;
extern crate cheap_alerts;

use std::default::Default;

use chrono::{
    DateTime,
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

fn main() -> Result<(), Error> {
    init_logging();
    let mut config = get_config()?;
    let mut users = Vec::with_capacity(0);
    ::std::mem::swap(&mut config.users, &mut users);
    let mut consecutive_errors = 0;
    loop {
        let mut errored = false;
        for user in users.iter_mut() {
            for address in &config.check_addresses {
                let changes = match update_orders(user, address) {
                    Ok(changes) => {
                        info!("Successfully updated orders for {}, found {} changes", user.name, changes.len());
                        changes
                    },
                    Err(e) => {
                        errored = true;
                        error!("Error updating orders for {}: {:?}", user.name, e);
                        vec![]
                    },
                };
                info!("Sending {} updates", changes.len());
                match send_updates(changes, &config.from_addr, &user.as_dest()?) {
                    Ok(()) => info!(target: "pizza_freak:info", "Successfully sent updates"),
                    Err(e) => {
                        error!("Error sending updates to {}: {}", &user.name, e);
                        errored = true;
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

fn get_config() -> Result<Config, Error> {
    trace!("get config");
    let mut config_path = home_dir().ok_or_else(|| Error::other("Unable to get home directory"))?;
    config_path.push(".pizza_freak");
    let config_text = ::std::fs::read_to_string(config_path)?;
    let config = toml::from_str(&config_text)?;
    trace!("{:#?}", config);
    Ok(config)
}

fn init_logging() {
    let mut b = env_logger::Builder::from_default_env();
    b.target(env_logger::Target::Stdout);
    b.init();
}

fn update_orders(user: &mut User, check_addr: &str) -> Result<Vec<(i32, OrderStatus)>, Error> {
    debug!("updating orders for {}", user.name);
    info!("starting orders: {:#?}", user.orders);
    let start_len = user.orders.len();
    user.orders.retain(|o|{
        let dur = Local::now().signed_duration_since(o.time_ordered.clone());
        let twelve = Duration::hours(12);
        if dur > twelve {
            debug!("Order is older than 12 hours and complete");
            false
        } else {
            debug!("Order is either younger than 12 hours or is not yet complete");
            true
        }
    });
    debug!("removed {} old orders", start_len - user.orders.len());
    let result = get_order_list(check_addr, &user.phone_number.dashes_string())?;
    debug!("got orders from phone numbers");
    let mut changes = vec![];
    if let Response::List(orders) = result.response {
        for order in orders {
            let mut order_not_found = true;
            for user_order in user.orders.iter_mut() {
                if user_order.order_id == order.order_id {
                    debug!("Updating orders status");
                    user_order.order_status_image = order.order_status_image.clone();
                    order_not_found = false;
                    if user_order.update_status() && (
                        user_order.status == OrderStatus::Cooking
                        || user_order.status == OrderStatus::OutForDelivery
                    ) {
                        changes.push((user_order.order_id, user_order.status));
                    }
                    break;
                }
            }
            if order_not_found {
                debug!("order not found, inserting new order");
                user.orders.push(order);
            }
        }
    }
    Ok(changes)
}
fn send_updates(changes: Vec<(i32, OrderStatus)>, from_addr: &str, dest: &Destination) -> Result<(), Error> {
    for (id, new_status) in changes {
        send_update(&format!("Pizza Freak Order #{}\n{}",
                                id,
                                new_status),
                    from_addr,
                    dest)?;
    }
    Ok(())
}

#[cfg(feature = "email")]
fn send_update(msg: &str, from_addr: &str, dest: &Destination) -> Result<(), Error> {
    debug!("sending update: {}", msg);
    let mut sender = Sender::builder()
        .address(from_addr)
        .smtp_unencrypted_localhost()?;
    sender.send_to(&dest, msg)?;
    Ok(())
}

#[cfg(not(feature = "email"))]
fn send_update(msg: &str, from_addr: &str, dest: &Destination) -> Result<(), Error> {
    let mut sender = Sender::builder()
        .address(from_addr)
        .stdout()?;
    sender.send_to(&dest, msg)?;
    Ok(())
}

fn get_order_list(url_base: &str, phone_number: &str) -> Result<OrderListResponse, Error> {
    let mut res = get(&format!("{}{}", url_base, phone_number))?;
    let text = res.text()?;
    trace!("json text:\n{:?}", text);
    let ret = serde_json::from_str(&text)?;
    Ok(ret)
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq)]
#[repr(u8)]
enum OrderStatus {
    Deferred,
    Reviewing,
    Cooking,
    OutForDelivery,
    Delivered,
    Unknown,
}

fn determine_status(val: &str) -> OrderStatus {
    debug!("determine_status {}", val);
    if val.contains("webfile?name=order-tracker-driving.png") {
        OrderStatus::OutForDelivery
    } else if val.contains("webfile?name=order-tracker-cooking.png") {
        OrderStatus::Cooking
    } else if val.contains("webfile?name=order-tracker-delivered.png") {
        OrderStatus::Delivered
    } else if val.contains("webfile?name=order-tracker-reviewing.png") {
        OrderStatus::Reviewing
    } else if val.contains("webfile?name=order-tracker-deferred.png") {
        OrderStatus::Deferred
    } else {
        OrderStatus::Unknown
    }
}

impl ::std::fmt::Display for OrderStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            OrderStatus::Deferred => write!(f, "Deferred, The store might not be open?"),
            OrderStatus::Reviewing => write!(f, "Reviewing, Management is checking things over apparently"),
            OrderStatus::Cooking => write!(f, "The cooks are working on your order now!"),
            OrderStatus::OutForDelivery => write!(f, "The driver is heading to your house!"),
            OrderStatus::Delivered => write!(f, "You are eating pizza!"),
            OrderStatus::Unknown => write!(f, "I don't know what status this is, are you sure you ordered a pizza?"),
        }
    }
}

// #[derive(Debug, Clone)]
// enum DomNode {
//     Text(String),
//     Element(HTMLElement),
// }

// #[derive(Debug, Clone)]
// struct HTMLElement {
//     name: String,
//     attributes: HashMap<String, String>,
//     children: Vec<DomNode>
// }

// impl DomNode {
//     fn get_element_by_id(&self, id: &str) -> Option<DomNode> {
//         match self {
//             DomNode::Text(_) => None,
//             DomNode::Element(ref el) => if el.has_id(id) {
//                 Some(self.clone())
//             } else {
//                 el.get_element_by_id(id)
//             }
//         }
//     }
// }

// impl HTMLElement {
//     fn has_id(&self, id: &str) -> bool {
//         if let Some(my_id) = self.attributes.get("id") {
//             my_id == id
//         } else {
//             false
//         }
//     }
//     fn get_element_by_id(&self, id: &str) -> Option<DomNode> {
//         for node in self.children.iter() {
//             if let Some(ret) = node.get_element_by_id(id) {
//                 return Some(ret)
//             }
//         }
//         None
//     }
// }

pub fn escape_default(s: &str) -> String {
    s.chars().flat_map(|c| c.escape_default()).collect()
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct OrderListResponse {
    meta: ResponseMeta,
    response: Response,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ResponseMeta {
    code: i32,
    error: String,
    info: String,
}
#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum Response {
    String(String),
    List(Vec<Order>)
}
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct Order {
    order_id: i32,
    order_tracker_link: String,
    #[serde(with = "date_parsing")]
    time_ordered: DateTime<Local>,
    #[serde(default)]
    status: OrderStatus,
    order_status_image: String,
}

impl Order {
    pub fn update_status(&mut self) -> bool {
        let old_status = self.status;
        let new_status = determine_status(&self.order_status_image);
        self.status = new_status;
        old_status != new_status
    }
}

impl ::std::cmp::PartialEq for Order {
    fn eq(&self, other: &Self) -> bool {
        self.order_id == other.order_id &&
        self.time_ordered == other.time_ordered
    }
}

impl Default for OrderStatus {
    fn default() -> OrderStatus {
        OrderStatus::Unknown
    }
}

mod date_parsing {
    use chrono::prelude::*;
    use serde::{de::{Deserialize, Deserializer, Error as DeErr}};
    use super::Error;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Local>, D::Error>
    where D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        parse_date_str(&s).map_err(DeErr::custom)
    }

    fn parse_date_str(s: &str) -> Result<DateTime<Local>, Error> {
        let mut parts = s.split_whitespace();
        let _dow = parts.next();
        let day = parts.next().ok_or(Error::Other("failed to get day from date string".into()))?.parse()?;
        let month_str = parts.next().ok_or(Error::Other("failed to get month str from date string".into()))?;
        let month = parse_month(month_str)?;
        let year = parts.next().ok_or(Error::Other("failed to get year from date string".into()))?.parse()?;
        let mut time = parts.next().ok_or(Error::Other("failed to get time from date string".into()))?.split(":");
        let hour = time.next().ok_or(Error::Other("failed to get hour from date string".into()))?.parse()?;
        let minute = time.next().ok_or(Error::Other("failed to get minute from date string".into()))?.parse()?;
        let sec = time.next().ok_or(Error::Other("failed to get sec from date string".into()))?.parse()?;
        let ret = Local.ymd(year, month, day).and_hms(hour, minute, sec);
        Ok(ret)
    }

    fn parse_month(m: &str) -> Result<u32, Error> {
        match m {
            "Jan" => Ok(1),
            "Feb" => Ok(2),
            "Mar" | "March" => Ok(3),
            "Apr" | "April" => Ok(4),
            "May" => Ok(5),
            "Jun" | "June" => Ok(6),
            "Jul" | "July" => Ok(7),
            "Aug" => Ok(8),
            "Sep" => Ok(9),
            "Oct" => Ok(10),
            "Nov" => Ok(11),
            "Dec" => Ok(12),
            _ => Err(Error::Other(format!("Unable to parse month str: {}", m)))
        }
    }
}

#[derive(Deserialize, Debug)]
struct Config {
    pub check_interval: usize,
    pub users: Vec<User>,
    pub check_addresses: Vec<String>,
    pub consecutive_errors_limit: usize,
    pub from_addr: String,
}

#[derive(Deserialize, Clone, Debug)]
struct User {
    pub name: String,
    pub carrier: String,
    pub phone_number: PhoneNumber,
    #[serde(default)]
    pub orders: Vec<Order>
}

impl User {
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

    fn dashes_string(&self) -> String {
        format!("{}-{}-{}", self.area_code, self.prefix, self.suffix)
    }

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
    fn from(other: reqwest::Error) -> Self {
        Error::Reqwest(other)
    }
}

impl From<serde_json::Error> for Error {
    fn from(other: serde_json::Error) -> Self {
        Error::Json(other)
    }
}

impl From<chrono::ParseError> for Error {
    fn from(other: chrono::ParseError) -> Self {
        Error::Time(other)
    }
}

impl From<::std::num::ParseIntError>  for Error {
    fn from(other: ::std::num::ParseIntError) -> Self {
        Error::Parse(other)
    }
}

impl From<::std::io::Error> for Error {
    fn from(other: ::std::io::Error) -> Self {
        Error::Io(other)
    }
}

impl From<cheap_alerts::Error> for Error {
    fn from(other: cheap_alerts::Error) -> Self {
        Error::Cheap(other)
    }
}

impl From<toml::de::Error> for Error {
    fn from(other: toml::de::Error) -> Self {
        Error::Toml(other)
    }
}

impl Error {
    fn other(s: &str) -> Self {
        Error::Other(s.into())
    }
}