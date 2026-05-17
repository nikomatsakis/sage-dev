use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use rand::rngs::SmallRng;
use rand::{Rng, RngExt, SeedableRng};
use sage_stash::*;
use std::hint::black_box;

// ---------------------------------------------------------------------------
// AST-like test types
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
enum Ty {
    Int(u8),
    Ref(Ptr<Ty>),
    Pair(Ptr<Ty>, Ptr<Ty>),
    Triple(Ptr<Ty>, Ptr<Ty>, Ptr<Ty>),
}

// ---------------------------------------------------------------------------
// Tree builder
// ---------------------------------------------------------------------------

struct TreeBuilder {
    rng: SmallRng,
    stash: Stash,
    pool: Vec<Ptr<Ty>>,
    leaf_variants: u8,
}

impl TreeBuilder {
    fn new(seed: u64, leaf_variants: u8) -> Self {
        Self {
            rng: SmallRng::seed_from_u64(seed),
            stash: Stash::new(),
            pool: Vec::new(),
            leaf_variants,
        }
    }

    fn leaf(&mut self) -> Ptr<Ty> {
        let ty = Ty::Int(self.rng.random_range(0..self.leaf_variants));
        let ptr = self.stash.alloc(ty);
        self.pool.push(ptr);
        ptr
    }

    fn pick_existing(&mut self) -> Ptr<Ty> {
        let idx = self.rng.random_range(0..self.pool.len());
        self.pool[idx]
    }

    fn child(&mut self, depth: u32, breadth: u32, overlap: f64) -> Ptr<Ty> {
        if !self.pool.is_empty() && self.rng.random_bool(overlap) {
            self.pick_existing()
        } else {
            self.build_node(depth - 1, breadth, overlap)
        }
    }

    fn build_node(&mut self, depth: u32, breadth: u32, overlap: f64) -> Ptr<Ty> {
        if depth == 0 {
            return self.leaf();
        }

        let ty = match breadth.min(3) {
            1 => {
                let c = self.child(depth, breadth, overlap);
                Ty::Ref(c)
            }
            2 => {
                let a = self.child(depth, breadth, overlap);
                let b = self.child(depth, breadth, overlap);
                Ty::Pair(a, b)
            }
            _ => {
                let a = self.child(depth, breadth, overlap);
                let b = self.child(depth, breadth, overlap);
                let c = self.child(depth, breadth, overlap);
                Ty::Triple(a, b, c)
            }
        };

        let ptr = self.stash.alloc(ty);
        self.pool.push(ptr);
        ptr
    }

    fn finish(self) -> (Stash, Vec<Ptr<Ty>>) {
        (self.stash, self.pool)
    }
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn bench_alloc_distinct(c: &mut Criterion) {
    let mut group = c.benchmark_group("alloc_distinct");
    for count in [100, 1_000, 10_000] {
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            b.iter(|| {
                let mut stash = Stash::new();
                for i in 0..count {
                    black_box(stash.alloc(Ty::Int((i % 256) as u8)));
                }
                black_box(&stash);
            });
        });
    }
    group.finish();
}

fn bench_alloc_high_dedup(c: &mut Criterion) {
    let mut group = c.benchmark_group("alloc_high_dedup");
    for count in [100, 1_000, 10_000] {
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            b.iter(|| {
                let mut stash = Stash::new();
                for i in 0..count {
                    black_box(stash.alloc(Ty::Int((i % 4) as u8)));
                }
                black_box(&stash);
            });
        });
    }
    group.finish();
}

fn bench_tree_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("tree_build");
    for (depth, breadth, overlap) in [(3, 3, 0.0), (5, 2, 0.0), (3, 3, 0.3), (5, 2, 0.5)] {
        group.bench_with_input(
            BenchmarkId::new(format!("d{depth}_b{breadth}"), format!("overlap={overlap}")),
            &(depth, breadth, overlap),
            |b, &(depth, breadth, overlap)| {
                b.iter(|| {
                    let mut builder = TreeBuilder::new(42, 8);
                    let root = builder.build_node(depth, breadth, overlap);
                    let (stash, _) = builder.finish();
                    black_box((&stash, root));
                });
            },
        );
    }
    group.finish();
}

fn bench_stashed_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("stashed_construction");
    for (depth, breadth) in [(3, 3), (5, 2), (4, 4)] {
        group.bench_with_input(
            BenchmarkId::new(format!("d{depth}_b{breadth}"), "fingerprint"),
            &(depth, breadth),
            |b, &(depth, breadth)| {
                b.iter(|| {
                    let mut builder = TreeBuilder::new(42, 8);
                    let root = builder.build_node(depth, breadth, 0.2);
                    let (stash, _) = builder.finish();
                    let s = Stashed::new(stash, root);
                    black_box(&s);
                });
            },
        );
    }
    group.finish();
}

fn bench_stashed_equality(c: &mut Criterion) {
    let mut group = c.benchmark_group("stashed_eq");

    for (label, depth, breadth, same) in [
        ("equal_small", 3, 2, true),
        ("equal_large", 5, 3, true),
        ("unequal_small", 3, 2, false),
        ("unequal_large", 5, 3, false),
    ] {
        let mut b1 = TreeBuilder::new(42, 8);
        let r1 = b1.build_node(depth, breadth, 0.2);
        let (s1, _) = b1.finish();
        let stashed1 = Stashed::new(s1, r1);

        let seed2 = if same { 42 } else { 99 };
        let mut b2 = TreeBuilder::new(seed2, 8);
        let r2 = b2.build_node(depth, breadth, 0.2);
        let (s2, _) = b2.finish();
        let stashed2 = Stashed::new(s2, r2);

        group.bench_function(label, |b| {
            b.iter(|| black_box(black_box(&stashed1) == black_box(&stashed2)));
        });
    }
    group.finish();
}

fn bench_read_back(c: &mut Criterion) {
    let mut group = c.benchmark_group("read_back");
    for count in [100, 1_000, 10_000] {
        let mut stash = Stash::new();
        let ptrs: Vec<_> = (0..count)
            .map(|i| stash.alloc(Ty::Int((i % 256) as u8)))
            .collect();

        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &(stash, ptrs),
            |b, (stash, ptrs)| {
                b.iter(|| {
                    for &p in ptrs {
                        black_box(&stash[p]);
                    }
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_alloc_distinct,
    bench_alloc_high_dedup,
    bench_tree_build,
    bench_stashed_construction,
    bench_stashed_equality,
    bench_read_back,
);
criterion_main!(benches);
