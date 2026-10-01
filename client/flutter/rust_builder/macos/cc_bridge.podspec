#
# Builds client/rust/bridge with cargokit before compiling the pod and
# force-loads the static library into the app (FRB loads it from the
# process). Architectures follow Xcode's ARCHS (Debug: active arch only,
# Release: arm64 + x86_64 → `rustup target add x86_64-apple-darwin`).
#
Pod::Spec.new do |s|
  s.name             = 'cc_bridge'
  s.version          = '0.1.0'
  s.summary          = 'ConsoleCrypt Rust core (flutter_rust_bridge, cargokit).'
  s.description      = 'Builds and links the ConsoleCrypt Rust core (client/rust/bridge).'
  s.homepage         = 'https://github.com/consolecrypt/consolecrypt'
  s.license          = { :type => 'AGPL-3.0-only' }
  s.author           = { 'The ConsoleCrypt Contributors' => 'noreply@consolecrypt.io' }

  s.source           = { :path => '.' }
  s.source_files     = 'Classes/**/*'
  s.dependency 'FlutterMacOS'
  # Touch ID prompt of the `consolecrypt/local_auth` channel (CcBridgePlugin).
  s.frameworks = 'LocalAuthentication'

  s.platform = :osx, '10.15'
  s.swift_version = '5.0'

  s.script_phase = {
    :name => 'Build Rust library',
    # First argument: path of the crate relative to this directory
    # (rust_builder/macos → client/rust/bridge); second: library name.
    :script => 'sh "$PODS_TARGET_SRCROOT/../cargokit/build_pod.sh" ../../../rust/bridge cc_bridge',
    :execution_position => :before_compile,
    :input_files => ['${BUILT_PRODUCTS_DIR}/cargokit_phony'],
    # Let Xcode know that the static library referenced in -force_load below
    # is created by this build step.
    :output_files => ["${BUILT_PRODUCTS_DIR}/libcc_bridge.a"],
  }
  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    'OTHER_LDFLAGS' => '-force_load ${BUILT_PRODUCTS_DIR}/libcc_bridge.a',
  }
end
