// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "membrane-tray-macos",
    platforms: [.macOS(.v13)],
    products: [
        .executable(name: "membrane-tray-macos", targets: ["MembraneTrayMacOS"])
    ],
    targets: [
        .executableTarget(
            name: "MembraneTrayMacOS",
            path: "Sources/MembraneTrayMacOS",
            exclude: ["Info.plist"]
        ),
        .testTarget(
            name: "MembraneTrayMacOSTests",
            dependencies: ["MembraneTrayMacOS"],
            path: "Tests/MembraneTrayMacOSTests"
        )
    ]
)
