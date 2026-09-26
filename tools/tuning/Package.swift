// swift-tools-version: 6.0
import PackageDescription
let package = Package(
    name: "TuningCompanion",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "TuningCompanion", targets: ["TuningCompanion"])],
    targets: [
        .executableTarget(name: "TuningCompanion")
    ]
)
