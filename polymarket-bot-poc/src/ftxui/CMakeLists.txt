cmake_minimum_required(VERSION 3.14)
project(ftxui_dashboard CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

include(FetchContent)
FetchContent_Declare(ftxui
  GIT_REPOSITORY https://github.com/ArthurSonzogni/FTXUI.git
  GIT_TAG v5.0.0
  GIT_SHALLOW TRUE
)
set(FTXUI_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
set(FTXUI_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(FTXUI_BUILD_DOCS OFF CACHE BOOL "" FORCE)
FetchContent_MakeAvailable(ftxui)

add_library(ftxui_dashboard STATIC ftxui_dashboard.cpp)
target_link_libraries(ftxui_dashboard PUBLIC
  ftxui::screen ftxui::dom ftxui::component)
target_include_directories(ftxui_dashboard PUBLIC ${CMAKE_CURRENT_SOURCE_DIR})
