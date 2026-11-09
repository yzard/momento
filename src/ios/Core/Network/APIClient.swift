import Foundation

actor APIClient {
    private let transport: URLSession
    private var session: SavedSession?
    private var refreshTask: Task<TokenPair, Error>?
    private let vault = SessionVault()

    init(transport: URLSession, session: SavedSession?) {
        self.transport = transport
        self.session = session
    }

    static func origin(_ input: String, allowHTTP: Bool) throws -> URL {
        guard let url = URL(string: input.trimmingCharacters(in: .whitespacesAndNewlines)),
              let parts = URLComponents(url: url, resolvingAgainstBaseURL: false),
              parts.scheme == "https" || (allowHTTP && parts.scheme == "http"),
              let host = parts.host, !host.isEmpty,
              parts.path.isEmpty || parts.path == "/",
              parts.user == nil, parts.password == nil, parts.query == nil, parts.fragment == nil
        else { throw ClientError.message("Enter a server origin such as https://photos.example.com, without a path or credentials.") }
        var normalized = parts
        normalized.scheme = parts.scheme?.lowercased()
        normalized.host = host.lowercased()
        normalized.path = ""
        if (normalized.scheme == "https" && normalized.port == 443) || (normalized.scheme == "http" && normalized.port == 80) { normalized.port = nil }
        guard let origin = normalized.url else { throw ClientError.message("Invalid server origin.") }
        return origin
    }

    func login(origin: URL, username: String, password: String) async throws -> SavedSession {
        let credentials = Data("\(username):\(password)".utf8).base64EncodedString()
        let tokens: TokenPair = try await publicRequest(origin: origin, path: "user/authenticate", method: "POST", body: EmptyRequest(), authorization: "Basic \(credentials)")
        let user: User = try await publicRequest(origin: origin, path: "user/get", method: "POST", body: EmptyRequest(), authorization: "Bearer \(tokens.accessToken)")
        let saved = SavedSession(origin: origin, tokens: tokens, user: user, namespace: UUID().uuidString)
        try vault.write(saved)
        session = saved
        return saved
    }

    func currentSession() -> SavedSession? { session }

    func publicRequest<Body: Encodable & Sendable, Response: Decodable & Sendable>(origin: URL, path: String, method: String, body: Body, authorization: String?) async throws -> Response {
        var request = URLRequest(url: origin.appending(path: "api/v1/" + path))
        request.httpMethod = method
        request.setValue(authorization, forHTTPHeaderField: "Authorization")
        if method != "GET" {
            request.httpBody = try JSONEncoder().encode(body)
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        let (data, response) = try await transport.data(for: request)
        try Self.validate(data, response)
        return try JSONDecoder().decode(Response.self, from: data)
    }

    func call<Body: Encodable & Sendable, Response: Decodable & Sendable>(_ path: String, _ body: Body) async throws -> Response {
        let data = try await bytes(path: path, method: "POST", body: JSONEncoder().encode(body), contentType: "application/json", headers: [:], cellular: true)
        return try JSONDecoder().decode(Response.self, from: data)
    }

    func capabilities() async throws -> Capabilities {
        let data = try await bytes(path: "client/capabilities", method: "GET", body: nil, contentType: nil, headers: [:], cellular: true)
        return try JSONDecoder().decode(Capabilities.self, from: data)
    }

    func download(path: String, destination: URL) async throws {
        guard let initial = session else { throw ClientError.http(401, "Please sign in.") }
        for attempt in 0...1 {
            guard let active = session, active.namespace == initial.namespace else { throw CancellationError() }
            var request = URLRequest(url: active.origin.appending(path: "api/v1/" + path))
            request.setValue("Bearer \(active.tokens.accessToken)", forHTTPHeaderField: "Authorization")
            let (file, response) = try await transport.download(for: request)
            defer { try? FileManager.default.removeItem(at: file) }
            guard session?.namespace == initial.namespace else { throw CancellationError() }
            if (response as? HTTPURLResponse)?.statusCode == 401 && attempt == 0 {
                try await refresh(namespace: initial.namespace, rejectedToken: active.tokens.accessToken)
                continue
            }
            if let http = response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
                let handle = try FileHandle(forReadingFrom: file)
                defer { try? handle.close() }
                try Self.validate(try handle.read(upToCount: 65536) ?? Data(), response)
            }
            try Task.checkCancellation()
            try FileManager.default.moveItem(at: file, to: destination)
            return
        }
        throw ClientError.http(401, "Please sign in again.")
    }

    func bytes(path: String, method: String, body: Data?, contentType: String?, headers: [String: String], cellular: Bool) async throws -> Data {
        try await response(path: path, method: method, payload: .data(body), contentType: contentType, headers: headers, cellular: cellular, delegate: nil).0
    }

    func uploadFile(path: String, file: URL, length: Int64, cellular: Bool, progress: @escaping @Sendable (Int64) -> Void) async throws -> Data {
        try await response(path: path, method: "POST", payload: .file(file), contentType: "application/octet-stream",
            headers: ["Content-Length": String(length)], cellular: cellular, delegate: SameOriginDelegate(progress: progress)).0
    }

    private enum Payload: Sendable { case data(Data?); case file(URL) }

    func mediaRange(path: String, offset: Int64, length: Int) async throws -> MediaByteRange {
        guard offset >= 0, length > 0, length <= 1024 * 1024 else { throw ClientError.message("Invalid media byte range.") }
        let (data, http) = try await response(path: path, method: "GET", payload: .data(nil), contentType: nil,
            headers: ["Range": "bytes=\(offset)-\(offset + Int64(length) - 1)"], cellular: true, delegate: nil)
        guard http.statusCode == 206, let contentRange = http.value(forHTTPHeaderField: "Content-Range"),
              contentRange.hasPrefix("bytes \(offset)-"), let totalText = contentRange.split(separator: "/").last,
              let total = Int64(totalText), total > offset, !data.isEmpty, data.count <= length,
              contentRange == "bytes \(offset)-\(offset + Int64(data.count) - 1)/\(total)",
              offset + Int64(data.count) <= total else {
            throw ClientError.message("Server did not return a valid media byte range.")
        }
        return MediaByteRange(data: data, total: total, mimeType: http.mimeType ?? "application/octet-stream")
    }

    private func response(path: String, method: String, payload: Payload, contentType: String?, headers: [String: String], cellular: Bool,
                          delegate: (any URLSessionTaskDelegate)?) async throws -> (Data, HTTPURLResponse) {
        guard let initial = session else { throw ClientError.http(401, "Please sign in.") }
        for attempt in 0...1 {
            try Task.checkCancellation()
            guard let active = session, active.namespace == initial.namespace else { throw CancellationError() }
            var request = URLRequest(url: active.origin.appending(path: "api/v1/" + path))
            request.httpMethod = method
            if path.hasPrefix("backup/") { request.timeoutInterval = 120 }
            if case .data(let body) = payload { request.httpBody = body }
            request.allowsCellularAccess = cellular
            request.setValue(contentType, forHTTPHeaderField: "Content-Type")
            request.setValue("Bearer \(active.tokens.accessToken)", forHTTPHeaderField: "Authorization")
            for (key, value) in headers { request.setValue(value, forHTTPHeaderField: key) }
            let data: Data
            let response: URLResponse
            switch payload {
            case .data: (data, response) = try await transport.data(for: request)
            case .file(let file): (data, response) = try await transport.upload(for: request, fromFile: file, delegate: delegate)
            }
            guard session?.namespace == initial.namespace else { throw CancellationError() }
            if (response as? HTTPURLResponse)?.statusCode == 401 && attempt == 0 {
                try await refresh(namespace: initial.namespace, rejectedToken: active.tokens.accessToken)
                continue
            }
            try Self.validate(data, response)
            guard let http = response as? HTTPURLResponse else { throw ClientError.message("Invalid HTTP response.") }
            return (data, http)
        }
        throw ClientError.http(401, "Session expired. Please sign in again.")
    }

    private func refresh(namespace: String, rejectedToken: String) async throws {
        guard let active = session, active.namespace == namespace else { throw CancellationError() }
        if active.tokens.accessToken != rejectedToken { return }
        let pending: Task<TokenPair, Error>
        if let refreshTask { pending = refreshTask }
        else {
            pending = Task {
                try await self.publicRequest(origin: active.origin, path: "user/refresh", method: "POST", body: RefreshTokenRequest(refreshToken: active.tokens.refreshToken), authorization: nil)
            }
            refreshTask = pending
        }
        defer { refreshTask = nil }
        let tokens = try await pending.value
        guard var saved = session, saved.namespace == namespace else { throw CancellationError() }
        saved.tokens = tokens
        try vault.write(saved)
        session = saved
    }

    func updateUser(_ user: User) throws {
        guard var saved = session else { return }
        saved.user = user
        try vault.write(saved)
        session = saved
    }

    func signOut() async throws {
        if let active = session {
            let _: MessageResponse = try await call("user/logout", LogoutRequest(refreshToken: active.tokens.refreshToken))
        }
        try forgetSession()
    }

    func forgetSession() throws {
        try vault.clear()
        refreshTask?.cancel()
        refreshTask = nil
        session = nil
    }

    static func validate(_ data: Data, _ response: URLResponse) throws {
        guard let http = response as? HTTPURLResponse else { throw ClientError.message("Invalid server response.") }
        guard (200..<300).contains(http.statusCode) else {
            struct Detail: Decodable { let detail: String }
            let detail = (try? JSONDecoder().decode(Detail.self, from: data))?.detail
            throw ClientError.http(http.statusCode, detail ?? "Server request failed (\(http.statusCode)).")
        }
    }
}

struct MediaByteRange: Sendable {
    let data: Data
    let total: Int64
    let mimeType: String
}
