#![allow(missing_docs)]

fn main() {
    #[cfg(feature = "bench-contenders-cpp")]
    {
        cc::Build::new()
            .cpp(true)
            .std("c++17")
            .opt_level(3)
            .include("cpp/include")
            .file("cpp/rigtorp_ffi.cpp")
            .compile("rigtorp_ffi");
    }
}
