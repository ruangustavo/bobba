use std::net::{IpAddr, Ipv4Addr};

pub const GAME_PORT: u16 = 30000;
pub const HOTEL_HOSTS: [&str; 10] = [
    "game-br.habbo.com",
    "game-de.habbo.com",
    "game-es.habbo.com",
    "game-fi.habbo.com",
    "game-fr.habbo.com",
    "game-it.habbo.com",
    "game-nl.habbo.com",
    "game-tr.habbo.com",
    "game-us.habbo.com",
    "game-s2.habbo.com",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirect {
    pub host: String,
    pub real_address: IpAddr,
    pub loopback: Ipv4Addr,
}

pub fn loopback_for(position: usize) -> Ipv4Addr {
    let k = position + 1;
    let third = u8::try_from(k / 254).unwrap_or(u8::MAX);
    let fourth = u8::try_from(1 + k % 254).unwrap_or(u8::MAX);
    Ipv4Addr::new(127, 0, third, fourth)
}

pub async fn resolve_hotels(hosts: &[&str]) -> Vec<Redirect> {
    let mut redirects = Vec::new();
    for host in hosts {
        let Ok(mut addresses) = tokio::net::lookup_host((*host, GAME_PORT)).await else {
            continue;
        };
        let Some(address) = addresses.find(|a| a.is_ipv4()) else {
            continue;
        };
        redirects.push(Redirect {
            host: (*host).to_owned(),
            real_address: address.ip(),
            loopback: loopback_for(redirects.len()),
        });
    }
    redirects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hands_out_loopbacks_like_gearth() {
        assert_eq!(loopback_for(0), Ipv4Addr::new(127, 0, 0, 2));
        assert_eq!(loopback_for(9), Ipv4Addr::new(127, 0, 0, 11));
        assert_eq!(loopback_for(253), Ipv4Addr::new(127, 0, 1, 1));
    }
}
