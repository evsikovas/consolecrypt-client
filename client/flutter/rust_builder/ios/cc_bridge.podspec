Pod::Spec.new do |s|
  s.name = 'cc_bridge'
  s.version = '0.1.0'
  s.summary = 'ConsoleCrypt Rust core and iOS secure platform channels.'
  s.description = 'Links the native Rust core and implements local authentication and private storage.'
  s.homepage = 'https://github.com/evsikovas/consolecrypt-client'
  s.license = { :type => 'AGPL-3.0-only' }
  s.author = { 'ConsoleCrypt Contributors' => 'i@evsikov.net' }
  s.source = { :path => '.' }
  s.source_files = 'Classes/**/*'
  s.dependency 'Flutter'
  s.frameworks = 'LocalAuthentication', 'Security', 'SystemConfiguration'
  # RDP graphics enables flate2's native zlib backend in the Rust static library.
  s.libraries = 'z'
  s.platform = :ios, '15.0'
  s.swift_version = '5.0'
  s.script_phase = {
    :name => 'Build Rust library',
    :script => 'bash "$PODS_TARGET_SRCROOT/../cargokit/build_pod.sh" ../../../rust/bridge cc_bridge',
    :execution_position => :before_compile,
    :input_files => ['${BUILT_PRODUCTS_DIR}/cargokit_phony'],
    :output_files => ['${BUILT_PRODUCTS_DIR}/libcc_bridge.a'],
  }
  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    'OTHER_LDFLAGS' => '-force_load ${BUILT_PRODUCTS_DIR}/libcc_bridge.a',
  }
end
