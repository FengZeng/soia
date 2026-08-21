use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

/// Returns the preferred private IPv4 address for services intended for peers on the current LAN.
pub(crate) fn preferred_private_ipv4() -> Result<Ipv4Addr, String> {
    let mut candidates: Vec<(u8, Ipv4Addr)> = if_addrs::get_if_addrs()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|interface| !is_virtual_interface(&interface.name))
        .filter_map(|interface| match interface.addr {
            if_addrs::IfAddr::V4(address) => Some(address.ip),
            if_addrs::IfAddr::V6(_) => None,
        })
        .filter(|ip| is_usable_private_ipv4(*ip))
        .map(|ip| (private_ip_priority(ip), ip))
        .collect();

    candidates.sort_by_key(|(priority, ip)| (*priority, *ip));
    candidates
        .first()
        .map(|(_, ip)| *ip)
        .ok_or_else(|| "No private local network address found".to_string())
}

/// Uses the OS routing table to select the local IPv4 address that reaches a specific receiver.
/// The result is checked against the same physical-interface policy as Web Remote before it is
/// embedded in a cast URL.
#[allow(dead_code)]
pub(crate) fn local_ipv4_for_target(target: Ipv4Addr) -> Result<Ipv4Addr, String> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).map_err(|error| error.to_string())?;
    socket
        .connect(SocketAddr::new(IpAddr::V4(target), 9))
        .map_err(|error| error.to_string())?;
    let IpAddr::V4(local_ip) = socket.local_addr().map_err(|error| error.to_string())?.ip() else {
        return Err("No IPv4 route to receiver".to_string());
    };
    let is_eligible = if_addrs::get_if_addrs()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|interface| !is_virtual_interface(&interface.name))
        .any(|interface| matches!(interface.addr, if_addrs::IfAddr::V4(address) if address.ip == local_ip));
    if !is_eligible || !is_usable_private_ipv4(local_ip) {
        return Err("No eligible private IPv4 route to receiver".to_string());
    }
    Ok(local_ip)
}

pub(crate) fn is_virtual_interface(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["lo", "utun", "tun", "tap", "docker", "vbox", "vmnet", "bridge"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

pub(crate) fn is_usable_private_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_private()
        && !ip.is_loopback()
        && !ip.is_unspecified()
        && !(ip.octets()[0] == 198 && matches!(ip.octets()[1], 18 | 19))
}

fn private_ip_priority(ip: Ipv4Addr) -> u8 {
    match ip.octets() {
        [192, 168, ..] => 0,
        [10, ..] => 1,
        [172, 16..=31, ..] => 2,
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::{is_usable_private_ipv4, is_virtual_interface};
    use std::net::Ipv4Addr;

    #[test]
    fn excludes_virtual_interfaces_and_non_lan_addresses() {
        assert!(is_virtual_interface("utun4"));
        assert!(is_virtual_interface("docker0"));
        assert!(!is_virtual_interface("en0"));
        assert!(is_usable_private_ipv4(Ipv4Addr::new(192, 168, 1, 10)));
        assert!(!is_usable_private_ipv4(Ipv4Addr::new(127, 0, 0, 1)));
        assert!(!is_usable_private_ipv4(Ipv4Addr::new(198, 18, 0, 1)));
    }
}
