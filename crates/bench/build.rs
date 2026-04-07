#![allow(missing_docs)]

fn main() {
    #[cfg(feature = "bench-seqlock-contenders-cpp")]
    {
        cc::Build::new()
            .cpp(true)
            .flag("-std=c++17")
            .flag("-O3")
            .file("cpp/seqlock_bench_contender.cpp")
            .include("cpp")
            .compile("seqlock_contender");
    }
}
