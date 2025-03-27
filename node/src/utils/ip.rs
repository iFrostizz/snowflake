use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub fn ip_octets(ip: IpAddr) -> Vec<u8> {
    let ip_v6 = match ip {
        IpAddr::V4(v4_addr) => v4_addr.to_ipv6_mapped(),
        IpAddr::V6(v6_addr) => v6_addr,
    };
    ip_v6.octets().to_vec()
}

pub fn ip_from_octets(mut octets: Vec<u8>) -> Result<IpAddr, std::io::Error> {
    if octets.len() == 16 && octets[0..10] == [0; 10] && octets[10..12] == [0xff, 0xff] {
        octets = octets[12..].to_vec();
    }

    if octets.len() == 4 {
        let a = octets[0];
        let b = octets[1];
        let c = octets[2];
        let d = octets[3];

        Ok(IpAddr::V4(Ipv4Addr::new(a, b, c, d)))
    } else if octets.len() == 16 {
        let a = two_bytes_u16(octets[0], octets[1]);
        let b = two_bytes_u16(octets[2], octets[3]);
        let c = two_bytes_u16(octets[4], octets[5]);
        let d = two_bytes_u16(octets[6], octets[7]);
        let e = two_bytes_u16(octets[8], octets[9]);
        let f = two_bytes_u16(octets[10], octets[11]);
        let g = two_bytes_u16(octets[12], octets[13]);
        let h = two_bytes_u16(octets[14], octets[15]);

        Ok(IpAddr::V6(Ipv6Addr::new(a, b, c, d, e, f, g, h)))
    } else {
        Err(std::io::ErrorKind::InvalidInput.into())
    }
}

fn two_bytes_u16(b1: u8, b2: u8) -> u16 {
    ((b1 as u16) << 8) | b2 as u16
}

#[cfg(test)]
mod tests {
    use super::{ip_from_octets, ip_octets};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[test]
    fn ip_octets_reversing() {
        let ip = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
        assert_eq!(ip_from_octets(ip_octets(ip)).unwrap(), ip);

        let ip = IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255));
        assert_eq!(ip_from_octets(ip_octets(ip)).unwrap(), ip);

        let ip = IpAddr::V6(Ipv6Addr::new(1, 2, 3, 4, 5, 6, 7, 8));
        assert_eq!(ip_from_octets(ip_octets(ip)).unwrap(), ip);

        let ip = IpAddr::V6(Ipv6Addr::new(255, 255, 255, 255, 255, 255, 255, 255));
        assert_eq!(ip_from_octets(ip_octets(ip)).unwrap(), ip);
    }
}
