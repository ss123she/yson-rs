use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use serde::{Deserialize, Serialize};
use std::hint::black_box;
use yson_rs::{Deserializer, Reader, Serializer, YsonFormat, scan_value};

use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone)]
struct BenchData<'a> {
    id: u64,
    #[serde(borrow)]
    name: &'a str,
    #[serde(borrow)]
    tags: Vec<&'a str>,
    #[serde(borrow)]
    properties: HashMap<&'a str, f64>,
}

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn generate_data() -> Vec<BenchData<'static>> {
    (0..10_000)
        .map(|i| {
            let mut props = HashMap::new();
            props.insert("x", 10.5);
            props.insert("y", 20.1);
            props.insert("velocity", 99.9);

            BenchData {
                id: i,
                name: leak_str(format!("Item-{i}")),
                tags: vec!["fast", "rust", "serde"],
                properties: props,
            }
        })
        .collect()
}

fn criterion_benchmark(c: &mut Criterion) {
    let data = generate_data();

    let mut ser_bin = Serializer::new(YsonFormat::Binary);
    data.serialize(&mut ser_bin).unwrap();
    let bin_bytes = ser_bin.output;

    let mut ser_text = Serializer::new(YsonFormat::Text);
    data.serialize(&mut ser_text).unwrap();
    let text_bytes = ser_text.output;

    let mut group = c.benchmark_group("YSON Throughput");

    // Bench: Serialize Binary
    group.throughput(Throughput::Bytes(bin_bytes.len() as u64));
    group.bench_function("Serialize Binary", |b| {
        b.iter(|| {
            let mut ser = Serializer::new(YsonFormat::Binary);
            black_box(&data).serialize(&mut ser).unwrap();
        });
    });

    // Bench: Deserialize Binary
    group.bench_function("Deserialize Binary", |b| {
        b.iter(|| {
            let mut de = Deserializer::new(black_box(&bin_bytes), YsonFormat::Binary);
            let _val: Vec<BenchData> = Vec::deserialize(&mut de).unwrap();
        });
    });

    // Bench: Serialize Text
    group.throughput(Throughput::Bytes(text_bytes.len() as u64));
    group.bench_function("Serialize Text", |b| {
        b.iter(|| {
            let mut ser = Serializer::new(YsonFormat::Text);
            black_box(&data).serialize(&mut ser).unwrap();
        });
    });

    // Bench: Deserialize Text
    group.bench_function("Deserialize Text", |b| {
        b.iter(|| {
            let mut de = Deserializer::new(black_box(&text_bytes), YsonFormat::Text);
            let _val: Vec<BenchData> = Vec::deserialize(&mut de).unwrap();
        });
    });

    group.finish();

    dom_benchmark(c, &bin_bytes, &text_bytes);
}

/// The untyped path, where borrowing is worth the most.
///
/// `read_value` against `read_value().into_owned()` is the whole borrowed-DOM
/// change expressed as two numbers: the same parse, with and without copying
/// every string out of the buffer it is already in.
fn dom_benchmark(c: &mut Criterion, bin_bytes: &[u8], text_bytes: &[u8]) {
    let mut group = c.benchmark_group("YSON DOM");

    group.throughput(Throughput::Bytes(bin_bytes.len() as u64));
    group.bench_function("Read borrowed (binary)", |b| {
        b.iter(|| {
            Reader::new(black_box(bin_bytes), YsonFormat::Binary)
                .read_value()
                .unwrap()
        });
    });
    group.bench_function("Read owned (binary)", |b| {
        b.iter(|| {
            Reader::new(black_box(bin_bytes), YsonFormat::Binary)
                .read_value()
                .unwrap()
                .into_owned()
        });
    });
    group.bench_function("Scan (binary)", |b| {
        b.iter(|| scan_value(black_box(bin_bytes), YsonFormat::Binary).unwrap());
    });

    group.throughput(Throughput::Bytes(text_bytes.len() as u64));
    group.bench_function("Read borrowed (text)", |b| {
        b.iter(|| {
            Reader::new(black_box(text_bytes), YsonFormat::Text)
                .read_value()
                .unwrap()
        });
    });
    group.bench_function("Scan (text)", |b| {
        b.iter(|| scan_value(black_box(text_bytes), YsonFormat::Text).unwrap());
    });

    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
