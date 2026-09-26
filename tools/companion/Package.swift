// swift-tools-version: 6.0
import PackageDescription
let package = Package(
    name: "ARPGCompanion",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "ARPGCompanion", targets: ["ARPGCompanion"])],
    targets: [
        .executableTarget(name: "ARPGCompanion")
    ]
)
