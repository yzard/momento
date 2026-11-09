import Foundation
import Security

struct SavedSession: Codable, Sendable {
    var origin: URL
    var tokens: TokenPair
    var user: User
    var namespace: String
}

struct SessionVault {
    private let service = "io.github.yzard.momento.ios.session"

    func read() throws -> SavedSession? {
        var item: CFTypeRef?
        let status = SecItemCopyMatching([
            kSecClass: kSecClassGenericPassword, kSecAttrService: service,
            kSecReturnData: true, kSecMatchLimit: kSecMatchLimitOne
        ] as CFDictionary, &item)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = item as? Data else { throw failure(status) }
        return try JSONDecoder().decode(SavedSession.self, from: data)
    }

    func write(_ session: SavedSession) throws {
        let data = try JSONEncoder().encode(session)
        let query = [kSecClass: kSecClassGenericPassword, kSecAttrService: service] as CFDictionary
        let status = SecItemUpdate(query, [kSecValueData: data] as CFDictionary)
        if status == errSecItemNotFound {
            let added = SecItemAdd([
                kSecClass: kSecClassGenericPassword, kSecAttrService: service,
                kSecValueData: data, kSecAttrAccessible: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
            ] as CFDictionary, nil)
            guard added == errSecSuccess else { throw failure(added) }
        } else if status != errSecSuccess { throw failure(status) }
    }

    func clear() throws {
        let status = SecItemDelete([kSecClass: kSecClassGenericPassword, kSecAttrService: service] as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw failure(status) }
    }

    private func failure(_ status: OSStatus) -> ClientError {
        .message("Secure storage failed (\(status)). Unlock your device and try again.")
    }
}
