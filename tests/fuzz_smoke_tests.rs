#![cfg(feature = "serde")]

//! A deterministic no-panic sweep that runs under `cargo test`, where
//! `cargo fuzz` does not.
//!
//! Nothing here asserts *what* the crate answers, only that it answers:
//! every input must produce an `Ok` or an `Err`, never a panic and never a
//! hang. The seed is fixed on purpose -- a test that fails one run in
//! fifty is a test nobody trusts.

use std::time::{Duration, Instant};

use yson_rs::{Reader, Scan, YsonFormat, YsonValue, from_slice, scan_value, to_vec};

/// The whole file must finish well inside this. A stray `/` in text input once
/// spun forever while allocating nothing, so a wall-clock bound is the only
/// thing that catches that class of defect: it cannot be caught by a limit on
/// memory, and it presents as a hung job rather than a crash.
const BUDGET: Duration = Duration::from_secs(30);

/// xorshift64*, so the corpus is identical on every platform and every run.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Documents worth damaging: one of each shape the format has.
const CORPUS: &[&[u8]] = &[
    b"#",
    b"42",
    b"-1",
    b"42u",
    b"1.5",
    b"%true",
    b"%nan",
    b"hello",
    b"\"quoted \\n string\"",
    b"[]",
    b"{}",
    b"[1;2;3]",
    b"{a=1;b=2}",
    b"<a=b>42",
    b"<a=b>#",
    b"<a=b>{x=10}",
    b"[[1;2];[3;4]]",
    b"{a={b={c=[1;2;{d=#}]}}}",
    b"<schema=<strict=%true>[a;b]>{rows=[{k=1};{k=2}]}",
    b"// comment\n[1;2]",
    b"/* comment */ {a=1}",
];

/// Every entry point, over one input. None of them may panic.
fn poke(input: &[u8], format: YsonFormat) {
    let _ = Reader::new(input, format).read_value();
    let _ = scan_value(input, format);
    let _ = from_slice::<YsonValue>(input, format);
    let _ = from_slice::<i64>(input, format);
    let _ = from_slice::<String>(input, format);
    let _ = from_slice::<Vec<i32>>(input, format);
    let _ = from_slice::<(i32, i32)>(input, format);
    let _ = from_slice::<std::collections::BTreeMap<String, i32>>(input, format);

    // Framing a damaged buffer must terminate and must never panic.
    let mut rest = input;
    for _ in 0..8 {
        match scan_value(rest, format) {
            Ok(Scan::Complete(len)) if len > 0 && len <= rest.len() => {
                let _ = from_slice::<YsonValue>(&rest[..len], format);
                rest = &rest[len..];
            }
            _ => break,
        }
        while matches!(rest.first(), Some(b) if *b == b';' || b.is_ascii_whitespace()) {
            rest = &rest[1..];
        }
    }

    // Anything that did decode has to encode again without panicking.
    if let Ok(value) = from_slice::<YsonValue>(input, format) {
        let _ = to_vec(&value, YsonFormat::Text);
        let _ = to_vec(&value, YsonFormat::Binary);
    }
}

fn both_formats(input: &[u8]) {
    poke(input, YsonFormat::Text);
    poke(input, YsonFormat::Binary);
}

#[test]
fn the_corpus_itself_survives() {
    for input in CORPUS {
        both_formats(input);
    }
}

#[test]
fn truncation_at_every_offset_survives() {
    let start = Instant::now();
    for input in CORPUS {
        for cut in 0..=input.len() {
            both_formats(&input[..cut]);
        }
    }
    assert!(start.elapsed() < BUDGET, "truncation sweep did not finish");
}

#[test]
fn single_bit_corruption_survives() {
    let start = Instant::now();
    for input in CORPUS {
        for byte in 0..input.len() {
            for bit in 0..8 {
                let mut damaged = input.to_vec();
                damaged[byte] ^= 1 << bit;
                both_formats(&damaged);
            }
        }
    }
    assert!(start.elapsed() < BUDGET, "bit-flip sweep did not finish");
}

#[test]
fn random_bytes_survive() {
    let start = Instant::now();
    let mut rng = Rng::new(0x5359_534F_4E52_5300);

    for _ in 0..2_000 {
        let len = rng.below(48);
        let mut input = Vec::with_capacity(len);
        for _ in 0..len {
            input.push(rng.next() as u8);
        }
        both_formats(&input);
    }

    assert!(start.elapsed() < BUDGET, "random sweep did not finish");
}

#[test]
fn random_structural_bytes_survive() {
    // Uniform random bytes almost never open a bracket. Drawing from the
    // structural alphabet gets the parser into the states that actually nest.
    let start = Instant::now();
    let mut rng = Rng::new(0x5953_4F4E_2D52_5300);
    let alphabet = b"<>[]{};=#%01abu \"\\/*\n\x01\x02\x03\x06\xff";

    for _ in 0..4_000 {
        let len = rng.below(64);
        let mut input = Vec::with_capacity(len);
        for _ in 0..len {
            input.push(alphabet[rng.below(alphabet.len())]);
        }
        both_formats(&input);
    }

    assert!(start.elapsed() < BUDGET, "structural sweep did not finish");
}

#[test]
fn splices_of_two_documents_survive() {
    let start = Instant::now();
    let mut rng = Rng::new(0x5350_4C49_4345_0000);

    for _ in 0..2_000 {
        let a = CORPUS[rng.below(CORPUS.len())];
        let b = CORPUS[rng.below(CORPUS.len())];
        let split_a = rng.below(a.len() + 1);
        let split_b = rng.below(b.len() + 1);

        let mut spliced = a[..split_a].to_vec();
        spliced.extend_from_slice(&b[split_b..]);
        both_formats(&spliced);
    }

    assert!(start.elapsed() < BUDGET, "splice sweep did not finish");
}

#[test]
fn deeply_nested_input_is_refused_rather_than_overflowing_the_stack() {
    for opener in [&b"["[..], b"{", b"<"] {
        for depth in [64, 200, 5_000] {
            let mut input = opener.repeat(depth);
            both_formats(&input);

            // And the balanced version, which recurses on the way back out.
            let closer: &[u8] = match opener {
                b"[" => b"]",
                b"{" => b"}",
                _ => b">",
            };
            input.extend_from_slice(&closer.repeat(depth));
            both_formats(&input);
        }
    }
}

#[test]
fn scanning_never_reports_a_length_past_the_buffer() {
    let mut rng = Rng::new(0x5343_414E_0000_0000);

    for _ in 0..2_000 {
        let len = rng.below(40);
        let mut input = Vec::with_capacity(len);
        for _ in 0..len {
            input.push(rng.next() as u8);
        }

        for format in [YsonFormat::Text, YsonFormat::Binary] {
            if let Ok(Scan::Complete(n)) = scan_value(&input, format) {
                assert!(
                    n <= input.len(),
                    "scan reported {n} for {} bytes",
                    input.len()
                );
                // A complete prefix has to be readable on its own.
                assert!(
                    Reader::new(&input[..n], format).read_value().is_ok(),
                    "scan framed a prefix the reader rejects: {:?}",
                    &input[..n]
                );
            }
        }
    }
}
