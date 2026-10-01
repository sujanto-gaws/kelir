//! An IP network written as `address/prefix` — `10.20.0.0/16`, `fd00::/8`.
//!
//! Hand-written rather than taken from a crate because the whole of what is
//! needed is parsing and one containment test, and the containment test is the
//! part a reviewer of an egress guard has to be able to read (FR-INT-002, #547).
//!
//! **An IPv4 network contains an IPv4-mapped IPv6 address of that network.**
//! `::ffff:10.0.0.5` is `10.0.0.5` on the wire, so asking whether `10.0.0.0/8`
//! contains it and answering *no* would make the allow-list depend on how the
//! resolver chose to spell the answer. The IPv4-compatible form `::10.0.0.5`
//! is read the same way (#622).

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cidr {
    network: IpAddr,
    prefix: u8,
}

/// Why a CIDR did not parse. The text is the caller's, so it is echoed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CidrParseError(pub String);

impl fmt::Display for CidrParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "'{}' is not a CIDR (expected address/prefix, e.g. 10.20.0.0/16)",
            self.0
        )
    }
}

impl std::error::Error for CidrParseError {}

impl Cidr {
    pub fn prefix(&self) -> u8 {
        self.prefix
    }

    /// Whether `address` lies inside this network.
    pub fn contains(&self, address: IpAddr) -> bool {
        let address = canonical(address);

        match (self.network, address) {
            (IpAddr::V4(network), IpAddr::V4(address)) => {
                let mask = mask32(self.prefix);
                u32::from(network) & mask == u32::from(address) & mask
            }
            (IpAddr::V6(network), IpAddr::V6(address)) => {
                let mask = mask128(self.prefix);
                u128::from(network) & mask == u128::from(address) & mask
            }
            _ => false,
        }
    }
}

impl FromStr for Cidr {
    type Err = CidrParseError;

    /// `address/prefix`. A bare address is refused rather than read as a host
    /// route: an operator who wrote `10.0.0.0` meant a network, and guessing
    /// `/32` would allow one address where they meant sixteen million or none.
    ///
    /// The address is masked to its prefix, so `10.1.2.3/8` reads as
    /// `10.0.0.0/8` — the network the operator named, whatever host bits came
    /// with it.
    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let error = || CidrParseError(raw.to_owned());
        let (address, prefix) = raw.trim().split_once('/').ok_or_else(error)?;
        let address: IpAddr = address.trim().parse().map_err(|_| error())?;
        let prefix: u8 = prefix.trim().parse().map_err(|_| error())?;

        let network = match address {
            IpAddr::V4(v4) if prefix <= 32 => IpAddr::V4((u32::from(v4) & mask32(prefix)).into()),
            IpAddr::V6(v6) if prefix <= 128 => {
                IpAddr::V6((u128::from(v6) & mask128(prefix)).into())
            }
            _ => return Err(error()),
        };

        Ok(Self { network, prefix })
    }
}

impl fmt::Display for Cidr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.network, self.prefix)
    }
}

/// A comma-separated list, blanks skipped. The first entry that does not parse
/// fails the whole list: half an allow-list is not what the operator wrote.
pub fn parse_list(raw: &str) -> Result<Vec<Cidr>, CidrParseError> {
    raw.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::parse)
        .collect()
}

/// An IPv4-mapped (`::ffff:a.b.c.d`) or IPv4-compatible (`::a.b.c.d`) IPv6
/// address as the IPv4 address it carries; anything else unchanged.
///
/// The compatible form is deprecated (RFC 4291 §2.5.5.1) and still parses,
/// so it is read rather than trusted to be unroutable (#622). `::` and `::1`
/// lie in the same `::/96` and are IPv6's own unspecified and loopback
/// addresses, not IPv4 addresses.
pub fn canonical(address: IpAddr) -> IpAddr {
    match address {
        IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .or_else(|| ipv4_compatible(v6))
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(v6)),
        v4 => v4,
    }
}

fn ipv4_compatible(address: Ipv6Addr) -> Option<Ipv4Addr> {
    let bits = u128::from(address);

    (bits >> 32 == 0 && !address.is_unspecified() && !address.is_loopback())
        .then(|| Ipv4Addr::from(bits as u32))
}

fn mask32(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - u32::from(prefix))
    }
}

fn mask128(prefix: u8) -> u128 {
    if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - u32::from(prefix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cidr(raw: &str) -> Cidr {
        raw.parse().expect("a CIDR")
    }

    fn ip(raw: &str) -> IpAddr {
        raw.parse().expect("an address")
    }

    #[test]
    fn a_network_contains_its_own_addresses_and_no_others() {
        let net = cidr("10.20.0.0/16");

        assert!(net.contains(ip("10.20.0.1")));
        assert!(net.contains(ip("10.20.255.255")));
        assert!(!net.contains(ip("10.21.0.1")));
        assert!(!net.contains(ip("11.20.0.1")));
    }

    #[test]
    fn host_bits_in_the_written_address_are_masked_off() {
        assert_eq!(cidr("10.1.2.3/8").to_string(), "10.0.0.0/8");
        assert!(cidr("10.1.2.3/8").contains(ip("10.200.0.1")));
    }

    #[test]
    fn a_v4_network_contains_the_mapped_form_of_its_addresses() {
        assert!(cidr("10.0.0.0/8").contains(ip("::ffff:10.1.2.3")));
        assert!(!cidr("10.0.0.0/8").contains(ip("::ffff:11.1.2.3")));
    }

    #[test]
    fn a_v4_network_contains_the_compatible_form_of_its_addresses() {
        // #622: `::a.b.c.d`, the deprecated `::/96`.
        assert!(cidr("10.0.0.0/8").contains(ip("::10.1.2.3")));
        assert!(!cidr("10.0.0.0/8").contains(ip("::11.1.2.3")));
        assert_eq!(canonical(ip("::127.0.0.1")), ip("127.0.0.1"));
    }

    #[test]
    fn ipv6_loopback_and_unspecified_are_not_ipv4_compatible_addresses() {
        assert_eq!(canonical(ip("::1")), ip("::1"));
        assert_eq!(canonical(ip("::")), ip("::"));
        // One bit above the /96 is an IPv6 address like any other.
        assert_eq!(canonical(ip("::1:a00:1")), ip("::1:a00:1"));
        assert!(!cidr("10.0.0.0/8").contains(ip("::1:a00:1")));
    }

    #[test]
    fn a_v6_network_contains_v6_addresses_only() {
        let net = cidr("fd00:1234::/32");

        assert!(net.contains(ip("fd00:1234::5")));
        assert!(!net.contains(ip("fd00:1235::5")));
        assert!(!net.contains(ip("10.0.0.1")));
    }

    #[test]
    fn a_zero_prefix_contains_its_whole_family() {
        assert!(cidr("0.0.0.0/0").contains(ip("203.0.113.9")));
        assert!(!cidr("0.0.0.0/0").contains(ip("2001:db8::1")));
        assert!(cidr("::/0").contains(ip("2001:db8::1")));
    }

    #[test]
    fn a_full_prefix_is_one_address() {
        assert!(cidr("10.0.0.5/32").contains(ip("10.0.0.5")));
        assert!(!cidr("10.0.0.5/32").contains(ip("10.0.0.6")));
    }

    #[test]
    fn malformed_entries_are_refused() {
        for bad in [
            "10.0.0.0",
            "10.0.0.0/33",
            "fd00::/129",
            "ten/8",
            "10.0.0.0/x",
            "",
        ] {
            assert!(bad.parse::<Cidr>().is_err(), "{bad}");
        }
    }

    #[test]
    fn a_list_skips_blanks_and_fails_whole_on_one_bad_entry() {
        let list = parse_list(" 10.0.0.0/8 , ,fd00::/8").expect("parses");
        assert_eq!(list.len(), 2);

        assert!(parse_list("10.0.0.0/8,nonsense").is_err());
        assert!(parse_list("").expect("empty is fine").is_empty());
    }
}
