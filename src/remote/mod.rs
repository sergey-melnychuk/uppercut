extern crate get_if_addrs;
use std::net::IpAddr;

pub mod server;
pub mod client;

#[allow(dead_code)]
pub fn host() -> String {
    let ip = get_if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .find(|i| !i.is_loopback())
        .map(|i| i.addr.ip())
        .unwrap_or_else(|| IpAddr::from([127, 0, 0, 1]));
    ip.to_string()
}