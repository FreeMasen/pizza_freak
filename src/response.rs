use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Response {
    Order(Order),
    NoOrder(NoOrder),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoOrder {
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    pub order_id: String,
    pub order_secondary_id: String,
    pub order_key: String,
    pub status: Status,
    pub has_baking_step: bool,
    pub delivery_address_lat: f32,
    pub delivery_address_lng: f32,
    pub driver_lat: f32,
    pub driver_lng: f32,
    pub driver_destination_radius: f32,
    pub driver_name: String,
    pub show_actual_driver_location: bool,
    pub veteran_driver: bool,
    pub driver_gps_off: bool,
    pub print_receipt_button_state: String,
    pub reorder_button_state: String,
    pub last_gps_refresh: u32,
    pub store_lat: f32,
    pub store_lng: f32,
    pub eligible_for_late_coupon: bool,
    pub has_registered_web_customer: bool,
    pub links: Vec<Link>,
    #[serde(default = "chrono::Local::now")]
    pub first_seen: chrono::DateTime<chrono::Local>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Status {
    Making,
    OnTheWay,
    PickupReady,
    Delivered,
    PickedUp,
    Deferred,
    Questionnaire,
    Suspended,
    Canceled,
    Reviewing,
    Cooking,
    MakingEmulated,
    CookingEmulated,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "rel", content = "href", rename_all = "camelCase")]
pub enum Link {
    Order(String),
    LateAward(String),
    DriverPhoto(String,)
}