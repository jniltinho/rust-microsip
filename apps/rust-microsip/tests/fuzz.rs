//! Bounded deterministic fuzzing of the parsers that take outside input
//! (dial strings, links, CSV, `.env`, arguments): mutated seeds and random
//! bytes (decoded lossily to text, as these parsers take `&str`) must return
//! a result, never panic. Fictional data only.

use rust_microsip_config::{parse_args, parse_dotenv};
use rust_microsip_core::DialTarget;
use rust_microsip_platform::parse_dial_uri;
use rust_microsip_storage::preview_csv;

const ROUNDS: usize = 20_000;

/// Xorshift inputs: byte flips, inserts, deletions and splices of the seeds,
/// plus fully random buffers. Deterministic so a failure is reproducible.
fn inputs(seeds: &[&str]) -> impl Iterator<Item = String> {
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let seeds: Vec<Vec<u8>> = seeds.iter().map(|s| s.as_bytes().to_vec()).collect();
    (0..ROUNDS).map(move |round| {
        let mut bytes = if round % 8 == 0 {
            (0..next() % 64).map(|_| next() as u8).collect()
        } else {
            seeds[round % seeds.len()].clone()
        };
        for _ in 0..1 + next() % 4 {
            let at = (next() as usize) % (bytes.len() + 1);
            match next() % 4 {
                0 if at < bytes.len() => bytes[at] = next() as u8,
                1 => {
                    let specials = b"%:@;?&=\r\n\",\0+#";
                    bytes.insert(at, specials[next() as usize % specials.len()]);
                }
                2 if at < bytes.len() => {
                    bytes.remove(at);
                }
                _ => {
                    let other = &seeds[(next() as usize) % seeds.len()];
                    bytes.splice(at..at, other.iter().take((next() % 16) as usize).copied());
                }
            }
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

#[test]
fn dial_targets_never_panic() {
    for input in inputs(&["1002", "+55 11 4000-1234", "*97", "sip:1002@pbx"]) {
        let _ = DialTarget::parse(&input);
    }
}

#[test]
fn links_never_panic() {
    for input in inputs(&[
        "sip:1002@pbx.invalid",
        "tel:+5511400012",
        "sip:%31002@h",
        "tel:1;x=y",
    ]) {
        let _ = parse_dial_uri(&input);
    }
}

#[test]
fn csv_previews_never_panic() {
    for input in inputs(&[
        "name,number\nAna,1002\n",
        "name,number,label\n\"B, C\",+551140,work\n",
    ]) {
        let _ = preview_csv(&input);
    }
}

#[test]
fn dotenv_and_arguments_never_panic() {
    for input in inputs(&[
        "RUST_MICROSIP_SERVER=pbx.invalid\n# c\nA=\"b c\"\n",
        "--server pbx --port=5061 sip:1002@pbx",
    ]) {
        let _ = parse_dotenv(&input);
        let _ = parse_args(input.split(' ').map(Into::into));
    }
}
