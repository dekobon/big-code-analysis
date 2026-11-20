//! Criterion measurements over the same registry as the source scaling gate.

use big_code_analysis_bench::documentation::{Parsed, Prepared, REGISTRY};
use criterion::{BenchmarkId, Criterion};

fn main() {
    let mut criterion = Criterion::default().configure_from_args();
    for probe in REGISTRY {
        let Some((group_name, name)) = probe.id.rsplit_once('/') else {
            eprintln!("invalid documentation benchmark ID: {}", probe.id);
            std::process::exit(2);
        };
        let mut group = criterion.benchmark_group(group_name);
        for size in probe.sizes {
            let result = (|| {
                let parsed = Parsed::new(probe, size)?;
                let index = parsed.collect(probe)?;
                let prepared = Prepared::new(probe, size, &parsed, &index)?;
                group.bench_function(BenchmarkId::new(name, size), |bencher| {
                    bencher.iter(|| {
                        if let Err(error) = prepared.execute() {
                            eprintln!("{} size {size}: {error}", probe.id);
                            std::process::exit(2);
                        }
                    });
                });
                Ok::<_, big_code_analysis_bench::documentation::HarnessError>(())
            })();
            if let Err(error) = result {
                eprintln!("{} size {size}: {error}", probe.id);
                std::process::exit(2);
            }
        }
        group.finish();
    }
    criterion.final_summary();
}
