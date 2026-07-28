// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "GRIDWallet",
    platforms: [.macOS(.v13)],
    products: [
        .executable(name: "GRIDWallet", targets: ["GRIDWallet"])
    ],
    targets: [
        .executableTarget(
            name: "GRIDWallet",
            path: "Sources/GRIDWallet"
        )
    ]
)
