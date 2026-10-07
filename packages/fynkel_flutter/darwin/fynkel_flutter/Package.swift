// swift-tools-version: 5.9
// The Swift side of package:fynkel_flutter, shared by macOS and iOS.
// Flutter builds it through Swift Package Manager; ../fynkel_flutter.podspec
// is the same for apps that use CocoaPods.

import PackageDescription

let package = Package(
    name: "fynkel_flutter",
    platforms: [
        .macOS("12.0"),
        .iOS("15.0"),
    ],
    products: [
        .library(name: "fynkel-flutter", targets: ["fynkel_flutter"])
    ],
    dependencies: [
        .package(name: "FlutterFramework", path: "../FlutterFramework")
    ],
    targets: [
        .target(
            name: "fynkel_flutter",
            dependencies: [
                .product(name: "FlutterFramework", package: "FlutterFramework")
            ]
        )
    ]
)
