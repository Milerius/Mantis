#![allow(missing_docs)]

fn main() {
    #[cfg(feature = "bench-contenders-cpp")]
    {
        cc::Build::new()
            .cpp(true)
            .std("c++17")
            .opt_level(3)
            .flag("-march=native")
            .include("cpp/include")
            .file("cpp/rigtorp_ffi.cpp")
            .compile("rigtorp_ffi");

        cc::Build::new()
            .cpp(true)
            .std("c++20")
            .opt_level(3)
            .flag("-march=native")
            .include("cpp/include")
            .file("cpp/drogalis_ffi.cpp")
            .compile("drogalis_ffi");
    }
}
