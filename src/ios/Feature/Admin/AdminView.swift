import SwiftUI

struct AdminView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.pageActive) private var active
    let destination: Destination
    @State private var users: [User] = []
    @State private var imports: ImportStatus?
    @State private var metadata: MetadataStatus?
    @State private var ai: AiStatusResponse?
    @State private var loaded = false
    @State private var creatingUser = false
    @State private var roleChange: UserRoleChange?
    @State private var width: CGFloat = 390
    @State private var confirmation: AdminAction?
    @State private var result: String?
    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 12) {
            if model.session?.user.role != "admin" { Text("Administrator access required") }
            else {
                if let result { Text(result).font(.caption).textSelection(.enabled).padding() }
                switch destination {
                case .users: usersContent
                case .imports: importContent
                case .metadata: metadataContent
                default: aiContent
                }
                Button("Refresh status") { Task { await load() } }.frame(maxWidth: .infinity)
            }
            }.padding(16)
        }.background(Color(uiColor: .systemGroupedBackground)).onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
            .task { if !loaded { await load() } }
            .task(id: active) {
                guard active && destination != .users else { return }
                do {
                    while !Task.isCancelled {
                        try await Task.sleep(for: .seconds(5))
                        if !model.busy && confirmation == nil && roleChange == nil { await load() }
                    }
                } catch is CancellationError { }
                catch { model.error = error.localizedDescription }
            }
            .refreshable { await load() }
            .sheet(isPresented: $creatingUser) { CreateUserEditor { users.append($0) } }
            .confirmationDialog("Change user role?", isPresented: Binding(get: { roleChange != nil }, set: { if !$0 { roleChange = nil } }), titleVisibility: .visible) {
                if let change = roleChange {
                    Button("Change role") { Task { await updateUser(change.user, role: change.role, active: nil); roleChange = nil } }
                }
            }
            .confirmationDialog(confirmation?.title ?? "Confirm", isPresented: Binding(get: { confirmation != nil }, set: { if !$0 { confirmation = nil } }), titleVisibility: .visible) {
                if let action = confirmation { Button(action.title, role: .destructive) { Task { await run(action); confirmation = nil } } }
            }
    }
    private func columns(_ base: [CGFloat]) -> [CGFloat] {
        let ratio = max(1, (width - 32) / base.reduce(0, +))
        return base.map { $0 * ratio }
    }
    private var usersContent: some View {
        Group {
            AdminPanel(title: "User Management", description: "Manage sign-in access and administrator permissions.") {
                Button("Create user", systemImage: "person.badge.plus") { creatingUser = true }.buttonStyle(ProminentGlassButtonStyle())
            }
            let widths = columns([190, 250, 150, 120, 210])
            AdminTable(headers: ["Username", "Email", "Role", "Status", "Actions"], widths: widths, alignments: Array(repeating: .leading, count: 5)) {
                ForEach(users) { user in
                    Divider()
                    HStack(spacing: 0) {
                        VStack(alignment: .leading) {
                            Text(user.username).fontWeight(.semibold)
                            if user.isReserved { Text("RESERVED").font(.caption).foregroundStyle(.tint) }
                        }.adminCell(widths[0])
                        Text(user.email).adminCell(widths[1])
                        Button(user.role == "admin" ? "Admin" : "User") {
                            roleChange = UserRoleChange(user: user, role: user.role == "admin" ? "user" : "admin")
                        }.buttonStyle(.glass).disabled(model.busy || !AdminPolicy.canChangeRole(user, currentUserId: model.session?.user.id)).adminCell(widths[2])
                        Text(user.isActive ? "Active" : "Inactive").fontWeight(.semibold).foregroundStyle(user.isActive ? Color.accentColor : .red).adminCell(widths[3])
                        HStack {
                            Button(user.isActive ? "Deactivate" : "Activate") { Task { await updateUser(user, role: nil, active: !user.isActive) } }
                                .buttonStyle(.glass).accessibilityLabel("\(user.isActive ? "Deactivate" : "Activate") \(user.username)")
                            Button("Delete \(user.username)", systemImage: "trash", role: .destructive) { confirmation = AdminAction(title: "Delete \(user.username)?", path: "user/delete", feature: nil, userId: user.id) }.labelStyle(.iconOnly)
                        }.disabled(model.busy || !AdminPolicy.canDeactivateOrDelete(user, currentUserId: model.session?.user.id)).adminCell(widths[4])
                    }.font(.subheadline)
                }
            }.accessibilityIdentifier("adminUsersTable")
        }
    }
    private var importContent: some View {
        Group {
            AdminPanel(title: "WebDAV", description: "Upload media through a WebDAV client using your Momento credentials.") {
                Text("WebDAV URL").font(.subheadline.bold())
                Text((model.session?.origin.absoluteString ?? "") + "/webdav/").textSelection(.enabled)
                Text("Use the same username and password used to sign in to Momento.").foregroundStyle(.secondary)
            }
            AdminPanel(title: "Local Import", description: "Import media staged in the server import directory.") {
                Text("Import directory").font(.subheadline.bold())
                Text("/data/imports/").textSelection(.enabled)
                AdminMetrics(metrics: [metric("Status", imports?.status), metric("Imported", imports?.successfulImports),
                    metric("Failed", imports?.failedImports, failed: (imports?.failedImports ?? 0) > 0), metric("Total Media", imports?.totalMedia)])
                if let imports, imports.status == "running", imports.totalFiles > 0 {
                    Text("\(imports.processedFiles) / \(imports.totalFiles) files").font(.caption)
                    ProgressView(value: Double(imports.processedFiles), total: Double(imports.totalFiles))
                }
                Button("Start import", systemImage: "play.fill") { Task { await run(AdminAction(title: "Import", path: "import/local", feature: nil, userId: nil)) } }
                    .buttonStyle(ProminentGlassButtonStyle()).disabled(model.busy || imports?.status == "running")
                AdminFailureLog(title: "Import failure log", entries: imports?.errors ?? [])
            }
        }
    }
    private var metadataContent: some View {
        AdminPanel(title: "Metadata", description: "Generate thumbnails and technical metadata before AI processing.") {
            AdminMetrics(metrics: [metric("Queued", metadata?.queuedJobs), metric("Waiting for Journal rollback (queued)", metadata?.waitingForRollbackJobs),
                metric("Processing", metadata?.processingJobs), metric("Completed", metadata?.completedJobs),
                metric("Failed", metadata?.failedJobs, failed: (metadata?.failedJobs ?? 0) > 0)])
            actionButtons(prefix: "metadata", feature: nil, running: ["queued", "processing"].contains(metadata?.status ?? ""),
                blocked: metadata == nil || ["cancelling", "cleaning"].contains(metadata?.status ?? ""), label: "Metadata", compact: false)
            AdminFailureLog(title: "Metadata failure log", entries: metadata?.errors ?? [])
        }
    }
    private var aiContent: some View {
        Group {
            AdminPanel(title: "AI processing", description: "Each task runs independently through the durable server queue.") {
                if let ai { Text("\(ai.faceGroups) face groups · \(ai.deduplicate.clustersCreated) duplicate groups") }
            }
            AdminPanel(title: "AI work status", description: "Queued, submitting, submitted, failed, and completed jobs by feature.") {
                let widths = columns([180, 108, 108, 108, 108, 108])
                AdminTable(headers: ["Feature", "Queued", "Submitting", "Submitted", "Failed", "Completed"], widths: widths, alignments: [.leading] + Array(repeating: .trailing, count: 5)) {
                    ForEach(AdminFeature.allCases) { feature in
                        Divider()
                        let jobs = feature.jobs(ai)
                        HStack(spacing: 0) {
                            Text(feature.title).adminCell(widths[0])
                            ForEach(Array([jobs?.queued, jobs?.submitting, jobs?.submitted, jobs?.failed, jobs?.completed].enumerated()), id: \.offset) { index, value in
                                Text(String(value ?? 0)).monospacedDigit().frame(maxWidth: .infinity, alignment: .trailing).adminCell(widths[index + 1])
                            }
                        }.font(.subheadline)
                    }
                }.accessibilityIdentifier("adminAIStatusTable")
            }
            AdminPanel(title: "AI job controls", description: "Cron schedules use five fields and the server's system timezone.") {
                HStack {
                    VStack(alignment: .leading) { Text("All AI jobs").fontWeight(.semibold); Text(allAIRunning ? "running" : "idle").font(.caption).foregroundStyle(.secondary) }
                    Spacer()
                    actionButtons(prefix: "ai", feature: nil, running: allAIRunning, blocked: ai == nil, label: "All AI jobs", compact: true)
                }
                AdminTable(headers: ["Feature", "Minute", "Hour", "Day", "Month", "Weekday", "Save", "Start / Cancel", "Clean"], widths: [176, 92, 92, 92, 92, 92, 72, 96, 72], alignments: [.leading] + Array(repeating: .center, count: 8)) {
                    ForEach(AdminFeature.allCases) { feature in
                        Divider()
                        HStack(spacing: 0) {
                            VStack(alignment: .leading) { Text(feature.title).fontWeight(.semibold); Text(feature.state(ai) ?? "Not loaded").font(.caption).foregroundStyle(.secondary) }.adminCell(176)
                            AdminScheduleFields(schedule: ai?.schedules.first { $0.feature == feature.rawValue }, feature: feature) { changed in
                                if let i = ai?.schedules.firstIndex(where: { $0.feature == changed.feature }) { ai?.schedules[i] = changed }
                            }
                            actionButtons(prefix: "ai/\(feature.rawValue)", feature: feature.rawValue, running: AdminPolicy.activeAI(feature.state(ai) ?? ""),
                                blocked: ai == nil || (feature != .deduplicate && ai?.tasks.first { $0.task == feature.rawValue }?.enabled != true), label: feature.title, compact: true)
                                .frame(width: 168)
                        }.font(.subheadline)
                    }
                }.accessibilityIdentifier("adminAIControlsTable")
                AdminFailureLog(title: "AI failure log", entries: (ai?.tasks.flatMap { task in task.errors.map { "\(task.task): \($0)" } } ?? []) + (ai?.deduplicate.error.map { [$0] } ?? []))
            }
        }
    }
    private var allAIRunning: Bool { ai.map { $0.tasks.contains { AdminPolicy.activeAI($0.state) } || AdminPolicy.activeAI($0.deduplicate.status) } ?? false }
    private func metric<T>(_ label: String, _ value: T?, failed: Bool = false) -> AdminMetric { AdminMetric(label: label, value: value.map { String(describing: $0) } ?? "—", failed: failed) }
    private func actionButtons(prefix: String, feature: String?, running: Bool, blocked: Bool, label: String, compact: Bool) -> some View {
        HStack(spacing: compact ? 0 : 10) {
            Button {
                if running { confirmation = AdminAction(title: "Cancel processing?", path: "\(prefix)/cancel", feature: feature, userId: nil) }
                else { Task { await run(AdminAction(title: "Start", path: "\(prefix)/\(prefix == "metadata" ? "generate" : "start")", feature: feature, userId: nil)) } }
            } label: { HStack { Image(systemName: running ? "stop.fill" : "play.fill"); if !compact { Text(running ? "Cancel" : prefix == "metadata" ? "Generate" : "Start") } }.frame(width: compact ? 24 : nil, height: 28).frame(maxWidth: compact ? nil : .infinity) }
                .buttonStyle(ProminentGlassButtonStyle()).frame(width: compact ? 96 : nil).frame(maxWidth: compact ? nil : .infinity)
                .accessibilityLabel("\(running ? "Cancel" : prefix == "metadata" ? "Generate" : "Start") \(label)")
            Button { confirmation = AdminAction(title: "Clean generated results?", path: "\(prefix)/clean", feature: feature, userId: nil) }
                label: { HStack { Image(systemName: "eraser"); if !compact { Text("Clean Data") } }.frame(width: compact ? 24 : nil, height: 28).frame(maxWidth: compact ? nil : .infinity) }
                .buttonStyle(.glass).frame(width: compact ? 72 : nil).frame(maxWidth: compact ? nil : .infinity).disabled(running).accessibilityLabel("Clean \(label) data")
        }.disabled(model.busy || blocked)
    }
    private func updateUser(_ user: User, role: String?, active: Bool?) async {
        guard AdminPolicy.canChangeRole(user, currentUserId: model.session?.user.id), active == nil || AdminPolicy.canDeactivateOrDelete(user, currentUserId: model.session?.user.id) else { return }
        await model.perform {
            let changed: User = try await model.api.call("user/update", AdminUserUpdateRequest(userId: user.id, role: role, isActive: active))
            if let index = users.firstIndex(where: { $0.id == changed.id }) { users[index] = changed }
        }
    }
    private func load() async {
        guard model.session?.user.role == "admin" else { return }
        await model.perform {
            switch destination {
            case .users: let response: UsersResponse = try await model.api.call("user/list", EmptyRequest()); users = response.users
            case .imports: imports = try await model.api.call("import/status", EmptyRequest())
            case .metadata: metadata = try await model.api.call("metadata/status", EmptyRequest())
            default: ai = try await model.api.call("ai/status", EmptyRequest())
            }
            loaded = true
        }
    }
    private func run(_ action: AdminAction) async {
        await model.perform {
            if let userId = action.userId {
                guard let user = users.first(where: { $0.id == userId }), AdminPolicy.canDeactivateOrDelete(user, currentUserId: model.session?.user.id) else {
                    throw ClientError.message("This account cannot be deleted.")
                }
                let response: MessageResponse = try await model.api.call(action.path, AdminUserIdRequest(userId: userId)); result = response.message; users.removeAll { $0.id == userId }
            } else if action.path.hasPrefix("ai/") {
                let response: AiActionResponse = try await model.api.call(action.path, EmptyRequest())
                result = response.results.map { "\($0.feature): \($0.outcome)\($0.error.map { " — " + $0 } ?? "")" }.joined(separator: "\n")
            } else if action.path.hasPrefix("metadata/") {
                let response: MetadataActionResponse = try await model.api.call(action.path, EmptyRequest()); result = "\(response.message) (\(response.affectedJobs))"
            } else {
                let response: MessageResponse = try await model.api.call(action.path, EmptyRequest()); result = response.message
            }
        }
        if action.userId == nil { await load() }
    }
}

private struct AdminAction { let title: String; let path: String; let feature: String?; let userId: Int64? }
extension AiFeatureSchedule: Identifiable { var id: String { feature } }

private struct UserRoleChange { let user: User; let role: String }

private struct CreateUserEditor: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let saved: (User) -> Void
    @State private var username = ""
    @State private var email = ""
    @State private var password = ""
    @State private var role = "user"
    var body: some View {
        NavigationStack {
            Form {
                TextField("Username", text: $username).textInputAutocapitalization(.never)
                TextField("Email", text: $email).keyboardType(.emailAddress).textInputAutocapitalization(.never)
                SecureField("Initial password", text: $password).textContentType(.newPassword)
                Picker("Role", selection: $role) { Text("User").tag("user"); Text("Administrator").tag("admin") }
            }.navigationTitle("New user").toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) { Button("Save") { Task { await model.perform {
                    let user: User = try await model.api.call("user/create", AdminUserCreateRequest(username: username, email: email, password: password, role: role))
                    saved(user); password = ""; dismiss()
                } } }.disabled(model.busy || username.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || email.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || password.count < 8) }
            }
        }.interactiveDismissDisabled()
    }
}
