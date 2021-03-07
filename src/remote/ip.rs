extern crate get_if_addrs;
use std::net::IpAddr;

#[allow(dead_code)] // unused for now, but might be useful in future
pub fn host() -> IpAddr {
    get_if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .find(|i| !i.is_loopback())
        .map(|i| i.addr.ip())
        .unwrap_or_else(|| IpAddr::from([127, 0, 0, 1]))
}
