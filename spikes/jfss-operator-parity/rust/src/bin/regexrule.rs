//! The Validation Rule Registry's `regex` rule (scope `both`) evaluated with the
//! Rust `regex` crate against the ECMA-262 patterns the registry's params schema
//! admits. Prints, per pattern, whether Rust can compile it at all and what it
//! decides for a sample input.
use regex::Regex;

const CASES: &[(&str, &str, &str)] = &[
    ("plain", r"^[A-Z]{3}-\d{4}$", "ABC-1234"),
    ("case-insensitive via inline flag", r"(?i)^abc$", "ABC"),
    ("lookahead (password complexity)", r"^(?=.*[A-Z])(?=.*\d).{8,}$", "Passw0rdd"),
    ("backreference (repeated token)", r"^(\w+)-\1$", "ab-ab"),
    ("ASCII vs Unicode digit class", r"^\d+$", "٣٤٥"),
    ("dollar and multiline", r"^a$", "a\n"),
];

fn main() {
    for (label, pattern, input) in CASES {
        match Regex::new(pattern) {
            Ok(re) => println!(
                "{:<34} COMPILES   matches({input:?}) = {}",
                label,
                re.is_match(input)
            ),
            Err(error) => println!(
                "{:<34} REJECTED   {}",
                label,
                error.to_string().lines().next().unwrap_or("")
            ),
        }
    }
}
