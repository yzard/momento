import SwiftUI
import BackgroundTasks

@main struct MomentoApp: App {
    @UIApplicationDelegateAdaptor(BackupAppDelegate.self) private var delegate
    @State private var model = AppModel.shared
    @Environment(\.scenePhase) private var phase

    var body: some Scene {
        WindowGroup {
            Group {
                if let session = model.session {
                    if session.user.mustChangePassword { PasswordView(required: true) }
                    else { MainShell(namespace: session.namespace).id(session.namespace) }
                } else { LoginView() }
            }
            .environment(model)
            .preferredColorScheme(model.theme == "system" ? nil : model.theme == "dark" ? .dark : .light)
            .tint(Color("InterfaceAccent"))
            .toggleStyle(SystemSwitchToggleStyle())
            .task { await model.prepareSession() }
            .alert("Momento", isPresented: Binding(get: { model.error != nil }, set: { if !$0 { model.error = nil } })) {
                Button("OK") { model.error = nil }
            } message: { Text(model.error ?? "") }
        }
        .onChange(of: phase) { _, phase in
            if phase == .background { model.backup?.schedule() }
        }
    }
}

final class BackupAppDelegate: NSObject, UIApplicationDelegate {
    func application(_ application: UIApplication, didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?) -> Bool {
        BGTaskScheduler.shared.register(forTaskWithIdentifier: BackupController.taskIdentifier, using: .main) { task in
            MainActor.assumeIsolated {
                let work = Task { @MainActor in
                    let model = AppModel.shared
                    await model.prepareSession()
                    await model.backup?.runScheduled()
                    task.setTaskCompleted(success: !Task.isCancelled && model.backup?.latestError == nil)
                }
                task.expirationHandler = { work.cancel() }
            }
        }
        return true
    }
}

struct LoginView: View {
    @Environment(AppModel.self) private var model
    @State private var username = ""
    @State private var password = ""
    @FocusState private var focused: LoginField?
    private enum LoginField { case server, username, password }
    var body: some View {
        @Bindable var model = model
        NavigationStack {
            Form {
                Section {
                    VStack(alignment: .leading, spacing: 12) {
                        Image("BrandMark").resizable().scaledToFit().frame(width: 72, height: 72).clipShape(.rect(cornerRadius: 18)).accessibilityLabel("Momento")
                        Text("Your moments. Your home.").font(.largeTitle.bold())
                        Text("Connect to your Momento server to browse and back up your library.").foregroundStyle(.secondary)
                    }.padding(.vertical, 22)
                }
                Section("Server") {
                    TextField("https://photos.example.com", text: $model.loginOrigin).keyboardType(.URL).textInputAutocapitalization(.never).autocorrectionDisabled().accessibilityIdentifier("server").focused($focused, equals: .server)
                }
                Section("Account") {
                    TextField("Username", text: $username).textContentType(.username).textInputAutocapitalization(.never).autocorrectionDisabled().accessibilityIdentifier("username").focused($focused, equals: .username)
                    SecureField("Password", text: $password).textContentType(.password).accessibilityIdentifier("password").focused($focused, equals: .password)
                }
                Button { focused = nil; Task { await model.login(origin: model.loginOrigin, username: username, password: password) } } label: {
                    HStack { Spacer(); if model.busy { ProgressView() }; Text("Sign in"); Spacer() }
                }.buttonStyle(ProminentGlassButtonStyle()).disabled(model.busy || model.loginOrigin.isEmpty || username.isEmpty || password.isEmpty)
            }.navigationTitle("Momento")
                .onChange(of: model.loginOrigin) { _, _ in username = ""; password = "" }
        }
    }
}
