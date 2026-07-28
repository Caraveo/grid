import Foundation

struct WalletSnapshot: Codable {
    let version: Int
    let configDir: String
    let auth: AuthSnapshot
    let grid: GridSnapshot
    let solana: SolanaSnapshot
    let bitcoin: BitcoinSnapshot
    let network: NetworkSnapshot
    let activity: [ChainTransaction]
}

struct AuthSnapshot: Codable {
    let initialized: Bool
    let mode: String
    let encrypted: Bool
    let unlocked: Bool
    let passkeyRegistered: Bool
    let detail: String
}

struct GridSnapshot: Codable {
    let initialized: Bool
    let address: String?
    let balance: Double
    let unclaimed: Double
    let totalMinted: Double
    let totalBurned: Double
    let maxSupply: Double
    let burnDeadlineDays: Int
}

struct SolanaSnapshot: Codable {
    let configured: Bool
    let address: String?
    let balance: Double?
    let network: String
    let custody: String?
    let mint: String
    let error: String?
}

struct BitcoinSnapshot: Codable {
    let network: String
    let role: String
    let route: String
    let live: Bool
}

struct NetworkSnapshot: Codable {
    let mode: String
    let truthUrl: String
    let p2pPeer: String
    let connected: Bool
    let trusted: Bool
    let chainId: String?
    let height: Int?
    let leaderPubkey: String?
    let error: String?
}

struct ChainTransaction: Codable, Identifiable {
    let id: String
    let kind: String
    let at: String
    let from: String?
    let to: String?
    let amount: Double
    let memo: String?
    let signature: String?
}

struct ActionResponse: Codable {
    let ok: Bool
    let message: String
    let recoveryPhrase: String?
    let transaction: ChainTransaction?
    let snapshot: WalletSnapshot
}

enum SidebarItem: String, CaseIterable, Identifiable {
    case overview = "Overview"
    case send = "Send"
    case receive = "Receive"
    case activity = "Activity"
    case security = "Security"

    var id: String { rawValue }
    var icon: String {
        switch self {
        case .overview: "square.grid.2x2"
        case .send: "arrow.up.right"
        case .receive: "arrow.down.left"
        case .activity: "clock.arrow.circlepath"
        case .security: "lock.shield"
        }
    }
}

enum AppTheme: String, CaseIterable, Identifiable {
    case system = "System"
    case light = "Light"
    case dark = "Dark"
    var id: String { rawValue }
}
