import Foundation
import Combine

@MainActor
final class WalletModel: ObservableObject {
    @Published var snapshot: WalletSnapshot?
    @Published var busy = false
    @Published var error: String?
    @Published var notice: String?
    @Published var recoveryPhrase: String?

    func refresh() {
        busy = true
        error = nil
        Task {
            do {
                let value = try await Task.detached { try GridBridge.snapshot() }.value
                snapshot = value
            } catch {
                self.error = error.localizedDescription
            }
            busy = false
        }
    }

    func act(_ payload: [String: Any]) {
        busy = true
        error = nil
        notice = nil
        Task {
            do {
                let result = try await Task.detached { try GridBridge.action(payload) }.value
                snapshot = result.snapshot
                notice = result.message
                recoveryPhrase = result.recoveryPhrase
            } catch {
                self.error = error.localizedDescription
            }
            busy = false
        }
    }
}
