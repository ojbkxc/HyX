# rust_lib_hyx_app — Windows CMake build

cmake_minimum_required(VERSION 3.14)

set(PROJECT_NAME "rust_lib_hyx_app")
project(${PROJECT_NAME} LANGUAGES CXX)

include("../cargokit/cmake/cargokit.cmake")

# Relative to app/windows/flutter/ephemeral/.plugin_symlinks/rust_lib_hyx_app/windows
apply_cargokit(${PROJECT_NAME} ../../../../../../../packages/hyx_isolates/rust rust_lib_hyx_app "")

set(rust_lib_hyx_app_bundled_libraries
  "${${PROJECT_NAME}_cargokit_lib}"
  PARENT_SCOPE
)