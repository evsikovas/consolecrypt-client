cmake_minimum_required(VERSION 3.13)
project(cc_bridge LANGUAGES CXX)

include("../cargokit/cmake/cargokit.cmake")
# The Linux filesystem resolves the plugin symlink before traversing '..':
# client/flutter/rust_builder/linux -> client/rust/bridge (three levels up).
apply_cargokit(cc_bridge ../../../rust/bridge cc_bridge "")

# This is an FFI-only plugin: Flutter packages the shared object beside its
# engine, and flutter_rust_bridge loads libcc_bridge.so through the bundle RPATH.
set(cc_bridge_bundled_libraries "${cc_bridge_cargokit_lib}" PARENT_SCOPE)
