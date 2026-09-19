// swift-tools-version: 5.9
// The Swift side of package:fynkel_flutter, shared by macOS and iOS
// (ADR-0009: one interop for both Apple targets). Flutter's tooling builds
// it through the generated plugin package when the app uses Swift Package
// Manager; darwin/fynkel_flutter.podspec covers CocoaPods apps.

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
