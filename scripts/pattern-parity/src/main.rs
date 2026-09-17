use std::io::{BufRead, Write};
// Reads one JSON array per line, `[pattern, flags, value]`, and prints `1`,
// `0` or `E` (the pattern does not compile) per line.
//
// The builder is `rad::domain::validation::compile_pattern`'s, copied rather
// than imported so this probe builds without the backend's database. **If
// that function changes, change this one.**
fn main() {
    let stdin = std::io::stdin();
    let mut out = std::io::BufWriter::new(std::io::stdout());
    for line in stdin.lock().lines() {
        let v: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        let (p, f, s) = (v[0].as_str().unwrap(), v[1].as_str().unwrap(), v[2].as_str().unwrap());
        let r = regex::RegexBuilder::new(p)
            .case_insensitive(f.contains('i'))
            .multi_line(f.contains('m'))
            .dot_matches_new_line(f.contains('s'))
            .build();
        let c = match r { Err(_) => "E", Ok(re) => if re.is_match(s) { "1" } else { "0" } };
        writeln!(out, "{c}").unwrap();
    }
}
