extern crate get_if_addrs;
use std::net::IpAddr;

pub fn host() -> IpAddr {
    get_if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .find(|i| !i.is_loopback())
        .map(|i| i.addr.ip())
        .unwrap_or_else(|| IpAddr::from([127, 0, 0, 1]))
}
