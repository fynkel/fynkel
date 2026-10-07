Pod::Spec.new do |s|
  s.name             = 'bevy_view'
  s.version          = '0.0.1'
  s.summary          = 'Spike: Bevy IOSurface ring through a Flutter Texture.'
  s.description      = 'Spike only.'
  s.homepage         = 'https://fynkel.dev'
  s.license          = { :type => 'Proprietary', :text => 'See the repository root.' }
  s.author           = { 'Fynkel' => 'noreply@fynkel.dev' }
  s.source           = { :path => '.' }
  s.source_files     = 'bevy_view/Sources/bevy_view/**/*.swift'
  s.osx.dependency 'FlutterMacOS'
  s.ios.dependency 'Flutter'
  s.osx.deployment_target = '12.0'
  s.ios.deployment_target = '15.0'
  s.pod_target_xcconfig = { 'DEFINES_MODULE' => 'YES' }
  s.swift_version = '5.0'
end
