import SwiftUI
import Photos
import Network
import BackgroundTasks
import ImageIO

@MainActor @Observable final class BackupController {
    static let taskIdentifier = "io.github.yzard.momento.ios.backup"
    let api: APIClient
    let store: BackupStore
    var records: [BackupRecord] = []
    var running = false
    var status = "No backup running"
    var latestError: String?
    var nextScheduled: Date?
    var cellular: Bool { didSet { UserDefaults.standard.set(cellular, forKey: "backupCellular") } }
    var cameraOnly: Bool { didSet { UserDefaults.standard.set(cameraOnly, forKey: "backupCameraOnly") } }
    var daily: Bool { didSet { UserDefaults.standard.set(daily, forKey: "backupDaily"); schedule() } }
    private var task: Task<Void, Never>?
    var batchTransfer: Task<BackupBatchResponse, Error>?
    var cancellationRequested = false
    private var stopped = false

    init(api: APIClient, root: URL) throws {
        self.api = api
        store = try BackupStore(root: root)
        cellular = UserDefaults.standard.bool(forKey: "backupCellular")
        cameraOnly = UserDefaults.standard.object(forKey: "backupCameraOnly") as? Bool ?? true
        daily = UserDefaults.standard.bool(forKey: "backupDaily")
        nextScheduled = UserDefaults.standard.object(forKey: "backupNextScheduled") as? Date
    }

    var completed: Int { records.filter { $0.state == .completed }.count }
    var total: Int { records.filter { $0.state != .excluded }.count }
    var uploadErrors: [String] {
        var messages = records.filter { $0.state == .failed || $0.state == .terminalFailed }.map { record in
            backupUploadFailure(filename: record.filename, reason: record.error ?? "Unknown upload error")
        }
        if let latestError, !messages.contains(latestError) { messages.append(latestError) }
        return messages
    }
    var verified: Int { records.filter { $0.state == .completed && $0.contentHash != nil && $0.contentHash == $0.serverHash }.count }
    var canClear: Bool { !running && !records.contains(where: { $0.state.active }) && !records.isEmpty }
    var cancelling: Bool { cancellationRequested }

    func reload() async throws { records = try await store.records() }
    func start() {
        _ = launch(requestPermission: true)
    }
    private func launch(requestPermission: Bool) -> Task<Void, Never>? {
        guard !stopped, task == nil, !running else { return nil }
        let work = Task {
            await run(requestPermission: requestPermission)
            task = nil
        }
        task = work
        return work
    }
    func stop() async {
        stopped = true
        task?.cancel(); batchTransfer?.cancel(); await task?.value
        BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: Self.taskIdentifier)
        nextScheduled = nil; UserDefaults.standard.removeObject(forKey: "backupNextScheduled")
    }
    func cancel() {
        guard running, !cancellationRequested else { return }
        cancellationRequested = true
        batchTransfer?.cancel()
        Task {
            do {
                try await store.requestCancellation()
                try await reload()
                if !running { _ = launch(requestPermission: false) }
            } catch { latestError = error.localizedDescription }
        }
    }

    func schedule() {
        guard !stopped else { return }
        BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: Self.taskIdentifier)
        guard daily else { nextScheduled = nil; UserDefaults.standard.removeObject(forKey: "backupNextScheduled"); return }
        let request = BGProcessingTaskRequest(identifier: Self.taskIdentifier)
        request.requiresNetworkConnectivity = true
        request.requiresExternalPower = false
        let proposed = Date().addingTimeInterval(records.contains(where: { $0.state.active }) ? 15 * 60 : 24 * 60 * 60)
        request.earliestBeginDate = nextScheduled.flatMap { $0 > .now ? min($0, proposed) : nil } ?? proposed
        do {
            try BGTaskScheduler.shared.submit(request)
            nextScheduled = request.earliestBeginDate
            UserDefaults.standard.set(nextScheduled, forKey: "backupNextScheduled")
        }
        catch { nextScheduled = nil; UserDefaults.standard.removeObject(forKey: "backupNextScheduled"); latestError = "Could not schedule background backup: \(error.localizedDescription)" }
    }
    func runScheduled() async {
        nextScheduled = nil
        guard let work = launch(requestPermission: false) else { schedule(); return }
        await withTaskCancellationHandler {
            await work.value
        } onCancel: {
            work.cancel()
        }
        schedule()
    }

    func run(requestPermission: Bool) async {
        guard !stopped, !running else { return }
        running = true
        latestError = nil
        var failedTransferMessage: String?
        defer { running = false }
        do {
            try await store.cleanBatchFiles()
            try await reload()
            if cancellationRequested || records.contains(where: { $0.state == .cancelling }) {
                try await cancelPending()
                cancellationRequested = false
                status = "Backup cancelled"
                return
            }
            for record in duplicateOriginalUpdates(records) {
                try await persist(record)
                try await store.removeSnapshot(record)
            }
            if requestPermission { _ = await PHPhotoLibrary.requestAuthorization(for: .readWrite) }
            try await allowedNetwork()
            let caps = try await api.capabilities()
            guard caps.backup.enabled && caps.backup.protocolVersion == 2 && caps.backup.maxChunkBytes > 0 else {
                throw ClientError.message("This server does not support lossless backup protocol 2.")
            }
            let defaults = UserDefaults.standard
            let device = defaults.string(forKey: "backupDevice") ?? UUID().uuidString
            defaults.set(device, forKey: "backupDevice")
            let _: BackupDeviceRegisterResponse = try await api.call("backup/device/register", BackupDeviceRegisterRequest(deviceId: device, deviceName: UIDevice.current.name))
            status = "Scanning original photos and videos…"
            let newRecords = try await PhotoLibrary.scan(existing: records, supportedExtensions: caps.supportedMediaExtensions)
            for record in newRecords { try await store.save(record) }
            if !cameraOnly {
                for var record in records where record.state == .excluded && record.duplicateOriginalOf == nil { record.state = .queued; try await store.save(record) }
            }
            try await reload()
            if let batch = caps.backup.batch, batch.version == 1, batch.maxBytes > 0, batch.maxItems > 0, batch.maxManifestBytes > 0 {
                try await runBatches(device: device, capabilities: caps.backup, limits: batch)
            } else {
            for original in records where original.state.active {
                try Task.checkCancellation()
                if cancellationRequested { try await cancelPending(); cancellationRequested = false; break }
                var record = original
                do {
                    try await allowedNetwork()
                    status = backupFileStatus(filename: record.filename, uploaded: record.uploadedSize, total: record.byteSize)
                    try await transfer(&record, device: device, capabilities: caps.backup, preparedOriginal: false)
                } catch is CancellationError { throw CancellationError() }
                catch {
                    record.state = .failed
                    record.error = error.localizedDescription
                    latestError = backupUploadFailure(filename: record.filename, reason: error.localizedDescription)
                    try await store.save(record)
                    if let index = records.firstIndex(where: { $0.id == record.id }) { records[index] = record }
                    if error is URLError { failedTransferMessage = latestError; throw error }
                    if case ClientError.http(let status, _) = error, status == 401 || status == 429 || status >= 500 {
                        failedTransferMessage = latestError; throw error
                    }
                }
                try await reload()
            }
            }
            try await pollProcessing(capabilities: caps.backup, attempts: 10)
            if cancellationRequested { try await cancelPending(); cancellationRequested = false }
            status = "\(completed)/\(total) originals verified"
        } catch is CancellationError {
            if cancellationRequested && !Task.isCancelled {
                do { try await cancelPending(); cancellationRequested = false; status = "Backup cancelled" }
                catch { latestError = error.localizedDescription; status = "Cancellation waiting — retry available" }
            } else { status = "Paused — progress saved" }
        }
        catch { latestError = failedTransferMessage ?? error.localizedDescription; status = "Backup waiting — retry available" }
    }

    func transfer(_ record: inout BackupRecord, device: String, capabilities: BackupCapabilities, preparedOriginal: Bool) async throws {
        let file = await store.snapshot(record)
        if record.uploadId == nil {
            if !preparedOriginal { try await prepareOriginal(&record, capabilities: capabilities) }
            if record.state == .excluded { return }
            let response: BackupUploadResponse = try await api.call("backup/upload/create", uploadManifest(record, device: device))
            try record.accept(response)
            try await store.save(record)
        } else if let upload = record.uploadId {
            let response: BackupUploadResponse = try await api.call("backup/upload/status", BackupUploadIdRequest(uploadId: upload))
            try record.accept(response)
            if ["expired", "cancelled"].contains(response.status) {
                if response.status == "expired" {
                    let cancelled: BackupUploadResponse = try await api.call("backup/upload/cancel", BackupUploadIdRequest(uploadId: upload))
                    try record.accept(cancelled)
                    guard cancelled.status == "cancelled" else { throw ClientError.message("Previous upload has not stopped yet.") }
                }
                let previous = record
                record.uploadId = nil; record.operationId = UUID().uuidString; record.uploadedSize = 0; record.state = .queued
                let replacement = await store.snapshot(record)
                if FileManager.default.fileExists(atPath: file.path) { try FileManager.default.copyItem(at: file, to: replacement) }
                try await store.save(record)
                try await store.removeSnapshot(previous)
                return
            }
            try await store.save(record)
        }
        guard let upload = record.uploadId else { return }
        if record.state == .completed { try await store.removeSnapshot(record); return }
        if [.serverProcessing, .terminalFailed, .cancelled, .cancelling].contains(record.state) { return }
        let (snapshotHash, snapshotSize) = try await SHA256Key.file(file)
        guard snapshotHash == record.contentHash && snapshotSize == record.byteSize else { throw ClientError.message("Protected original changed. Upload stopped to prevent corruption.") }
        let handle = try FileHandle(forReadingFrom: file)
        defer { try? handle.close() }
        let chunkSize = Int(min(capabilities.maxChunkBytes, 8 * 1024 * 1024))
        status = backupFileStatus(filename: record.filename, uploaded: record.uploadedSize, total: record.byteSize)
        while record.uploadedSize < record.byteSize {
            try Task.checkCancellation()
            if cancellationRequested { record.state = .cancelling; try await store.save(record); return }
            try await allowedNetwork()
            try handle.seek(toOffset: UInt64(record.uploadedSize))
            guard let chunk = try handle.read(upToCount: Int(min(Int64(chunkSize), record.byteSize - record.uploadedSize))), !chunk.isEmpty else {
                throw ClientError.message("Protected original snapshot is truncated.")
            }
            let expected = record.uploadedSize + Int64(chunk.count)
            guard let hash = record.contentHash else { throw ClientError.message("Original hash is missing.") }
            let response = try await BackupChunkTransfer.upload(api: api, uploadId: upload, offset: record.uploadedSize, total: record.byteSize,
                originalHash: hash, bytes: chunk, cellular: cellular, attempts: 3)
            try record.accept(response)
            guard record.uploadedSize == expected else { throw ClientError.message("Server did not acknowledge the complete chunk; retry will reconcile its offset.") }
            try await store.save(record)
            status = backupFileStatus(filename: record.filename, uploaded: record.uploadedSize, total: record.byteSize)
        }
        record.state = .completing
        try await store.save(record)
        let response: BackupUploadResponse = try await api.call("backup/upload/complete", BackupUploadIdRequest(uploadId: upload))
        try record.accept(response)
        try await store.save(record)
        if record.state == .completed { try await store.removeSnapshot(record) }
    }

    private func cancelPending() async throws {
        for var record in try await store.records() where record.state.active {
            record.state = .cancelling
            try await store.save(record)
            if let upload = record.uploadId {
                let response: BackupUploadResponse
                do { response = try await api.call("backup/upload/cancel", BackupUploadIdRequest(uploadId: upload)) }
                catch ClientError.http(409, _) { response = try await api.call("backup/upload/status", BackupUploadIdRequest(uploadId: upload)) }
                try record.accept(response)
                if record.state.active { record.state = .cancelling }
            } else { record.state = .cancelled }
            try await store.save(record)
            if !record.state.active { try await store.removeSnapshot(record) }
        }
        try await reload()
    }

    func clearHistory() async throws { guard !running else { return }; try await store.clear(); try await reload() }
    func repairHistory() async throws {
        guard !running else { return }
        for var record in records where record.state == .completed && (record.contentHash == nil || record.contentHash != record.serverHash) {
            record.state = .queued; record.uploadId = nil; record.operationId = UUID().uuidString; record.uploadedSize = 0
            try await store.save(record)
        }
        try await reload()
    }
    var diagnostics: String {
        (["Momento iOS backup protocol 2", status, latestError ?? "", "Verified: \(verified)/\(completed)"] + records.map {
            "\($0.filename): \($0.state.rawValue) \($0.uploadedSize)/\($0.byteSize) \($0.duplicateOriginalOf.map { "Duplicate discovery record; original tracked by " + $0 } ?? ($0.state == .excluded ? "" : ($0.error ?? "")))"
        }).joined(separator: "\n")
    }

    func allowedNetwork() async throws {
        let monitor = NWPathMonitor()
        let path = await withCheckedContinuation { continuation in
            monitor.pathUpdateHandler = { path in monitor.cancel(); continuation.resume(returning: path) }
            monitor.start(queue: DispatchQueue(label: "momento.backup.network"))
        }
        guard path.status == .satisfied, cellular || (!path.isExpensive && !path.isConstrained) else {
            throw ClientError.message("Waiting for Wi-Fi. Enable mobile data to back up on this connection.")
        }
    }
}

extension BackupController {
    func prepareOriginal(_ record: inout BackupRecord, capabilities: BackupCapabilities) async throws {
        let file = await store.snapshot(record)
            try await PhotoLibrary.export(record, to: file, cellular: cellular)
            if record.cameraCapture == nil { record.cameraCapture = try await PhotoLibrary.cameraOriginal(file, mimeType: record.mimeType) }
            if cameraOnly && record.cameraCapture == false {
                record.state = .excluded
                record.error = nil
                try await store.save(record)
                try await store.removeSnapshot(record)
                return
            }
            let (hash, size) = try await SHA256Key.file(file)
            guard size > 0 && size <= capabilities.maxUploadBytes else { throw ClientError.message("Original exceeds the server upload limit.") }
            record.byteSize = size
            record.contentHash = hash
            record.metadata["momentoBackup"] = .object([
                "schemaVersion": .number(2), "source": .string("iosPhotoKit"), "contentHash": .string(hash),
                "byteSize": .number(Double(size)), "originalFilename": .string(record.filename),
                "sourceModifiedAt": .string(record.modifiedAt), "capturedAt": .string(Date().ISO8601Format())
            ])
            try await store.save(record)
    }
    func uploadManifest(_ record: BackupRecord, device: String) throws -> BackupUploadCreateRequest {
        guard let hash = record.contentHash else { throw ClientError.message("Original hash is missing.") }
        return BackupUploadCreateRequest(protocolVersion: 2, deviceId: device,
            clientAssetId: SHA256Key.value(record.id + ":" + record.operationId), operationId: record.operationId,
            originalFilename: record.filename, mimeType: record.mimeType, byteSize: record.byteSize,
            contentHash: hash, sourceModifiedAt: record.modifiedAt, metadata: record.metadata)
    }
}
