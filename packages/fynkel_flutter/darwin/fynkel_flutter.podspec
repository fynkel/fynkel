# The Swift side of package:fynkel_flutter for apps that use CocoaPods.
# The same sources as fynkel_flutter/Package.swift.
Pod::Spec.new do |s|
  s.name             = 'fynkel_flutter'
  s.version          = '0.1.0'
  s.summary          = 'Shows Fynkel frames in Flutter on macOS and iOS.'
  s.homepage         = 'https://github.com/fynkel/fynkel'
  s.license          = { :type => 'MIT', :file => '../LICENSE' }
  s.author           = { 'Fynkel' => 'noreply@fynkel.dev' }
  s.source           = { :path => '.' }
  s.source_files     = 'fynkel_flutter/Sources/fynkel_flutter/**/*.swift'
  s.osx.dependency 'FlutterMacOS'
  s.ios.dependency 'Flutter'
  s.osx.deployment_target = '12.0'
  s.ios.deployment_target = '15.0'
  s.pod_target_xcconfig = { 'DEFINES_MODULE' => 'YES' }
  s.swift_version = '5.9'
end
