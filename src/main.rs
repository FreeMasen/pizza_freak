extern crate chrono;
extern crate dirs;
extern crate env_logger;
extern crate html5ever;
extern crate lettre;
#[macro_use]
extern crate log;
extern crate reqwest;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate toml;

use std::default::Default;
use std::collections::HashMap;

use chrono::{
    DateTime,
    Local,
    Duration
};
use dirs::home_dir;
use html5ever::{
    tendril::TendrilSink,
    rcdom::{
        NodeData,
        Handle
    }
};
use lettre::{
    SimpleSendableEmail,
    EmailTransport,
    SmtpTransport
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
        for mut user in users.iter_mut() {
            match update_orders(user) {
                Ok(_) => {
                    info!(target: "pizza_freak:info", "Successfully updated orders for {}", user.name);
                },
                Err(e) => {
                    errored = true;
                    error!(target: "pizza_freak:error", "Error updating orders for {}: {}", user.name, e);
                },
            }
        }
        if errored {
            consecutive_errors += 1
        } else {
            consecutive_errors = 0;
        }
        if consecutive_errors > config.consecutive_errors_limit {
            error!(target: "pizza_freak:error", "Too many consecutive errors, exiting program");
            break;
        }
        ::std::thread::sleep(::std::time::Duration::from_millis(config.check_interval as u64))
    }
    Ok(())
}

fn get_config() -> Result<Config, Error> {
    let mut config_path = home_dir().ok_or(Error::other("Unable to get home directory"))?;
    config_path.push(".pizza_freak");
    let config_text = ::std::fs::read_to_string(config_path)?;
    let config = toml::from_str(&config_text)?;
    Ok(config)
}

fn init_logging() {
    let mut b = env_logger::Builder::from_default_env();
    b.target(env_logger::Target::Stdout);
    b.init();
}

fn update_orders(user: &mut User) -> Result<(), Error> {
    debug!(target: "pizza_freak:debug", "updating orders for {}", user.name);
    let result = get_order_list(&user.phone_number.dashes_string())?;
    debug!(target: "pizza_freak:debug", "got orders from phone numbers");
    if let Response::List(orders) = result.response {
        for order in orders {
            if !user.orders.iter().any(|o| o == &order) {
                user.orders.push(order);
            }
        }
    }
    debug!(target: "pizza_freak:debug", "updated user's orders {:?}", user.orders);
    let mut changes = vec![];
    for mut order in user.orders.iter_mut() {
        let new_status = get_order_status(&order.order_tracker_link)?;
        debug!(target: "pizza_freak:debug", "old_status: {:?}, new_status: {:?}", order.status, new_status);
        if new_status != order.status {
            changes.push((order.order_id, new_status));
        }
        order.status = new_status;
    }
    for change in changes {
        send_update(&format!("Pizza Freak Order #{}\n{}", change.0, change.1), &user.phone_number.to_string(), &user.phone_email_url)?;
    }
    user.orders.retain(|o| o.time_ordered.signed_duration_since(Local::now()) > Duration::days(1));
    Ok(())
}

fn send_update(msg: &str, ph: &str, email_suffix: &str) -> Result<(), Error> {
    debug!(target: "pizza_freak:debug", "sending update: {}", msg);
    let address = format!("{}@{}", ph, email_suffix);
    let id = format!("{}", Local::now().timestamp_millis());
    let msg = SimpleSendableEmail::new(
                "rfm@robertmasen.pizza".to_string(),
                &[address],
                id,
                msg.to_string(),
    )?;
    let mut mailer =
        SmtpTransport::builder_unencrypted_localhost()?.build();
    mailer.send(&msg)?;
    Ok(())
}

fn get_order_list(phone_number: &str) -> Result<OrderListResponse, Error> {
    let mut res = get(&format!("https://downtown.pizzaluce.com/ws/ordertracker/orders?phone={}", phone_number))?;
    let text = res.text()?;
    let ret = serde_json::from_str(&text)?;
    Ok(ret)
}
fn get_order_status(url: &str) -> Result<OrderStatus, Error> {
    debug!(target: "pizza_freak:debug", "getting order status");
    let html = get(url)?.text()?;
    let status = extract_order_status(html)?;
    debug!(target: "pizza_freak:debug", "order status: {}", status);
    Ok(status)
}
fn extract_order_status(html: String) -> Result<OrderStatus, Error> {
    let p = html5ever::parse_document(html5ever::rcdom::RcDom::default(),
                                            html5ever::ParseOpts::default())
                        .from_utf8()
                        .read_from(&mut html.as_bytes())?;
    let dom = convert(p.document);
    if let Some(current_step) = dom.get_element_by_id("currentStep") {
        if let DomNode::Element(mut current_step) = current_step {
            let inner_text = current_step.children.pop().ok_or(Error::Other(String::from("Current Step DOM node needs at least 1 child")))?;
            match inner_text {
                DomNode::Text(val) => Ok(determine_status(&val)),
                _ => Err(Error::Other(String::from("First child of current step node must be a text node"))),
            }
        } else {
            Err(Error::Other(format!("Current step must be an element, not just raw text")))
        }
    } else {
        Err(Error::Other(String::from("Unable to find current step in the dom")))
    }
}

fn convert(handle: Handle) -> DomNode {
    let mut ret = DomNode::Text(String::from("unparsed"));
    for child in handle.children.borrow().iter() {
        match child.data {
            NodeData::Element { ref name, ref attrs, ..} => {
                let name_str = format!("{}", name.local);
                if name_str == "html" {
                    ret = DomNode::Element(HTMLElement {
                        name: name_str,
                        attributes: attrs.borrow().iter().map(|a| (format!("{}", a.name.local), format!("{}", a.value))).collect(),
                        children: convert_children(handle.clone()),
                    })
                }
            },
            _ => ()
        }
    }
    ret
}

fn convert_children(parent: Handle) -> Vec<DomNode> {
    let mut ret = vec![];
    for child in parent.children.borrow().iter() {
        match child.data {
            NodeData::Element { ref name, ref attrs, ..} => {
                let name = format!("{}", name.local);
                let attributes = attrs.borrow().iter().map(|a| (format!("{}", a.name.local), format!("{}", a.value))).collect();
                let children = convert_children(child.clone());
                ret.push(DomNode::Element(HTMLElement {
                    name,
                    attributes,
                    children
                }))
            },
            NodeData::Text { ref contents } => {
                ret.push(DomNode::Text(escape_default(&contents.borrow())))
            },
            _ => ()
        }
    }
    ret
}
#[derive(Debug, Deserialize, Clone, Copy, PartialEq)]
#[repr(u8)]
enum OrderStatus {
    Deferred,
    Reviewing,
    Pending,
    Cooking,
    OutForDelivery,
    Delivered,
    Unknown,
}

fn determine_status(val: &str) -> OrderStatus {
    match val {
        "0" => OrderStatus::Deferred,
        "1" => OrderStatus::Reviewing,
        "2" => OrderStatus::Pending,
        "3" => OrderStatus::Cooking,
        "4" => OrderStatus::OutForDelivery,
        "5" => OrderStatus::Delivered,
        _ => OrderStatus::Unknown,
    }
}

impl ::std::fmt::Display for OrderStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            OrderStatus::Deferred => write!(f, "Deferred, The store might not be open?"),
            OrderStatus::Reviewing => write!(f, "Reviewing, Management is checking things over apparently"),
            OrderStatus::Pending => write!(f, "Pending, Not yet being made but not held up by anything"),
            OrderStatus::Cooking => write!(f, "The cooks are working on your order now!"),
            OrderStatus::OutForDelivery => write!(f, "The driver is heading to your house!"),
            OrderStatus::Delivered => write!(f, "You are eating pizza!"),
            OrderStatus::Unknown => write!(f, "I don't know what status this is, are you sure you ordered a pizza?"),
        }
    }
}


#[derive(Debug, Clone)]
enum DomNode {
    Text(String),
    Element(HTMLElement),
}

#[derive(Debug, Clone)]
struct HTMLElement {
    name: String,
    attributes: HashMap<String, String>,
    children: Vec<DomNode>
}

impl DomNode {
    fn get_element_by_id(&self, id: &str) -> Option<DomNode> {
        match self {
            DomNode::Text(_) => None,
            DomNode::Element(ref el) => if el.has_id(id) {
                Some(self.clone())
            } else {
                el.get_element_by_id(id)
            }
        }
    }
}

impl HTMLElement {
    fn has_id(&self, id: &str) -> bool {
        if let Some(my_id) = self.attributes.get("id") {
            my_id == id
        } else {
            false
        }
    }
    fn get_element_by_id(&self, id: &str) -> Option<DomNode> {
        for node in self.children.iter() {
            if let Some(ret) = node.get_element_by_id(id) {
                return Some(ret)
            }
        }
        None
    }
}

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

#[derive(Deserialize)]
struct Config {
    pub check_interval: usize,
    pub users: Vec<User>,
    pub check_address: String,
    pub consecutive_errors_limit: usize,
}

#[derive(Deserialize, Clone)]
struct User {
    pub name: String,
    pub phone_email_url: String,
    pub phone_number: PhoneNumber,
    #[serde(default)]
    pub orders: Vec<Order>
}

#[derive(Clone)]
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
            area_code: area_code,
            prefix: prefix,
            suffix: suffix,
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
    Email(lettre::Error),
    Stmp(lettre::smtp::error::Error),
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
            Error::Email(ref e) => e.fmt(f),
            Error::Stmp(ref e) => e.fmt(f),
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

impl From<lettre::Error> for Error {
    fn from(other: lettre::Error) -> Self {
        Error::Email(other)
    }
}
impl From<lettre::smtp::error::Error> for Error {
    fn from(other: lettre::smtp::error::Error) -> Self {
        Error::Stmp(other)
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