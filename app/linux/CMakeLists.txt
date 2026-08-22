# HyX — Linux CMake build

cmake_minimum_required(VERSION 3.10)

project(hyx_app LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Flutter-generated glue code.
add_subdirectory(flutter)

# Application executable; sources are in the root of this directory.
add_executable(${BINARY_NAME}
  "main.cc"
  "my_application.cc"
  "${FLUTTER_MANIFEST_IN}"
)
apply_standard_settings(${BINARY_NAME})
add_dependencies(${BINARY_NAME} flutter_assemble)
target_link_libraries(${BINARY_NAME} PRIVATE flutter flutter_wrapper_app)
target_include_directories(${BINARY_NAME} PRIVATE "${CMAKE_SOURCE_DIR}")