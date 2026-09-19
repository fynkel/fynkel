# CocoaPods description of the Swift side of package:fynkel_flutter (apps
# built without Swift Package Manager). Same sources as Package.swift.
Pod::Spec.new do |s|
  s.name             = 'fynkel_flutter'
  s.version          = '0.0.1'
  s.summary          = 'Fynkel Engine viewport plugin for macOS and iOS.'
  s.description      = <<-DESC
Presents the Fynkel runtime's IOSurface ring (platform/apple) through Flutter's Texture API.
                       DESC
  s.homepage         = 'https://fynkel.dev'
  s.license          = { :type => 'Proprietary', :text => 'See the repository root.' }
  s.author           = { 'Fynkel' => 'noreply@fynkel.dev' }
  s.source           = { :path => '.' }
  s.source_files     = 'fynkel_flutter/Sources/fynkel_flutter/**/*.swift'
  s.osx.dependency 'FlutterMacOS'
  s.ios.dependency 'Flutter'
  s.osx.deployment_target = '12.0'
  s.ios.deployment_target = '15.0'
  s.pod_target_xcconfig = { 'DEFINES_MODULE' => 'YES', 'EXCLUDED_ARCHS[sdk=iphonesimulator*]' => 'i386' }
  s.swift_version = '5.0'
end
