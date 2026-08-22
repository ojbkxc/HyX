#
# To learn more about a Podspec see http://guides.cocoapods.org/syntax/podspec.html.
# Run `pod lib lint rust_lib_hyx_app.podspec` to validate before publishing.
#
Pod::Spec.new do |s|
  s.name             = 'rust_lib_hyx_app'
  s.version          = '0.0.1'
  s.summary          = 'HyX Rust core FFI plugin.'
  s.description      = <<-DESC
HyX Rust core FFI plugin via flutter_rust_bridge.
                       DESC
  s.homepage         = 'https://github.com/ojbkxc/HyX'
  s.license          = { :file => '../LICENSE' }
  s.author           = { 'ojbkxc' => 'ojbkxc@users.noreply.github.com' }

  s.source           = { :path => '.' }
  s.source_files = 'Classes/**/*'
  s.dependency 'Flutter'
  s.platform = :ios, '11.0'

  s.pod_target_xcconfig = { 'DEFINES_MODULE' => 'YES', 'EXCLUDED_ARCHS[sdk=iphonesimulator*]' => 'i386' }
  s.swift_version = '5.0'

  s.script_phase = {
    :name => 'Build Rust library',
    :script => 'sh "$PODS_TARGET_SRCROOT/../cargokit/build_pod.sh" ../../rust rust_lib_hyx_app',
    :execution_position => :before_compile,
    :input_files => ['${BUILT_PRODUCTS_DIR}/cargokit_phony'],
    :output_files => ["${BUILT_PRODUCTS_DIR}/librust_lib_hyx_app.a"],
  }
  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    'EXCLUDED_ARCHS[sdk=iphonesimulator*]' => 'i386',
    'OTHER_LDFLAGS' => '-force_load ${BUILT_PRODUCTS_DIR}/librust_lib_hyx_app.a',
  }
end