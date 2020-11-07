use std::pin::Pin;

use cao_queue::{
    collections::bucket_allocator::BucketAllocator, message::ManagedMessage, MessageId,
};
use criterion::{criterion_group, BenchmarkId, Criterion};

fn alloc_system(size: usize) -> Box<[u8]> {
    vec![0; size].into_boxed_slice()
}

fn alloc_bucket(allocator: &BucketAllocator, size: usize) -> ManagedMessage {
    ManagedMessage::new(MessageId(9), size, allocator)
}

thread_local! {
    static BUCKET: Pin<Box<BucketAllocator>> = BucketAllocator::new(4096);

}

fn bench_de_allocs(c: &mut Criterion) {
    let mut group = c.benchmark_group("byte array de/allocation");
    for size in (8..24).step_by(2) {
        let size = 1usize << size;
        group.bench_with_input(BenchmarkId::new("system allocator", size), &size, |b, i| {
            b.iter(|| alloc_system(*i))
        });

        let bucket = BucketAllocator::new(size);
        group.bench_with_input(BenchmarkId::new("local bucket", size), &size, |b, i| {
            b.iter(|| alloc_bucket(&*bucket, *i))
        });

        group.bench_with_input(BenchmarkId::new("global bucket", size), &size, |b, i| {
            b.iter(|| {
                let res: ManagedMessage<'static> = BUCKET.with(|bucket| {
                    let bucket: &'static BucketAllocator =
                        unsafe { std::mem::transmute(&**bucket) };
                    alloc_bucket(bucket, *i)
                });
                res
            })
        });
    }
    group.finish();
}

criterion_group!(benches, bench_de_allocs);
