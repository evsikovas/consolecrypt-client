# The Flutter tooling requires that developers have a version of Visual Studio
# installed that includes CMake 3.14 or later. You should not increase this
# version, as doing so will cause the plugin to fail to compile for some
# customers of the plugin.
cmake_minimum_required(VERSION 3.14)

# Project-level configuration.
set(PROJECT_NAME "cc_bridge")
project(${PROJECT_NAME} LANGUAGES CXX)

include("../cargokit/cmake/cargokit.cmake")
# Crate path relative to windows/flutter/ephemeral/.plugin_symlinks/cc_bridge/windows
# (7 levels up = client/, then rust/bridge).
apply_cargokit(${PROJECT_NAME} ../../../../../../../rust/bridge cc_bridge "")

# List of absolute paths to libraries that should be bundled with the plugin.
set(cc_bridge_bundled_libraries
  "${${PROJECT_NAME}_cargokit_lib}"
  PARENT_SCOPE
)
