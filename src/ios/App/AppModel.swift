import SwiftUI
import Observation

@MainActor @Observable final class AppModel {
    static let shared = AppModel()
    let api: APIClient
    var session: SavedSession?
    var capabilities: Capabilities?
    var cache: MediaCache?
    var backup: BackupController?
    var error: String?
    private var operations = 0
    var busy: Bool { operations > 0 }
    var loginOrigin: String
    var trashedMedia: Set<Int64> = []
    var albumUpdates: [Int64: Album] = [:]
    var deletedAlbums: Set<Int64> = []
    var albumRevision = 0
    var theme: String { didSet { UserDefaults.standard.set(theme, forKey: "appearance") } }
    var cacheMB: Int { didSet { UserDefaults.standard.set(cacheMB, forKey: "cacheMB") } }

    init() {
        loginOrigin = UserDefaults.standard.string(forKey: "lastServerOrigin") ?? Bundle.main.object(forInfoDictionaryKey: "MomentoDefaultServerOrigin") as? String ?? ""
        theme = UserDefaults.standard.string(forKey: "appearance") ?? "system"
        cacheMB = UserDefaults.standard.object(forKey: "cacheMB") as? Int ?? 512
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 60
        configuration.timeoutIntervalForResource = 1800
        do {
            let saved = try SessionVault().read()
            session = saved
            if let saved { loginOrigin = saved.origin.absoluteString }
            api = APIClient(transport: URLSession(configuration: configuration, delegate: SameOriginDelegate(progress: nil), delegateQueue: nil), session: saved)
        } catch {
            api = APIClient(transport: URLSession(configuration: configuration, delegate: SameOriginDelegate(progress: nil), delegateQueue: nil), session: nil)
            self.error = error.localizedDescription
        }
    }

    func prepareSession() async {
        guard let session else { return }
        if backup != nil { return }
        await perform {
            let base = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
            let account = SHA256Key.value(session.origin.absoluteString + ":" + String(session.user.id))
            self.cache = try MediaCache(root: base.appending(path: "media/\(session.namespace)"), limit: self.cacheMB * 1024 * 1024)
            self.backup = try BackupController(api: self.api, root: base.appending(path: "backup/\(account)"))
            self.capabilities = try await self.api.capabilities()
        }
    }

    func login(origin: String, username: String, password: String) async {
        await perform {
            #if DEBUG
            let url = try APIClient.origin(origin, allowHTTP: true)
            #else
            let url = try APIClient.origin(origin, allowHTTP: false)
            #endif
            self.session = try await self.api.login(origin: url, username: username, password: password)
            self.loginOrigin = url.absoluteString
            UserDefaults.standard.set(self.loginOrigin, forKey: "lastServerOrigin")
        }
        if session != nil { await prepareSession() }
    }

    func perform(_ operation: () async throws -> Void) async {
        operations += 1
        defer { operations -= 1 }
        do { try await operation() }
        catch is CancellationError { }
        catch { self.error = error.localizedDescription }
    }

    func trash(_ ids: Set<Int64>) async throws {
        let _: MessageResponse = try await api.call("media/delete", MediaIdsRequest(mediaIds: Array(ids)))
        trashedMedia.formUnion(ids)
    }

    func publishAlbum(_ detail: AlbumDetail) {
        albumUpdates[detail.id] = Album(id: detail.id, name: detail.name, description: detail.description, coverMediaId: detail.coverMediaId,
            thumbnailMediaIds: Array(detail.media.prefix(4).map(\.id)), mediaCount: Int64(detail.media.count), createdAt: detail.createdAt)
        albumRevision += 1
    }

    func logout(localOnly: Bool) async {
        await perform {
            if localOnly { try await self.api.forgetSession() } else { try await self.api.signOut() }
            await self.backup?.stop()
            let previousCache = self.cache
            self.session = nil
            self.capabilities = nil
            self.cache = nil
            self.backup = nil
            self.trashedMedia = []; self.albumUpdates = [:]; self.deletedAlbums = []; self.albumRevision = 0
            try await previousCache?.clear()
        }
    }
}
