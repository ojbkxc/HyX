# HyX — Windows CMake build

cmake_minimum_required(VERSION 3.14)

project(hyx_app LANGUAGES CXX)

# The Flutter tooling requires that developers have a version of Visual Studio
# installed that includes CMake 3.14 or later.

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Flutter-generated glue code.
add_subdirectory(flutter)

# Application executable; sources are in the runner/ subdirectory.
add_executable(${BINARY_NAME} WIN32
  "runner/main.cpp"
  "runner/flutter_window.cpp"
  "runner/utils.cpp"
  "${FLUTTER_MANIFEST_IN}"
)
apply_standard_settings(${BINARY_NAME})
add_dependencies(${BINARY_NAME} flutter_assemble)
target_link_libraries(${BINARY_NAME} PRIVATE flutter flutter_wrapper_app)
target_include_directories(${BINARY_NAME} PRIVATE "${CMAKE_SOURCE_DIR}")