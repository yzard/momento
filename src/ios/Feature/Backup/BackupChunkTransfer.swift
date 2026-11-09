import Foundation

enum BackupChunkTransfer {
    static func upload(api: APIClient, uploadId: String, offset: Int64, total: Int64, originalHash: String,
                       bytes: Data, cellular: Bool, attempts: Int) async throws -> BackupUploadResponse {
        let end = offset + Int64(bytes.count)
        guard offset >= 0, !bytes.isEmpty, end <= total, attempts > 0 else { throw ClientError.message("Invalid backup chunk range.") }
        var latest: any Error = ClientError.message("Upload could not be confirmed.")
        for attempt in 0..<attempts {
            try Task.checkCancellation()
            do {
                let data = try await api.bytes(path: "backup/upload/chunk/\(uploadId)", method: "PUT", body: bytes, contentType: "application/octet-stream",
                    headers: ["Content-Range": "bytes \(offset)-\(end - 1)/\(total)", "X-Content-SHA256": SHA256Key.digest(bytes)], cellular: cellular)
                let result = try JSONDecoder().decode(BackupUploadResponse.self, from: data)
                try validate(result, uploadId: uploadId, total: total, originalHash: originalHash)
                guard result.uploadedSize == end else { throw ClientError.message("Server did not confirm this complete chunk.") }
                return result
            } catch {
                if error is CancellationError || (error as? URLError)?.code == .cancelled { throw CancellationError() }
                guard retryable(error) else { throw error }
                latest = error
            }
            do {
                let result: BackupUploadResponse = try await api.call("backup/upload/status", BackupUploadIdRequest(uploadId: uploadId))
                try validate(result, uploadId: uploadId, total: total, originalHash: originalHash)
                if result.uploadedSize == end { return result }
                guard result.uploadedSize == offset, ["uploading", "writing"].contains(result.status) else {
                    throw ClientError.message("Server upload changed. Resume from its durable status before sending more data.")
                }
            } catch {
                if error is CancellationError || (error as? URLError)?.code == .cancelled { throw CancellationError() }
                guard retryable(error) else { throw error }
                latest = error
            }
            if attempt + 1 < attempts { try await Task.sleep(for: .seconds(min(8, 1 << attempt))) }
        }
        throw latest
    }

    private static func validate(_ response: BackupUploadResponse, uploadId: String, total: Int64, originalHash: String) throws {
        guard response.uploadId == uploadId, response.expectedSize == total, response.contentHash == originalHash,
              response.uploadedSize >= 0, response.uploadedSize <= total else { throw ClientError.message("Server upload does not match the protected original.") }
    }
    private static func retryable(_ error: any Error) -> Bool {
        if case ClientError.http(let status, _) = error { return [408, 409, 425, 429].contains(status) || status >= 500 }
        return error is URLError
    }
}
