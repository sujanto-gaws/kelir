//! Where a test call may connect (FR-INT-002, #547; ADR-0043 §2, the product
//! owner's answer 2 of 2026-09-29).
//!
//! **The address is judged, not the name.** A host name is whatever its zone
//! answers today, so the guard runs on every address the name resolved to, and
//! the connection is then pinned to an address that passed
//! (`integration::outbound`). This module is the judgement alone: pure, and
//! tested address by address.
//!
//! | Class | Verdict |
//! |---|---|
//! | Loopback, link-local (`169.254.169.254` included), unspecified, multicast, IPv4 broadcast | **Always refused.** No setting opens them |
//! | Private: RFC 1918, IPv6 unique-local `fc00::/7` | Refused unless inside a CIDR in `KELIR_INTEGRATION_ALLOWED_CIDRS` |
//! | Everything else | Allowed |
//!
//! **An IPv4-mapped IPv6 address is judged as the IPv4 address it carries**:
//! `::ffff:127.0.0.1` is loopback, and `::ffff:10.0.0.1` is private.
//!
//! Two readings that go slightly beyond the words of the decision, both in the
//! refusing direction: **all of `0.0.0.0/8` is unspecified**, not only
//! `0.0.0.0`, because an address in it reaches the local host on Linux; and
//! **`255.255.255.255`** is refused with multicast, as the other address that
//! reaches more than one host.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::utils::cidr::{canonical, Cidr};

/// What kind of address something resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressClass {
    Public,
    Private,
    Loopback,
    LinkLocal,
    Unspecified,
    Multicast,
}

impl AddressClass {
    /// The words a refusal uses. No address in them: the class is what the
    /// administrator can act on.
    pub fn describe(self) -> &'static str {
        match self {
            Self::Public => "a public address",
            Self::Private => "a private address",
            Self::Loopback => "a loopback address",
            Self::LinkLocal => "a link-local address",
            Self::Unspecified => "an unspecified address",
            Self::Multicast => "a multicast or broadcast address",
        }
    }
}

impl fmt::Display for AddressClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.describe())
    }
}

/// Classifies one address.
pub fn classify(address: IpAddr) -> AddressClass {
    match canonical(address) {
        IpAddr::V4(v4) => classify_v4(v4),
        IpAddr::V6(v6) => classify_v6(v6),
    }
}

fn classify_v4(address: Ipv4Addr) -> AddressClass {
    let [first, second, ..] = address.octets();

    if address.is_loopback() {
        AddressClass::Loopback
    } else if first == 0 {
        AddressClass::Unspecified
    } else if address.is_link_local() {
        AddressClass::LinkLocal
    } else if address.is_multicast() || address.is_broadcast() {
        AddressClass::Multicast
    } else if first == 10
        || (first == 172 && (16..=31).contains(&second))
        || (first == 192 && second == 168)
    {
        AddressClass::Private
    } else {
        AddressClass::Public
    }
}

fn classify_v6(address: Ipv6Addr) -> AddressClass {
    let first = address.segments()[0];

    if address.is_loopback() {
        AddressClass::Loopback
    } else if address.is_unspecified() {
        AddressClass::Unspecified
    } else if address.is_multicast() {
        AddressClass::Multicast
    } else if first & 0xffc0 == 0xfe80 {
        // fe80::/10
        AddressClass::LinkLocal
    } else if first & 0xfe00 == 0xfc00 {
        // fc00::/7, unique-local
        AddressClass::Private
    } else {
        AddressClass::Public
    }
}

/// The deployment's rules for one call: its private allow-list, and the test
/// seam (`AppConfig::integration_allow_loopback`).
#[derive(Debug, Clone, Default)]
pub struct EgressPolicy {
    pub allowed_cidrs: Vec<Cidr>,
    /// Never `true` outside the test harness; see the config field.
    pub allow_loopback: bool,
}

/// An address the guard refused, and why. Carries the class and not the
/// address, because the refusal is shown to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EgressRefusal {
    pub class: AddressClass,
}

impl EgressPolicy {
    /// Whether a connection to `address` is allowed.
    pub fn check(&self, address: IpAddr) -> Result<(), EgressRefusal> {
        let class = classify(address);

        match class {
            AddressClass::Public => Ok(()),
            AddressClass::Private
                if self.allowed_cidrs.iter().any(|cidr| cidr.contains(address)) =>
            {
                Ok(())
            }
            AddressClass::Loopback if self.allow_loopback => Ok(()),
            _ => Err(EgressRefusal { class }),
        }
    }

    /// Every address must pass, not one of them: a name that resolves to a
    /// public and a private address is a name the connection could take to
    /// either. Answers the first address in `addresses` order that passed, the
    /// one the connection is pinned to.
    pub fn choose(&self, addresses: &[IpAddr]) -> Result<Option<IpAddr>, EgressRefusal> {
        for address in addresses {
            self.check(*address)?;
        }

        Ok(addresses.first().copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(raw: &str) -> IpAddr {
        raw.parse().expect("an address")
    }

    fn cidr(raw: &str) -> Cidr {
        raw.parse().expect("a CIDR")
    }

    #[test]
    fn each_address_is_classified_as_the_decision_names_it() {
        let cases = [
            ("127.0.0.1", AddressClass::Loopback),
            ("127.255.0.9", AddressClass::Loopback),
            ("::1", AddressClass::Loopback),
            ("169.254.169.254", AddressClass::LinkLocal),
            ("169.254.0.1", AddressClass::LinkLocal),
            ("fe80::1", AddressClass::LinkLocal),
            ("febf::1", AddressClass::LinkLocal),
            ("0.0.0.0", AddressClass::Unspecified),
            ("0.1.2.3", AddressClass::Unspecified),
            ("::", AddressClass::Unspecified),
            ("224.0.0.1", AddressClass::Multicast),
            ("239.255.255.250", AddressClass::Multicast),
            ("255.255.255.255", AddressClass::Multicast),
            ("ff02::1", AddressClass::Multicast),
            ("10.0.0.1", AddressClass::Private),
            ("172.16.0.1", AddressClass::Private),
            ("172.31.255.254", AddressClass::Private),
            ("192.168.1.1", AddressClass::Private),
            ("fc00::1", AddressClass::Private),
            ("fd12:3456::1", AddressClass::Private),
            ("8.8.8.8", AddressClass::Public),
            ("172.15.0.1", AddressClass::Public),
            ("172.32.0.1", AddressClass::Public),
            ("192.169.0.1", AddressClass::Public),
            ("2001:4860:4860::8888", AddressClass::Public),
            ("fec0::1", AddressClass::Public),
        ];

        for (address, class) in cases {
            assert_eq!(classify(ip(address)), class, "{address}");
        }
    }

    #[test]
    fn an_ipv4_mapped_address_is_judged_as_the_address_it_carries() {
        assert_eq!(classify(ip("::ffff:127.0.0.1")), AddressClass::Loopback);
        assert_eq!(
            classify(ip("::ffff:169.254.169.254")),
            AddressClass::LinkLocal
        );
        assert_eq!(classify(ip("::ffff:0.0.0.0")), AddressClass::Unspecified);
        assert_eq!(classify(ip("::ffff:224.0.0.1")), AddressClass::Multicast);
        assert_eq!(classify(ip("::ffff:10.1.2.3")), AddressClass::Private);
        assert_eq!(classify(ip("::ffff:8.8.8.8")), AddressClass::Public);
    }

    #[test]
    fn with_no_allow_list_only_public_addresses_pass() {
        let policy = EgressPolicy::default();

        assert!(policy.check(ip("8.8.8.8")).is_ok());
        for refused in [
            "127.0.0.1",
            "::1",
            "169.254.169.254",
            "fe80::1",
            "0.0.0.0",
            "::",
            "224.0.0.1",
            "10.0.0.1",
            "192.168.0.1",
            "fd00::1",
            "::ffff:127.0.0.1",
            "::ffff:192.168.0.1",
        ] {
            assert!(policy.check(ip(refused)).is_err(), "{refused}");
        }
    }

    #[test]
    fn a_listed_private_range_opens_that_range_only() {
        let policy = EgressPolicy {
            allowed_cidrs: vec![cidr("10.20.0.0/16"), cidr("fd00:1::/32")],
            allow_loopback: false,
        };

        assert!(policy.check(ip("10.20.3.4")).is_ok());
        assert!(policy.check(ip("::ffff:10.20.3.4")).is_ok());
        assert!(policy.check(ip("fd00:1::9")).is_ok());
        assert_eq!(
            policy.check(ip("10.21.3.4")),
            Err(EgressRefusal {
                class: AddressClass::Private
            })
        );
        assert!(policy.check(ip("192.168.0.1")).is_err());
    }

    #[test]
    fn no_allow_list_opens_an_always_refused_range() {
        let policy = EgressPolicy {
            allowed_cidrs: vec![
                cidr("127.0.0.0/8"),
                cidr("169.254.0.0/16"),
                cidr("0.0.0.0/0"),
                cidr("::/0"),
            ],
            allow_loopback: false,
        };

        for refused in [
            "127.0.0.1",
            "169.254.169.254",
            "::1",
            "fe80::1",
            "224.0.0.1",
            "0.0.0.0",
        ] {
            assert!(policy.check(ip(refused)).is_err(), "{refused}");
        }
        // The same list does open a private address, so the refusals above
        // are the class and not a list that failed to match.
        assert!(policy.check(ip("10.0.0.1")).is_ok());
    }

    #[test]
    fn the_test_seam_opens_loopback_and_nothing_else() {
        let policy = EgressPolicy {
            allowed_cidrs: Vec::new(),
            allow_loopback: true,
        };

        assert!(policy.check(ip("127.0.0.1")).is_ok());
        assert!(policy.check(ip("::1")).is_ok());
        assert!(policy.check(ip("169.254.169.254")).is_err());
        assert!(policy.check(ip("10.0.0.1")).is_err());
    }

    #[test]
    fn every_resolved_address_must_pass() {
        let policy = EgressPolicy::default();

        assert_eq!(
            policy.choose(&[ip("8.8.8.8"), ip("10.0.0.1")]),
            Err(EgressRefusal {
                class: AddressClass::Private
            }),
            "one private answer refuses the name"
        );
        assert_eq!(
            policy.choose(&[ip("8.8.8.8"), ip("1.1.1.1")]),
            Ok(Some(ip("8.8.8.8"))),
            "the first address that passed is the one pinned"
        );
        assert_eq!(policy.choose(&[]), Ok(None));
    }
}
