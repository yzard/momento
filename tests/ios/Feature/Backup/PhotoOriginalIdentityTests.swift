import Foundation
import Photos
import Testing
@testable import Momento

struct PhotoOriginalIdentityTests {
    func record() -> BackupRecord {
        var record = BackupTests().record()
        record.metadata["photoKit"] = .object(["resourceType": .number(Double(PHAssetResourceType.photo.rawValue)), "uniformTypeIdentifier": .string("public.jpeg")])
        return record
    }
    let original = PhotoOriginalResource(type: PHAssetResourceType.photo.rawValue, filename: "original.jpg", uniformType: "public.jpeg")
    let edited = PhotoOriginalResource(type: PHAssetResourceType.fullSizePhoto.rawValue, filename: "original.jpg", uniformType: "public.jpeg")

    @Test func reorderedResourcesAndChangedAssetMetadataStillLocateTheUneditedOriginal() throws {
        var item = record(); item.resourceIndex = 99; item.modifiedAt = "2000-01-01T00:00:00Z"
        #expect(try PhotoOriginalResource.select(for: item, from: [edited, original]) == 1)
        #expect(try PhotoOriginalResource.select(for: item, from: [original, edited]) == 0)
    }
    @Test func editedOnlyMissingChangedTypeAndAmbiguousResourcesNeverSubstituteForAnOriginal() {
        let item = record()
        for resources in [[edited], [], [original, original], [PhotoOriginalResource(type: 1, filename: "other.jpg", uniformType: "public.jpeg")],
            [PhotoOriginalResource(type: 1, filename: "original.jpg", uniformType: "public.heic")]] {
            #expect(throws: (any Error).self) { try PhotoOriginalResource.select(for: item, from: resources) }
        }
    }
    @Test func persistedRecordsWithoutUTIStillRequireAnUneditedResourceOfTheSameType() throws {
        var item = record(); item.metadata["photoKit"] = .object(["resourceType": .number(1)])
        #expect(try PhotoOriginalResource.select(for: item, from: [edited, original]) == 1)
        let alternate = PhotoOriginalResource(type: 4, filename: "original.jpg", uniformType: "public.jpeg")
        #expect(throws: (any Error).self) { try PhotoOriginalResource.select(for: item, from: [alternate]) }
    }
    @Test func duplicateMetadataVersionsPreserveVerifiedHistoryAndExcludeOnlyUnpreparedTrackingEntries() {
        var verified = record(); verified.state = .completed; verified.serverHash = verified.contentHash
        var queued = verified; queued.id = "new-discovery"; queued.operationId = "new-operation"; queued.uploadId = nil
        queued.state = .queued; queued.byteSize = 0; queued.uploadedSize = 0; queued.contentHash = nil; queued.serverHash = nil
        queued.modifiedAt = "2026-11-07T01:02:03Z"
        let updates = duplicateOriginalUpdates([queued, verified])
        #expect(updates.count == 1); #expect(updates.first?.id == queued.id)
        #expect(updates.first?.state == .excluded); #expect(updates.first?.duplicateOriginalOf == verified.id)
        #expect(duplicateOriginalUpdates([verified] + updates).isEmpty)
        #expect(verified.uploadId == "upload"); #expect(verified.state == .completed)
    }
    @Test func duplicatesNeverCancelOrReplaceAnInflightOrHashedOriginal() {
        var first = record(); first.state = .uploading
        var second = first; second.id = "other"; second.uploadId = "other-upload"
        #expect(duplicateOriginalUpdates([first, second]).isEmpty)
        second.uploadId = nil
        #expect(duplicateOriginalUpdates([first, second]).isEmpty)
        second.contentHash = nil; second.byteSize = 0; second.state = .queued
        #expect(duplicateOriginalUpdates([first, second]).single?.duplicateOriginalOf == first.id)
    }
}
private extension Array {
    var single: Element? { count == 1 ? first : nil }
}
