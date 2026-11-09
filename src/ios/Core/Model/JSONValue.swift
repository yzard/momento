import Foundation

enum JSONValue: Codable, Sendable, Equatable {
    case string(String), number(Double), bool(Bool), object([String: JSONValue]), array([JSONValue]), null

    init(from decoder: any Decoder) throws {
        let value = try decoder.singleValueContainer()
        if value.decodeNil() { self = .null }
        else if let bool = try? value.decode(Bool.self) { self = .bool(bool) }
        else if let number = try? value.decode(Double.self) { self = .number(number) }
        else if let string = try? value.decode(String.self) { self = .string(string) }
        else if let array = try? value.decode([JSONValue].self) { self = .array(array) }
        else { self = .object(try value.decode([String: JSONValue].self)) }
    }

    func encode(to encoder: any Encoder) throws {
        var value = encoder.singleValueContainer()
        switch self {
        case .string(let v): try value.encode(v)
        case .number(let v): try value.encode(v)
        case .bool(let v): try value.encode(v)
        case .object(let v): try value.encode(v)
        case .array(let v): try value.encode(v)
        case .null: try value.encodeNil()
        }
    }
}

struct EmptyRequest: Codable, Sendable {}
extension Media: Identifiable {}
extension Album: Identifiable {}
extension User: Identifiable {}
extension TrashMedia: Identifiable {}
extension Place: Identifiable { var id: String { placeId } }
extension FaceGroup: Identifiable { var id: Int64 { faceGroupId } }
extension FaceDetection: Identifiable { var id: Int64 { faceId } }
extension DeduplicateGroup: Identifiable { var id: Int64 { clusterId } }
extension MapCluster: Identifiable {}

enum ClientError: LocalizedError {
    case message(String)
    case http(Int, String)
    var errorDescription: String? {
        switch self {
        case .message(let message), .http(_, let message): message
        }
    }
}
