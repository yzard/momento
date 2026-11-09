import Foundation

enum AdminPolicy {
    static func activeAI(_ state: String) -> Bool { ["queued", "submitting", "submitted", "running", "cancelling"].contains(state) }
    static func validCron(_ fields: [String]) -> Bool {
        fields.count == 5 && fields.allSatisfy {
            let value = $0.trimmingCharacters(in: .whitespacesAndNewlines)
            return !value.isEmpty && !value.contains(where: \.isWhitespace)
        }
    }
    static func canChangeRole(_ user: User, currentUserId: Int64?) -> Bool { user.id != currentUserId }
    static func canDeactivateOrDelete(_ user: User, currentUserId: Int64?) -> Bool { !user.isReserved && user.id != currentUserId }
}
