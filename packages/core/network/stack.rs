use core::ffi::CStr;
use std::net::{Ipv4Addr, Ipv6Addr};

use crate::support::bridge::AstralRustString;

pub struct InterfaceAddress {
    pub interface: String,
    pub address: String,
    pub ipv6: bool,
}

fn is_loopback(entry: &InterfaceAddress) -> bool {
    entry.interface.starts_with("lo") || entry.address == "127.0.0.1" || entry.address == "::1"
}

fn is_translated_ipv4(entry: &InterfaceAddress) -> bool {
    !entry.ipv6 && (entry.interface.starts_with("v4-") || entry.address.starts_with("192.0.0."))
}

fn is_global_ipv6(entry: &InterfaceAddress) -> bool {
    entry.ipv6 && !entry.address.starts_with("fe80") && !entry.address.starts_with("fd") && !entry.address.starts_with("fc")
}

pub fn marker(addresses: &[InterfaceAddress]) -> &'static str {
    let external: Vec<&InterfaceAddress> = addresses.iter().filter(|entry| !is_loopback(entry)).collect();
    let native_ipv4 = external.iter().any(|entry| !entry.ipv6 && !is_translated_ipv4(entry));
    let global_ipv6 = external.iter().any(|entry| is_global_ipv6(entry));
    if native_ipv4 || !global_ipv6 {
        return "";
    }
    if external.iter().any(|entry| is_translated_ipv4(entry)) {
        "IPv6-only network (464XLAT)"
    } else {
        "IPv6-only network"
    }
}

fn address_text(address: &libc::sockaddr) -> Option<(String, bool)> {
    match i32::from(address.sa_family) {
        libc::AF_INET => {
            let ipv4 = unsafe { &*(address as *const libc::sockaddr).cast::<libc::sockaddr_in>() };
            Some((Ipv4Addr::from(u32::from_be(ipv4.sin_addr.s_addr)).to_string(), false))
        }
        libc::AF_INET6 => {
            let ipv6 = unsafe { &*(address as *const libc::sockaddr).cast::<libc::sockaddr_in6>() };
            Some((Ipv6Addr::from(ipv6.sin6_addr.s6_addr).to_string(), true))
        }
        _ => None,
    }
}

pub fn interface_addresses() -> Vec<InterfaceAddress> {
    let mut addresses = Vec::new();
    let mut list: *mut libc::ifaddrs = core::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut list) } != 0 {
        return addresses;
    }
    let mut entry = list;
    while let Some(current) = unsafe { entry.as_ref() } {
        entry = current.ifa_next;
        let flags = current.ifa_flags as i32;
        let up = flags & libc::IFF_UP != 0 && flags & libc::IFF_RUNNING != 0;
        let Some(address) = (unsafe { current.ifa_addr.as_ref() }) else {
            continue;
        };
        if !up {
            continue;
        }
        let Some((text, ipv6)) = address_text(address) else { continue };
        let interface = if current.ifa_name.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(current.ifa_name) }.to_string_lossy().into_owned()
        };
        addresses.push(InterfaceAddress {
            interface,
            address: text,
            ipv6,
        });
    }
    unsafe { libc::freeifaddrs(list) };
    addresses
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_current_network_stack_marker() -> AstralRustString {
    AstralRustString::new(marker(&interface_addresses()).as_bytes().to_vec())
}
