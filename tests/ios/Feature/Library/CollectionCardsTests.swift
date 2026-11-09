import Foundation
import Testing
@testable import Momento

@MainActor struct CollectionPolicyTests {
    @Test func keepsAtLeastOneOriginalInEveryDuplicateGroup() throws {
        let base = try JSONDecoder().decode(Media.self, from: Data("""
        {"id":1,"filename":"a.jpg","originalFilename":"a.jpg","mediaType":"image","createdAt":"2026-01-01"}
        """.utf8))
        let items = (1...4).map { id in var item = base; item.id = Int64(id); return item }
        let groups = [DeduplicateGroup(clusterId: 1, items: Array(items.prefix(2))), DeduplicateGroup(clusterId: 2, items: Array(items.suffix(2)))]
        #expect(CollectionPolicy.keepsOriginalInEveryGroup(groups, selected: [1, 3]))
        #expect(!CollectionPolicy.keepsOriginalInEveryGroup(groups, selected: [1, 2]))
        #expect(!CollectionPolicy.keepsOriginalInEveryGroup(groups, selected: [1, 3, 4]))
        #expect(CollectionPolicy.keepsOriginalInEveryGroup(groups, selected: []))
    }

    @Test func trashRetentionRoundsUpAndStopsAtZero() throws {
        let now = try #require(ISO8601DateFormatter().date(from: "2026-02-01T00:00:00Z"))
        #expect(CollectionPolicy.trashDaysRemaining(deletedAt: "2026-01-03T12:00:00Z", now: now) == 2)
        #expect(CollectionPolicy.trashDaysRemaining(deletedAt: "2025-01-01T00:00:00Z", now: now) == 0)
        #expect(CollectionPolicy.trashDaysRemaining(deletedAt: "bad date", now: now) == nil)
    }

    @Test func foldAndWideWindowColumnCountsMatchAndroid() {
        #expect(MediaGrid.columns(width: 350) == 2)
        #expect(MediaGrid.columns(width: 466) == 3)
        #expect(MediaGrid.columns(width: 669) == 5)
        #expect(MediaGrid.columns(width: 951) == 6)
        #expect(CollectionPolicy.placeColumns(width: 466) == 2)
        #expect(CollectionPolicy.placeColumns(width: 669) == 3)
        #expect(CollectionPolicy.comparisonColumns(width: 951) == 4)
    }
}
