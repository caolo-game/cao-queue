mod bucket_allocator_benches;

use criterion::criterion_main;

criterion_main!(
    bucket_allocator_benches::benches
);
