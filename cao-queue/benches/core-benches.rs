use parking_lot::RwLock;
use rand::Rng;
use std::{collections::HashMap, sync::Arc};

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

use cao_queue::{collections::concurrent_message_map::MessageMap, MessageId};

fn concurrent_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_insert");

    for i in (0..16).step_by(2).skip(1) {
        let i = 1 << i;
        group.bench_with_input(BenchmarkId::new("MessageMap", i), &i, |b, i| {
            let map: Arc<MessageMap<String>> = Arc::new(MessageMap::with_capacity(i / 4));

            b.iter(|| {
                rayon::scope(|s| {
                    for _ in 0..16 {
                        let map = Arc::clone(&map);
                        s.spawn(move |_| {
                            for _ in 0..*i {
                                let mut rng = rand::thread_rng();
                                map.insert(MessageId(rng.gen_range(0, 16000)), "boi".to_owned());
                            }
                        });
                    }
                });
            });
        });
        group.bench_with_input(BenchmarkId::new("RwLocked HashMap", i), &i, |b, i| {
            let map: Arc<RwLock<HashMap<MessageId, String>>> =
                Arc::new(RwLock::new(HashMap::with_capacity(i / 4)));

            b.iter(|| {
                rayon::scope(|s| {
                    for _ in 0..16 {
                        let map = Arc::clone(&map);
                        s.spawn(move |_| {
                            for _ in 0..*i {
                                let mut map = map.write();
                                let mut rng = rand::thread_rng();
                                map.insert(MessageId(rng.gen_range(0, 16000)), "boi".to_owned());
                            }
                        });
                    }
                });
            });
        });
    }

    group.finish();
}

criterion_group!(message_map_benches, concurrent_insert);
criterion_main!(message_map_benches);
