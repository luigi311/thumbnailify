use criterion::{black_box, criterion_group, criterion_main, Criterion};
use image::open;
use thumbnailify::thumbnail::generate_thumbnail;

fn bench_generate_thumbnail(c: &mut Criterion) {
    // Open a test image from the tests/images directory.
    // Make sure you have a sample image available at this path.
    let img = open("../tests/images/nasa-4019x4019.png").expect("Failed to open test image");

    c.bench_function("generate_thumbnail", |b| {
        b.iter(|| {
            // Run the generate_thumbnail function with a max dimension of 128 pixels.
            // We use black_box to prevent compiler optimizations.
            let thumb = generate_thumbnail(black_box(&img), black_box(128));
            black_box(thumb);
        })
    });
}

criterion_group!(benches, bench_generate_thumbnail);
criterion_main!(benches);
