import Foundation

extension BackupController {
    func persist(_ original: BackupRecord) async throws {
        var record = original
        if cancellationRequested && record.state.active { record.state = .cancelling }
        try await store.save(record)
        if let index = records.firstIndex(where: { $0.id == record.id }) { records[index] = record }
    }

    func failed(_ record: BackupRecord, error: any Error) async throws {
        var record = record
        record.state = .failed
        record.error = error.localizedDescription
        try await persist(record)
        latestError = backupUploadFailure(filename: record.filename, reason: error.localizedDescription)
        if error is URLError { throw error }
        if case ClientError.http(let status, _) = error, status == 401 || status == 429 || status >= 500 { throw error }
    }

    func runBatches(device: String, capabilities: BackupCapabilities, limits: BackupBatchCapabilities) async throws {
        try await pollProcessing(capabilities: capabilities, attempts: 1)
        var group: [BackupRecord] = []
        let pending = records.filter { $0.state.active && $0.state != .serverProcessing }
        for original in pending {
            try Task.checkCancellation()
            if cancellationRequested { throw CancellationError() }
            var record = original
            if record.uploadId != nil {
                if !group.isEmpty { try await flushBatch(group, device: device, capabilities: capabilities, limits: limits); group.removeAll() }
                do { try await transfer(&record, device: device, capabilities: capabilities, preparedOriginal: false); try await persist(record) }
                catch is CancellationError { throw CancellationError() }
                catch { try await failed(record, error: error) }
                continue
            }
            do {
                try await allowedNetwork()
                status = "Preparing original \(record.filename)…"
                try await prepareOriginal(&record, capabilities: capabilities)
                try await persist(record)
            } catch is CancellationError { throw CancellationError() }
            catch { try await failed(record, error: error); continue }
            if record.state == .excluded { continue }
            let fitsBytes = backupBatchCanAppend(sizes: group.map(\.byteSize), next: record.byteSize, limits: limits)
            let manifests = try (group + [record]).map { try uploadManifest($0, device: device) }
            let fitsJSON = try JSONEncoder().encode(BackupBatchPrepareRequest(uploads: manifests)).count <= limits.maxManifestBytes
            if !group.isEmpty && (!fitsBytes || !fitsJSON) {
                try await flushBatch(group, device: device, capabilities: capabilities, limits: limits)
                group.removeAll()
            }
            let singleJSON = try JSONEncoder().encode(BackupBatchPrepareRequest(uploads: [uploadManifest(record, device: device)]))
            if record.byteSize > limits.maxBytes || singleJSON.count > limits.maxManifestBytes {
                // A large original keeps byte-level resume without staging another copy of the whole movie.
                do { try await transfer(&record, device: device, capabilities: capabilities, preparedOriginal: true); try await persist(record) }
                catch is CancellationError { throw CancellationError() }
                catch { try await failed(record, error: error) }
            } else { group.append(record) }
        }
        if !group.isEmpty { try await flushBatch(group, device: device, capabilities: capabilities, limits: limits) }
    }

    func flushBatch(_ group: [BackupRecord], device: String, capabilities: BackupCapabilities, limits: BackupBatchCapabilities) async throws {
        if cancellationRequested { throw CancellationError() }
        try await allowedNetwork()
        var current = group
        let bodyFile = await store.batchFile()
        defer { try? FileManager.default.removeItem(at: bodyFile); batchTransfer = nil }
        do {
            let request = BackupBatchPrepareRequest(uploads: try group.map { try uploadManifest($0, device: device) })
            let prepared: BackupBatchResponse = try await api.call("backup/upload/batch/prepare", request)
            let indexed = try prepared.indexed(keys: group.map(\.operationId))
            var files: [BackupBatchFile] = []
            // Persist every server ID before any bytes leave the device. An interrupted response remains resumable.
            for index in current.indices {
                let item = indexed[current[index].operationId]!
                if let error = item.error { try await failed(current[index], error: ClientError.http(error.status, error.detail)); continue }
                try current[index].accept(item.upload!)
                try await persist(current[index])
                if current[index].state == .completed { try await store.removeSnapshot(current[index]) }
                if current[index].state == .uploading && current[index].uploadedSize == 0 {
                    files.append(BackupBatchFile(recordId: current[index].id, filename: current[index].filename,
                        file: await store.snapshot(current[index]), uploadId: current[index].uploadId!,
                        byteSize: current[index].byteSize, contentHash: current[index].contentHash!))
                } else if current[index].state == .uploading {
                    try await transfer(&current[index], device: device, capabilities: capabilities, preparedOriginal: true)
                    try await persist(current[index])
                }
            }
            if cancellationRequested { throw CancellationError() }
            if !files.isEmpty {
                let originals = files
                let (events, continuation) = AsyncStream<Int64>.makeStream(bufferingPolicy: .bufferingNewest(1))
                let work = Task { @MainActor in
                    defer { continuation.finish() }
                    let payload = try await BackupBatchPayload.write(originals: originals, to: bodyFile, limits: limits)
                    let data = try await api.uploadFile(path: "backup/upload/batch", file: payload.file, length: payload.length,
                        cellular: cellular, progress: { continuation.yield($0) })
                    return try JSONDecoder().decode(BackupBatchResponse.self, from: data)
                }
                batchTransfer = work
                async let observe: Void = observeBatch(events, originals: originals)
                let response = try await withTaskCancellationHandler { try await work.value } onCancel: { work.cancel() }
                await observe
                let results = try response.indexed(keys: originals.map(\.uploadId))
                for index in current.indices {
                    guard let id = current[index].uploadId, let item = results[id] else { continue }
                    if let error = item.error { try await failed(current[index], error: ClientError.http(error.status, error.detail)); continue }
                    try current[index].accept(item.upload!)
                    try await persist(current[index])
                    if current[index].state == .completed { try await store.removeSnapshot(current[index]) }
                }
            }
            try await pollProcessing(capabilities: capabilities, attempts: 1)
        } catch is CancellationError { throw CancellationError() }
        catch {
            if cancellationRequested { throw CancellationError() }
            for record in current {
                let retained = records.first { $0.id == record.id } ?? record
                if retained.state.active && retained.state != .serverProcessing {
                    var failure = retained; failure.state = .failed; failure.error = error.localizedDescription
                    try await persist(failure)
                }
            }
            throw error
        }
    }

    private func observeBatch(_ events: AsyncStream<Int64>, originals: [BackupBatchFile]) async {
        // Reproduce framing length without reading the 50 MB staging file on the main actor.
        let manifest = BackupBatchManifest(version: 1, entries: originals.map { BackupBatchEntry(uploadId: $0.uploadId, byteSize: $0.byteSize, contentHash: $0.contentHash) })
        guard let json = try? JSONEncoder().encode(manifest) else { return }
        let progress = BackupBatchPayload(file: URL(filePath: "/"), headerBytes: Int64(4 + json.count), originals: originals)
        var last = Date.distantPast
        for await sent in events {
            guard Date().timeIntervalSince(last) >= 0.2 || sent == progress.length else { continue }
            last = .now
            if let file = progress.fileProgress(sent: sent) { status = backupFileStatus(filename: file.filename, uploaded: file.finished, total: file.total) }
        }
    }

    func pollProcessing(capabilities: BackupCapabilities, attempts: Int) async throws {
        for attempt in 0..<attempts {
            let pending = records.filter { $0.state == .serverProcessing }
            if pending.isEmpty || cancellationRequested { return }
            try await allowedNetwork()
            if let limits = capabilities.batch, limits.version == 1, limits.maxItems > 0 {
                for start in stride(from: 0, to: pending.count, by: limits.maxItems) {
                    let group = Array(pending[start..<min(start + limits.maxItems, pending.count)])
                    let ids = group.compactMap(\.uploadId)
                    let response: BackupBatchResponse = try await api.call("backup/upload/batch/status", BackupUploadIdsRequest(uploadIds: ids))
                    let results = try response.indexed(keys: ids)
                    for var record in group {
                        guard let id = record.uploadId, let item = results[id] else { continue }
                        if let error = item.error { try await failed(record, error: ClientError.http(error.status, error.detail)); continue }
                        try record.accept(item.upload!); try await persist(record)
                        if record.state == .completed { try await store.removeSnapshot(record) }
                    }
                }
            } else {
                for var record in pending {
                    try Task.checkCancellation()
                    guard let upload = record.uploadId else { continue }
                    let response: BackupUploadResponse = try await api.call("backup/upload/status", BackupUploadIdRequest(uploadId: upload))
                    try record.accept(response); try await persist(record)
                    if record.state == .completed { try await store.removeSnapshot(record) }
                }
            }
            if attempt + 1 < attempts && records.contains(where: { $0.state == .serverProcessing }) { try await Task.sleep(for: .seconds(2)) }
        }
    }
}
