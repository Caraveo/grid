import SwiftUI
import AppKit
import CoreImage
import CoreImage.CIFilterBuiltins

@main
struct GRIDWalletApp: App {
    @StateObject private var model = WalletModel()
    @AppStorage("grid.wallet.theme") private var theme = AppTheme.system.rawValue

    private var scheme: ColorScheme? {
        switch AppTheme(rawValue: theme) ?? .system {
        case .system: nil
        case .light: .light
        case .dark: .dark
        }
    }

    var body: some Scene {
        WindowGroup {
            WalletRootView(model: model, theme: $theme)
                .preferredColorScheme(scheme)
                .frame(minWidth: 980, minHeight: 680)
                .onAppear { model.refresh() }
        }
        .windowStyle(.hiddenTitleBar)
        .commands {
            CommandGroup(after: .sidebar) {
                Button("Refresh Wallet") { model.refresh() }
                    .keyboardShortcut("r", modifiers: .command)
            }
        }
    }
}

struct WalletRootView: View {
    @ObservedObject var model: WalletModel
    @Binding var theme: String
    @State private var selection: SidebarItem? = .overview

    var body: some View {
        NavigationSplitView {
            VStack(spacing: 0) {
                Brand()
                List(SidebarItem.allCases, selection: $selection) { item in
                    Label(item.rawValue, systemImage: item.icon)
                        .tag(item)
                }
                .listStyle(.sidebar)
                HStack {
                    Circle()
                        .fill(model.snapshot?.auth.unlocked == true ? Color.green : Color.orange)
                        .frame(width: 8, height: 8)
                    Text(model.snapshot?.auth.unlocked == true ? "Vault unlocked" : "Vault locked")
                        .font(.caption)
                    Spacer()
                }
                .padding()
            }
            .navigationSplitViewColumnWidth(min: 210, ideal: 230)
        } detail: {
            ZStack {
                LinearGradient(
                    colors: [Color.accentColor.opacity(0.08), .clear],
                    startPoint: .topLeading,
                    endPoint: .center
                )
                .ignoresSafeArea()
                Group {
                    switch selection ?? .overview {
                    case .overview: OverviewView(model: model)
                    case .send: SendView(model: model)
                    case .receive: ReceiveView(model: model)
                    case .activity: ActivityView(model: model)
                    case .security: SecurityView(model: model, theme: $theme)
                    }
                }
                .padding(32)
            }
            .toolbar {
                ToolbarItem {
                    if model.busy { ProgressView().controlSize(.small) }
                }
                ToolbarItem {
                    Button(action: model.refresh) {
                        Image(systemName: "arrow.clockwise")
                    }
                }
            }
        }
        .alert("Phoenix — GRID Wallet", isPresented: Binding(
            get: { model.error != nil },
            set: { if !$0 { model.error = nil } }
        )) {
            Button("OK") { model.error = nil }
        } message: {
            Text(model.error ?? "")
        }
        .sheet(item: Binding(
            get: { model.recoveryPhrase.map(RecoveryPhrase.init) },
            set: { if $0 == nil { model.recoveryPhrase = nil } }
        )) { phrase in
            RecoveryPhraseView(phrase: phrase.value) {
                model.recoveryPhrase = nil
            }
        }
    }
}

struct RecoveryPhrase: Identifiable {
    let id = UUID()
    let value: String
    init(_ value: String) { self.value = value }
}

struct Brand: View {
    var body: some View {
        HStack(spacing: 12) {
            ZStack {
                RoundedRectangle(cornerRadius: 10).fill(.primary)
                Image(systemName: "square.grid.3x3.fill")
                    .foregroundStyle(.background)
            }
            .frame(width: 38, height: 38)
            VStack(alignment: .leading, spacing: 1) {
                Text("PHOENIX").font(.headline).tracking(3)
                Text("GRID WALLET").font(.caption2).foregroundStyle(.secondary).tracking(2)
            }
            Spacer()
        }
        .padding(18)
    }
}

struct PageTitle: View {
    let eyebrow: String
    let title: String
    let subtitle: String
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(eyebrow.uppercased())
                .font(.caption2.weight(.semibold))
                .foregroundStyle(.secondary)
                .tracking(2)
            Text(title).font(.system(size: 34, weight: .light))
            Text(subtitle).foregroundStyle(.secondary)
        }
    }
}

struct MetricCard: View {
    let label: String
    let value: String
    let detail: String
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(label.uppercased()).font(.caption2.weight(.semibold)).foregroundStyle(.secondary)
            Text(value).font(.system(size: 26, weight: .medium, design: .rounded))
            Text(detail).font(.caption).foregroundStyle(.secondary).lineLimit(2)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(20)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 18))
    }
}

struct OverviewView: View {
    @ObservedObject var model: WalletModel
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                PageTitle(
                    eyebrow: "GRID → SOL → BTC",
                    title: "One wallet. Three layers.",
                    subtitle: "GRID utility, Solana settlement, Bitcoin transaction security."
                )
                if let value = model.snapshot {
                    HStack(spacing: 16) {
                        MetricCard(
                            label: "GRID chain",
                            value: value.grid.balance.formatted(.number.precision(.fractionLength(2))),
                            detail: value.grid.address ?? "Initialize your GRID wallet"
                        )
                        MetricCard(
                            label: "Mining rewards",
                            value: "\(value.grid.unclaimed.formatted(.number.precision(.fractionLength(2)))) GRID",
                            detail: "Claim before the \(value.grid.burnDeadlineDays)-day protocol deadline"
                        )
                        MetricCard(
                            label: "Solana \(value.solana.network)",
                            value: value.solana.balance.map {
                                "\($0.formatted(.number.precision(.fractionLength(2)))) GRID"
                            } ?? "Not linked",
                            detail: value.solana.address ?? "Create or import a reward wallet"
                        )
                    }
                    RouteCard(snapshot: value)
                    if !value.auth.initialized {
                        SetupCallout(model: model)
                    } else if !value.grid.initialized {
                        ActionCallout(
                            title: "Create your GRID wallet",
                            detail: "Derive the grid0 address from your protected operator key.",
                            button: "Initialize GRID wallet"
                        ) {
                            model.act(["action": "initializeGrid"])
                        }
                    } else if value.grid.unclaimed > 0 {
                        ActionCallout(
                            title: "Mining rewards are ready",
                            detail: "\(value.grid.unclaimed.formatted()) GRID is waiting on the local chain.",
                            button: "Claim all"
                        ) {
                            model.act(["action": "claim"])
                        }
                    }
                } else if model.busy {
                    ProgressView("Loading the GRID chain…").frame(maxWidth: .infinity, minHeight: 300)
                }
            }
        }
    }
}

struct RouteCard: View {
    let snapshot: WalletSnapshot
    var body: some View {
        HStack(spacing: 10) {
            LayerPill(title: "GRID", detail: "Compute utility", active: snapshot.grid.initialized)
            Image(systemName: "arrow.right").foregroundStyle(.secondary)
            LayerPill(title: "SOL", detail: "Fast settlement", active: snapshot.solana.configured)
            Image(systemName: "arrow.right").foregroundStyle(.secondary)
            LayerPill(title: "BTC", detail: "Security + exit", active: snapshot.bitcoin.live)
            Spacer()
            Text(snapshot.bitcoin.live ? "LIVE" : "CONSOLIDATION ROADMAP")
                .font(.caption2.weight(.bold))
                .foregroundStyle(snapshot.bitcoin.live ? .green : .orange)
        }
        .padding(20)
        .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 18))
    }
}

struct LayerPill: View {
    let title: String
    let detail: String
    let active: Bool
    var body: some View {
        HStack {
            Circle().fill(active ? .green : .secondary.opacity(0.4)).frame(width: 8, height: 8)
            VStack(alignment: .leading) {
                Text(title).font(.headline)
                Text(detail).font(.caption2).foregroundStyle(.secondary)
            }
        }
    }
}

struct ActionCallout: View {
    let title: String
    let detail: String
    let button: String
    let action: () -> Void
    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 5) {
                Text(title).font(.headline)
                Text(detail).font(.subheadline).foregroundStyle(.secondary)
            }
            Spacer()
            Button(button, action: action).buttonStyle(.borderedProminent)
        }
        .padding(20)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 18))
    }
}

struct SetupCallout: View {
    @ObservedObject var model: WalletModel
    @State private var mode = "keyphrase"
    @State private var password = ""
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Protect your GRID wallet").font(.headline)
            Text("Use the existing GRID vault workflow. A 24-word recovery phrase is recommended for a portable wallet.")
                .foregroundStyle(.secondary)
            Picker("Protection", selection: $mode) {
                Text("24-word phrase").tag("keyphrase")
                Text("Passkey").tag("passkey")
                Text("Password").tag("password")
                Text("Combo").tag("combo")
            }
            .pickerStyle(.segmented)
            if mode == "password" || mode == "combo" {
                SecureField("Wallet password", text: $password).textFieldStyle(.roundedBorder)
            }
            Button("Create encrypted vault") {
                var payload: [String: Any] = ["action": "setup\(mode.capitalized)"]
                if mode == "password" || mode == "combo" { payload["password"] = password }
                model.act(payload)
            }
            .buttonStyle(.borderedProminent)
            .disabled((mode == "password" || mode == "combo") && password.isEmpty)
        }
        .padding(22)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 18))
    }
}

struct SendView: View {
    @ObservedObject var model: WalletModel
    @State private var recipient = ""
    @State private var amount = ""
    @State private var memo = ""
    @State private var confirming = false
    var body: some View {
        VStack(alignment: .leading, spacing: 24) {
            PageTitle(eyebrow: "GRID chain", title: "Send GRID", subtitle: "Signed locally with your protected operator key.")
            Form {
                TextField("grid0 recipient", text: $recipient)
                TextField("Amount", text: $amount)
                TextField("Memo (optional)", text: $memo)
                HStack {
                    Text("Available")
                    Spacer()
                    Text("\(model.snapshot?.grid.balance ?? 0, specifier: "%.6f") GRID")
                }
                Button("Review and send") {
                    confirming = true
                }
                .buttonStyle(.borderedProminent)
                .disabled(
                    recipient.isEmpty
                        || (Double(amount) ?? 0) <= 0
                        || model.snapshot?.auth.unlocked != true
                )
            }
            .formStyle(.grouped)
            Spacer()
        }
        .confirmationDialog(
            "Send \(amount) GRID?",
            isPresented: $confirming,
            titleVisibility: .visible
        ) {
            Button("Send GRID", role: .destructive) {
                guard let value = Double(amount), value > 0 else { return }
                model.act([
                    "action": "send",
                    "to": recipient,
                    "amount": value,
                    "memo": memo,
                ])
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Recipient: \(recipient)\nThis transaction cannot be undone.")
        }
    }
}

struct ReceiveView: View {
    @ObservedObject var model: WalletModel
    @State private var rail = ReceiveRail.grid

    private var address: String? {
        switch rail {
        case .grid: model.snapshot?.grid.address
        case .solana: model.snapshot?.solana.address
        }
    }

    private var addressLabel: String {
        switch rail {
        case .grid: "GRID chain address"
        case .solana: "Solana \(model.snapshot?.solana.network ?? "devnet") reward address"
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 24) {
            PageTitle(eyebrow: "Receive", title: "Receive funds", subtitle: "Choose the correct network, then share only this public address.")
            Picker("Network", selection: $rail) {
                ForEach(ReceiveRail.allCases) { option in
                    Text(option.rawValue).tag(option)
                }
            }
            .pickerStyle(.segmented)
            .frame(maxWidth: 420)

            if let address {
                VStack(spacing: 18) {
                    Text(addressLabel)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                    QRCodeView(value: address)
                        .frame(width: 244, height: 244)
                    Text(address).font(.system(.body, design: .monospaced)).textSelection(.enabled)
                    Button("Copy address") { NSPasteboard.general.setString(address, forType: .string) }
                    Text("Scan or copy this address. Confirm the network before sending.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity)
                .padding(40)
                .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 20))
            } else {
                Text(
                    rail == .grid
                        ? "Initialize your GRID wallet from Overview first."
                        : "Create or import a Solana reward wallet from Security first."
                )
                .foregroundStyle(.secondary)
            }
            Spacer()
        }
    }
}

private enum ReceiveRail: String, CaseIterable, Identifiable {
    case grid = "GRID"
    case solana = "Solana rewards"
    var id: String { rawValue }
}

private struct QRCodeView: View {
    let value: String

    private var image: NSImage? {
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(value.utf8)
        filter.correctionLevel = "M"
        guard let output = filter.outputImage else { return nil }
        let scaled = output.transformed(by: CGAffineTransform(scaleX: 12, y: 12))
        let context = CIContext()
        guard let cgImage = context.createCGImage(scaled, from: scaled.extent) else { return nil }
        return NSImage(cgImage: cgImage, size: NSSize(width: 244, height: 244))
    }

    var body: some View {
        Group {
            if let image {
                Image(nsImage: image)
                    .interpolation(.none)
                    .resizable()
            } else {
                Image(systemName: "exclamationmark.triangle")
                    .font(.largeTitle)
            }
        }
        .padding(12)
        .background(Color.white, in: RoundedRectangle(cornerRadius: 16))
        .accessibilityLabel("QR code for public receive address")
    }
}

struct ActivityView: View {
    @ObservedObject var model: WalletModel
    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            PageTitle(eyebrow: "Local chain", title: "Activity", subtitle: "Claims, sends, receives, mints, and protocol burns.")
            Table(model.snapshot?.activity ?? []) {
                TableColumn("Type") { tx in Text(tx.kind.uppercased()).font(.caption.weight(.semibold)) }
                TableColumn("Amount") { tx in Text("\(tx.amount, specifier: "%.6f") GRID") }
                TableColumn("Memo") { tx in Text(tx.memo ?? "—").lineLimit(1) }
                TableColumn("Time") { tx in Text(String(tx.at.prefix(19))).font(.caption) }
            }
        }
    }
}

struct SecurityView: View {
    @ObservedObject var model: WalletModel
    @Binding var theme: String
    @State private var password = ""
    @State private var phrase = ""
    @State private var solanaAddress = ""
    @State private var networkMode = "genesis"
    @State private var truthUrl = ""
    @State private var p2pPeer = ""
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                PageTitle(eyebrow: "Custody", title: "Security & settlement", subtitle: "Vault protection, themes, and Solana reward routing.")
                GroupBox("Appearance") {
                    Picker("Theme", selection: $theme) {
                        ForEach(AppTheme.allCases) { Text($0.rawValue).tag($0.rawValue) }
                    }
                    .pickerStyle(.segmented)
                    .padding(.vertical, 8)
                }
                GroupBox("GRID network") {
                    VStack(alignment: .leading, spacing: 12) {
                        HStack {
                            Circle()
                                .fill(model.snapshot?.network.connected == true && model.snapshot?.network.trusted == true ? .green : .orange)
                                .frame(width: 8, height: 8)
                            Text(networkStatus)
                        }
                        Picker("Node", selection: $networkMode) {
                            Text("GRID Genesis").tag("genesis")
                            Text("Local node").tag("local")
                            Text("Custom node").tag("custom")
                        }
                        .pickerStyle(.segmented)
                        if networkMode == "custom" {
                            TextField("Truth URL (http://host:9100)", text: $truthUrl)
                                .textFieldStyle(.roundedBorder)
                            TextField("P2P peer (host:9900)", text: $p2pPeer)
                                .textFieldStyle(.roundedBorder)
                        }
                        Button("Save network") {
                            model.act([
                                "action": "setNetwork",
                                "mode": networkMode,
                                "truthUrl": truthUrl,
                                "p2pPeer": p2pPeer,
                            ])
                        }
                        Text(model.snapshot?.network.truthUrl ?? "http://genesis.grid-compute.com:9100")
                            .font(.system(.caption, design: .monospaced))
                            .foregroundStyle(.secondary)
                        Text(model.snapshot?.network.p2pPeer ?? "genesis.grid-compute.com:9900")
                            .font(.system(.caption, design: .monospaced))
                            .foregroundStyle(.secondary)
                    }
                    .padding(.vertical, 8)
                }
                GroupBox("GRID vault") {
                    VStack(alignment: .leading, spacing: 12) {
                        Text(model.snapshot?.auth.detail ?? "Checking…").foregroundStyle(.secondary)
                        if model.snapshot?.auth.initialized == false {
                            SetupCallout(model: model)
                        } else if model.snapshot?.auth.unlocked == false {
                            if model.snapshot?.auth.mode == "password" || model.snapshot?.auth.mode == "combo" {
                                SecureField("Password", text: $password).textFieldStyle(.roundedBorder)
                            }
                            if model.snapshot?.auth.mode == "keyphrase" || model.snapshot?.auth.mode == "combo" {
                                SecureField("24-word recovery phrase", text: $phrase).textFieldStyle(.roundedBorder)
                            }
                            Button("Unlock vault") {
                                model.act([
                                    "action": "unlock",
                                    "password": password,
                                    "keyphrase": phrase,
                                ])
                                password = ""
                                phrase = ""
                            }
                            .buttonStyle(.borderedProminent)
                        }
                    }
                    .padding(.vertical, 8)
                }
                GroupBox("Solana mining rewards") {
                    VStack(alignment: .leading, spacing: 12) {
                        if let address = model.snapshot?.solana.address {
                            Text(address).font(.system(.caption, design: .monospaced)).textSelection(.enabled)
                            Text("\(model.snapshot?.solana.balance ?? 0, specifier: "%.6f") GRID · devnet")
                        } else {
                            Button("Create local Solana reward wallet") {
                                model.act(["action": "createSolana"])
                            }
                            .buttonStyle(.borderedProminent)
                            HStack {
                                TextField("Or import a Solana public address", text: $solanaAddress)
                                Button("Import") {
                                    model.act(["action": "importSolana", "address": solanaAddress])
                                }
                                .disabled(solanaAddress.isEmpty)
                            }
                        }
                    }
                    .padding(.vertical, 8)
                }
                GroupBox("Bitcoin consolidation layer") {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("GRID → SOL → BTC").font(.headline)
                        Text("Bitcoin is the Transact Security Layer. Direct conversion is shown as a roadmap until audited liquidity and exit execution are live.")
                            .foregroundStyle(.secondary)
                    }
                    .padding(.vertical, 8)
                }
            }
            .onAppear { loadNetworkSettings() }
            .onChange(of: model.snapshot?.network.mode) { _ in loadNetworkSettings() }
        }
    }

    private var networkStatus: String {
        guard let network = model.snapshot?.network else { return "Connecting to Genesis…" }
        if network.connected && network.trusted {
            return "Connected · block \(network.height ?? 0)"
        }
        return network.error ?? "Genesis unavailable"
    }

    private func loadNetworkSettings() {
        guard let network = model.snapshot?.network else { return }
        networkMode = network.mode
        truthUrl = network.truthUrl
        p2pPeer = network.p2pPeer
    }
}

struct RecoveryPhraseView: View {
    let phrase: String
    let done: () -> Void
    @State private var confirmed = false
    var body: some View {
        VStack(alignment: .leading, spacing: 22) {
            Image(systemName: "key.horizontal.fill").font(.largeTitle)
            Text("Your 24-word recovery phrase").font(.title2.weight(.semibold))
            Text("Write these words down in order and store them offline. GRID will not show them again.")
                .foregroundStyle(.secondary)
            Text(phrase)
                .font(.system(.body, design: .monospaced))
                .textSelection(.enabled)
                .padding(20)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(.quaternary, in: RoundedRectangle(cornerRadius: 14))
            Toggle("I saved the phrase offline", isOn: $confirmed)
            HStack {
                Spacer()
                Button("Continue", action: done)
                    .buttonStyle(.borderedProminent)
                    .disabled(!confirmed)
            }
        }
        .padding(30)
        .frame(width: 600)
    }
}
