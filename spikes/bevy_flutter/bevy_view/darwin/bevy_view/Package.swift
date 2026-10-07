// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "bevy_view",
    platforms: [
        .macOS("12.0"),
        .iOS("15.0"),
    ],
    products: [
        .library(name: "bevy-view", targets: ["bevy_view"])
    ],
    dependencies: [
        .package(name: "FlutterFramework", path: "../FlutterFramework")
    ],
    targets: [
        .target(
            name: "bevy_view",
            dependencies: [
                .product(name: "FlutterFramework", package: "FlutterFramework")
            ]
        )
    ]
)
