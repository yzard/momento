package io.github.yzard.momento.core.model

import kotlinx.serialization.json.Json
import kotlinx.serialization.decodeFromString
import org.junit.Assert.assertEquals
import org.junit.Test

class DtoMappingTest {
    @Test fun decodesBackendCamelCaseMediaDto() {
        val media = Json.decodeFromString<Media>("{\"id\":7,\"filename\":\"stored.jpg\",\"originalFilename\":\"IMG_0007.jpg\",\"mediaType\":\"image\",\"cameraMake\":\"Fuji\",\"focalLength35mm\":35.0,\"keywords\":\"coast, sunset\",\"contentHash\":\"abc123\",\"createdAt\":\"2026-01-01T00:00:00Z\"}")
        assertEquals("IMG_0007.jpg", media.originalFilename)
        assertEquals("Fuji", media.cameraMake)
        assertEquals(35.0, media.focalLength35mm)
        assertEquals("coast, sunset", media.keywords)
        assertEquals("abc123", media.contentHash)
    }

    @Test fun decodesTimelineSearchContract() {
        val response = Json.decodeFromString<TimelineResponse>("{\"groups\":[{\"date\":\"2026-01-01\",\"media\":[{\"id\":7,\"filename\":\"stored.jpg\",\"originalFilename\":\"IMG_0007.jpg\",\"mediaType\":\"image\",\"createdAt\":\"2026-01-01T00:00:00Z\"}]}],\"nextCursor\":null,\"previousCursor\":null,\"hasOlder\":false,\"hasNewer\":false}")

        assertEquals(listOf(7L), response.groups.single().media.map { it.id })
    }

    @Test fun decodesBackupUploadContract() {
        val response = Json.decodeFromString<BackupUploadResponse>("{\"uploadId\":\"abc\",\"status\":\"uploading\",\"uploadedSize\":12,\"expectedSize\":42,\"mediaId\":null,\"error\":null}")
        assertEquals(12, response.uploadedSize)
    }

    @Test fun decodesCapabilitiesContract() {
        val capabilities = Json.decodeFromString<Capabilities>("{\"appVersion\":\"1.0\",\"apiVersion\":1,\"supportedMediaExtensions\":[\".jpg\"],\"features\":{\"llm\":false,\"imageTagging\":false,\"deduplicate\":true,\"faceDetection\":false,\"imageAesthetics\":false,\"screenshotDetection\":false,\"documentDetection\":false},\"backup\":{\"enabled\":true,\"maxUploadBytes\":100,\"maxChunkBytes\":10,\"maxActiveUploadsPerUser\":2,\"sessionExpiryHours\":24}}")
        assertEquals(10, capabilities.backup.maxChunkBytes)
    }

    @Test fun decodesFaceMergeResponseContract() {
        val response = Json.decodeFromString<FaceMergeResponse>("{\"group\":{\"faceGroupId\":4,\"faceCount\":8,\"mediaCount\":3}}")
        assertEquals(4, response.group.faceGroupId)
    }

    @Test fun encodesAlbumUpdateAndDecodesJobStatusContracts() {
        val request = AlbumUpdateRequest(3, "Summer", "Trip", 9)
        assertEquals("{\"albumId\":3,\"name\":\"Summer\",\"description\":\"Trip\",\"coverMediaId\":9}", Json.encodeToString(AlbumUpdateRequest.serializer(), request))
        val status = Json.decodeFromString<JobStatus>("{\"status\":\"queued\",\"queuedJobs\":2,\"processingJobs\":1,\"completedJobs\":3,\"failedJobs\":0,\"errors\":[]}")
        assertEquals(2, status.queuedJobs)
    }

    @Test fun decodesDeduplicateAndImportContracts() {
        val duplicate = Json.decodeFromString<DeduplicateStatusResponse>("{\"status\":\"running\",\"runId\":1,\"trigger\":\"manual\",\"scheduledFor\":null,\"startedAt\":null,\"completedAt\":null,\"ensembledMedia\":1,\"processedMedia\":0,\"candidateComparisons\":0,\"clustersCreated\":0,\"error\":null,\"nextScheduledAt\":null}")
        assertEquals("running", duplicate.status)
    }

    @Test fun decodesTrashMediaMetadataContract() {
        val trash = Json.decodeFromString<TrashResponse>("{\"items\":[{\"id\":8,\"filename\":\"8.jpg\",\"originalFilename\":\"photo.jpg\",\"mediaType\":\"image\",\"mimeType\":\"image/jpeg\",\"width\":1200,\"height\":800,\"fileSize\":2048,\"durationSeconds\":null,\"dateTaken\":\"2026-08-23T10:30:00\",\"deletedAt\":\"2026-08-24T10:30:00Z\",\"createdAt\":\"2026-08-23T10:30:00Z\"}],\"totalCount\":1}")

        assertEquals(1200, trash.items.single().width)
        assertEquals("2026-08-23T10:30:00", trash.items.single().dateTaken)
    }

    @Test fun decodesPlaceThumbnailContract() {
        val response = Json.decodeFromString<PlaceThumbnailResponse>("{\"thumbnail\":\"data:image/jpeg;base64,AQID\"}")
        assertEquals("data:image/jpeg;base64,AQID", response.thumbnail)
    }

    @Test fun encodesMapBoundsAndAlbumMediaRequestContracts() {
        val map = MapClustersRequest(BoundingBox(2.0, 1.0, 4.0, 3.0), 12)
        assertEquals("{\"bounds\":{\"north\":2.0,\"south\":1.0,\"east\":4.0,\"west\":3.0},\"zoom\":12}", Json.encodeToString(MapClustersRequest.serializer(), map))
        assertEquals("{\"albumId\":5,\"mediaIds\":[7,8]}", Json.encodeToString(AlbumMediaRequest.serializer(), AlbumMediaRequest(5, listOf(7, 8))))
    }
}
