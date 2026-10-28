//! One segment, a few times: how long one model run takes (`seg_bench [runs]`, default 3).
//!
//! Uses the engines the app uses (`OPENFADER_STEMS_THREADS` for the CPU thread count) and the model from the cache. Meant for
//! comparing settings on one machine: `OPENFADER_STEMS_OPT` and `OPENFADER_STEMS_THREADS`.

use openfader::stems::model::{self, Engine, SEGMENT};
use std::time::Instant;

fn main() {
    model::init_ort();
    let runs: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);
    let threads = std::env::var("OPENFADER_STEMS_THREADS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| openfader::stems::threads(true));
    let engine = Engine::Cpu(threads);
    let mix: Vec<f32> = (0..2 * SEGMENT)
        .map(|i| ((i as f32 * 0.01).sin() + (i as f32 * 0.37).sin()) * 0.1)
        .collect();
    for i in 0..runs {
        let t = Instant::now();
        let out = engine.run(mix.clone()).expect("run");
        println!(
            "run {i}: {:.2} s, {} samples out",
            t.elapsed().as_secs_f32(),
            out.len()
        );
    }
    model::shutdown();
}
