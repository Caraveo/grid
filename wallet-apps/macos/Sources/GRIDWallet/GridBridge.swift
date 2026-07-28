import Foundation

enum BridgeError: LocalizedError {
    case gridNotFound
    case failed(String)
    case invalidResponse

    var errorDescription: String? {
        switch self {
        case .gridNotFound:
            "GRID CLI not found. Install GRID or place the grid binary in the app Resources folder."
        case .failed(let message):
            message
        case .invalidResponse:
            "GRID returned an invalid wallet response."
        }
    }
}

struct GridBridge {
    static func snapshot() throws -> WalletSnapshot {
        let data = try run(arguments: ["gui", "snapshot"], input: nil)
        return try JSONDecoder().decode(WalletSnapshot.self, from: lastJSONLine(data))
    }

    static func action(_ payload: [String: Any]) throws -> ActionResponse {
        let input = try JSONSerialization.data(withJSONObject: payload)
        let data = try run(arguments: ["gui", "action"], input: input)
        return try JSONDecoder().decode(ActionResponse.self, from: lastJSONLine(data))
    }

    private static func run(arguments: [String], input: Data?) throws -> Data {
        let process = Process()
        let output = Pipe()
        let errors = Pipe()
        let stdin = Pipe()
        let executable = try resolveGrid()
        process.executableURL = executable
        process.arguments = arguments
        process.standardOutput = output
        process.standardError = errors
        if input != nil { process.standardInput = stdin }
        try process.run()
        if let input {
            stdin.fileHandleForWriting.write(input)
            stdin.fileHandleForWriting.closeFile()
        }
        let data = output.fileHandleForReading.readDataToEndOfFile()
        let errorData = errors.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        guard process.terminationStatus == 0 else {
            let message = String(data: errorData.isEmpty ? data : errorData, encoding: .utf8)?
                .trimmingCharacters(in: .whitespacesAndNewlines)
            throw BridgeError.failed(message?.isEmpty == false ? message! : "GRID wallet action failed.")
        }
        return data
    }

    private static func lastJSONLine(_ data: Data) throws -> Data {
        guard let text = String(data: data, encoding: .utf8),
              let line = text.split(separator: "\n").last(where: { $0.first == "{" })
        else { throw BridgeError.invalidResponse }
        return Data(line.utf8)
    }

    private static func resolveGrid() throws -> URL {
        let fm = FileManager.default
        var candidates: [String] = []
        if let configured = ProcessInfo.processInfo.environment["GRID_BIN"] {
            candidates.append(configured)
        }
        if let resource = Bundle.main.resourceURL?.appendingPathComponent("grid").path {
            candidates.append(resource)
        }
        candidates += [
            "\(NSHomeDirectory())/.local/bin/grid",
            "\(NSHomeDirectory())/bin/grid",
            "/opt/homebrew/bin/grid",
            "/usr/local/bin/grid",
        ]
        if let path = ProcessInfo.processInfo.environment["PATH"] {
            candidates += path.split(separator: ":").map { "\($0)/grid" }
        }
        guard let path = candidates.first(where: { fm.isExecutableFile(atPath: $0) }) else {
            throw BridgeError.gridNotFound
        }
        return URL(fileURLWithPath: path)
    }
}
