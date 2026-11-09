import SwiftUI
import Photos

/// Flat, icon-led rows shared by the settings sections on compact and expanded screens.
private struct SettingsRow<Details: View, Actions: View>: View {
    let icon: String?
    let title: String
    @ViewBuilder let details: Details
    @ViewBuilder let actions: Actions
    var body: some View {
        HStack(alignment: .top, spacing: 16) {
            if let icon { Image(systemName: icon).font(.title3).frame(width: 24).padding(.top, 2).accessibilityHidden(true) }
            VStack(alignment: .leading, spacing: 5) {
                Text(title).font(.body.weight(.medium))
                details.font(.subheadline).foregroundStyle(.secondary)
            }.frame(maxWidth: .infinity, alignment: .leading)
            actions.font(.subheadline).buttonStyle(.borderless).fixedSize(horizontal: false, vertical: true)
        }.padding(.horizontal, 20).padding(.vertical, 16).contentShape(.rect)
    }
}

private struct SettingsControlRow<Actions: View>: View {
    let icon: String?
    let title: String
    @ViewBuilder let actions: Actions
    var body: some View {
        HStack(spacing: 12) {
            if let icon { Image(systemName: icon).font(.title3).frame(width: 24).accessibilityHidden(true) }
            Text(title).font(.body.weight(.medium))
            Spacer(minLength: 8)
            actions.font(.subheadline).buttonStyle(.borderless).fixedSize()
        }.padding(.horizontal, 20).padding(.vertical, 12)
    }
}

private struct SettingsChoiceLabel: View {
    let value: String
    var body: some View {
        HStack(spacing: 6) {
            Text(value)
            Image(systemName: "chevron.down").accessibilityHidden(true)
        }
    }
}

struct BackupRunButton: View {
    let running: Bool
    let enabled: Bool
    let start: () -> Void
    let cancel: () -> Void
    var body: some View {
        Button(role: running ? .destructive : nil) {
            if running { cancel() } else { start() }
        } label: { Text(running ? "Cancel" : "Back up now") }
            .disabled(!enabled)
            .accessibilityIdentifier("backupRunAction")
    }
}

struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var password = false
    @State private var logout = false
    @State private var clearingCache = false
    var body: some View {
        @Bindable var model = model
        ScrollView {
            VStack(spacing: 0) {
                SettingsRow(icon: "person.crop.circle", title: model.session?.user.username ?? "Account") {
                    Text(model.session?.origin.absoluteString ?? "No server selected").textSelection(.enabled)
                    if let capabilities = model.capabilities { Text("Server version \(capabilities.appVersion)") }
                } actions: {
                    VStack(alignment: .trailing, spacing: 12) {
                        Button("Change password") { password = true }
                        Button("Sign out", role: .destructive) { logout = true }.accessibilityIdentifier("signOut")
                    }
                }
                Divider()
                SettingsRow(icon: "info.circle", title: "Momento iOS Client \(Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "1.0")") {
                    EmptyView()
                } actions: { EmptyView() }
                Divider()
                if let backup = model.backup { BackupSettingsView(backup: backup, available: model.capabilities?.backup.enabled == true) }
                Divider()
                SettingsControlRow(icon: "internaldrive", title: "Local Cache Limit") {
                    HStack(spacing: 12) {
                    Button("Clear", role: .destructive) {
                        clearingCache = true
                        Task {
                            defer { clearingCache = false }
                            await model.perform { try await model.cache?.clear() }
                        }
                    }.foregroundStyle(.red).tint(.red)
                        .disabled(clearingCache || model.cache == nil)
                        .accessibilityIdentifier("clearMediaCache")
                    Menu {
                        ForEach([256, 512, 1024, 2048], id: \.self) { limit in
                            Button {
                                Task { await model.perform {
                                    try await model.cache?.setLimit(limit * 1024 * 1024)
                                    model.cacheMB = limit
                                } }
                            } label: {
                                if model.cacheMB == limit { Label("\(limit) MiB", systemImage: "checkmark") }
                                else { Text("\(limit) MiB") }
                            }
                        }
                    } label: {
                        SettingsChoiceLabel(value: "\(model.cacheMB) MiB")
                    }.disabled(clearingCache).accessibilityIdentifier("mediaCacheSettings")
                    }
                }
                Divider()
                SettingsControlRow(icon: "paintpalette", title: "Appearance") {
                    Menu {
                        Picker("Appearance", selection: $model.theme) {
                            Text("Follow system").tag("system")
                            Text("Light").tag("light")
                            Text("Dark").tag("dark")
                        }
                    } label: {
                        SettingsChoiceLabel(value: model.theme == "system" ? "Follow system" : model.theme.capitalized)
                    }.accessibilityIdentifier("appearanceSettings")
                }
            }.frame(maxWidth: .infinity)
        }
        .ignoresSafeArea(.container, edges: .horizontal)
        .accessibilityIdentifier("settingsScrollView")
        .sheet(isPresented: $password) { PasswordView(required: false) }
        .confirmationDialog("Sign out?", isPresented: $logout) {
            Button("Sign out", role: .destructive) { Task { await model.logout(localOnly: false) } }.accessibilityIdentifier("confirmSignOut")
            Button("Remove local session (offline)", role: .destructive) { Task { await model.logout(localOnly: true) } }
        }
    }
}

struct BackupSettingsView: View {
    @Environment(AppModel.self) private var model
    @Bindable var backup: BackupController
    let available: Bool
    @State private var clear = false
    @State private var repair = false
    @State private var copied = false
    @Environment(\.scenePhase) private var phase
    @State private var authorization = PHPhotoLibrary.authorizationStatus(for: .readWrite)
    var body: some View {
        VStack(spacing: 0) {
            if !available {
                SettingsRow(icon: "icloud.and.arrow.up", title: "Device backup unavailable") {
                    Text("This server has disabled device backup.")
                } actions: { EmptyView() }
            } else {
                SettingsRow(icon: nil, title: "Backup permissions") {
                    Text(permission)
                    if authorization == .denied { Text("Photos access was denied. Enable access in Settings to back up your library.") }
                } actions: {
                    if authorization == .denied {
                        Button("Open Photos permissions") { if let url = URL(string: UIApplication.openSettingsURLString) { UIApplication.shared.open(url) } }
                    }
                }
                VStack(alignment: .leading, spacing: 8) {
                    Toggle("Camera photos and videos only", isOn: $backup.cameraOnly)
                    if backup.cameraOnly { Text("Uses original camera metadata. Screenshots and files without camera metadata are excluded.").font(.subheadline).foregroundStyle(.secondary) }
                }.padding(.horizontal, 20).padding(.vertical, 12)
                Toggle("Use mobile data", isOn: $backup.cellular).padding(.horizontal, 20).padding(.vertical, 12)
                Toggle("Daily background backup", isOn: $backup.daily).padding(.horizontal, 20).padding(.vertical, 12)
                SettingsRow(icon: "icloud.and.arrow.up", title: "Back up this device") {
                    EmptyView()
                } actions: {
                    BackupRunButton(
                        running: backup.running,
                        enabled: !backup.running || !backup.cancelling,
                        start: { backup.daily = true; backup.start() },
                        cancel: backup.cancel
                    ).frame(maxWidth: 120, alignment: .trailing)
                }
                BackupProgressContent(status: backup.status, running: backup.running,
                                      completed: backup.completed, total: backup.total, errors: backup.uploadErrors)
                SettingsControlRow(icon: nil, title: "Backup diagnostics") {
                    HStack(spacing: 16) {
                        Button("Copy") {
                            UIPasteboard.general.string = backup.diagnostics
                            copied.toggle()
                            UIAccessibility.post(notification: .announcement, argument: "Diagnostic logs copied")
                        }
                            .accessibilityIdentifier("copyBackupLogs")
                        ShareLink(item: backup.diagnostics) { Image(systemName: "square.and.arrow.up").frame(minWidth: 44, minHeight: 44) }
                            .accessibilityLabel("Share logs").accessibilityIdentifier("shareBackupLogs")
                    }
                }
                SettingsControlRow(icon: nil, title: "Backup history") {
                    HStack(spacing: 16) {
                        Button("Reverify") { repair = true }.disabled(backup.running || backup.verified == backup.completed)
                            .accessibilityIdentifier("reverifyBackupHistory")
                        Button("Clear", role: .destructive) { clear = true }.disabled(!backup.canClear)
                            .accessibilityIdentifier("clearBackupHistory")
                    }
                }
            }
        }
        .task { authorization = PHPhotoLibrary.authorizationStatus(for: .readWrite); await model.perform { try await backup.reload() } }
        .onChange(of: phase) { _, value in if value == .active { authorization = PHPhotoLibrary.authorizationStatus(for: .readWrite) } }
        .onChange(of: backup.running) { _, _ in authorization = PHPhotoLibrary.authorizationStatus(for: .readWrite) }
        .sensoryFeedback(.success, trigger: copied)
        .confirmationDialog("Clear backup history? Server originals remain intact.", isPresented: $clear, titleVisibility: .visible) {
            Button("Clear history", role: .destructive) { Task { await model.perform { try await backup.clearHistory() } } }
        }
        .confirmationDialog("Queue unverified originals for another backup?", isPresented: $repair, titleVisibility: .visible) {
            Button("Re-verify") { Task { await model.perform { try await backup.repairHistory() } } }
        }
    }
    private var permission: String {
        switch authorization {
        case .authorized: "Full access"
        case .limited: "Selected photos only"
        case .denied, .restricted: "Not allowed"
        default: "Not requested"
        }
    }
}

struct PasswordView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let required: Bool
    @State private var current = ""
    @State private var new = ""
    @State private var confirmation = ""
    var body: some View {
        NavigationStack {
            Form {
                if required { Text("Change your initial password to continue.") }
                SecureField("Current password", text: $current).textContentType(.password)
                SecureField("New password", text: $new).textContentType(.newPassword)
                SecureField("Confirm password", text: $confirmation).textContentType(.newPassword)
                Button("Change password") { Task { await model.perform {
                    let _: MessageResponse = try await model.api.call("user/change-password", ChangePasswordRequest(currentPassword: current, newPassword: new))
                    let user: User = try await model.api.call("user/get", EmptyRequest())
                    try await model.api.updateUser(user); model.session?.user = user
                    current = ""; new = ""; confirmation = ""; dismiss()
                } } }.accessibilityIdentifier("submitPasswordChange")
                    .disabled(current.isEmpty || new.count < 8 || new != confirmation || model.busy)
                if !new.isEmpty && new.count < 8 { Text("Use at least 8 characters.").font(.caption).foregroundStyle(.secondary) }
            }.navigationTitle("Change password")
                .toolbar { if !required { Button("Cancel") { dismiss() } } }
        }.interactiveDismissDisabled(required || !new.isEmpty)
    }
}
