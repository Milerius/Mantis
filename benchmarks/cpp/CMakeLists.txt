cmake_minimum_required(VERSION 3.16)
project(spsc-bench CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

set(CMAKE_CXX_FLAGS_RELEASE "-O2 -march=native -flto -DNDEBUG")
set(CMAKE_INTERPROCEDURAL_OPTIMIZATION_RELEASE TRUE)

add_executable(spsc-bench src/main.cpp)

target_include_directories(spsc-bench PRIVATE
    src
    vendor/SPSCQueue/include
    vendor/SPSC-Queue/include
)

target_link_libraries(spsc-bench PRIVATE pthread)
