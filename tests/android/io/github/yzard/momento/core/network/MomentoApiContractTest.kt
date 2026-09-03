package io.github.yzard.momento.core.network

import io.github.yzard.momento.core.model.BackupUploadIdRequest
import io.github.yzard.momento.core.model.AlbumMediaRequest
import io.github.yzard.momento.core.model.AlbumCreateRequest
import io.github.yzard.momento.core.model.AlbumIdRequest
import io.github.yzard.momento.core.model.AlbumUpdateRequest
import io.github.yzard.momento.core.model.MapClustersRequest
import io.github.yzard.momento.core.model.EmptyRequest
import io.github.yzard.momento.core.model.AiScheduleUpdateRequest
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Test
import retrofit2.http.GET
import retrofit2.http.POST

class MomentoApiContractTest {
    @Test fun uploadStatusUsesTheBodyBasedBackendRoute() {
        val method = MomentoApi::class.java.getMethod("uploadStatus", BackupUploadIdRequest::class.java, kotlin.coroutines.Continuation::class.java)
        val cancel = MomentoApi::class.java.getMethod("cancelUpload", BackupUploadIdRequest::class.java, kotlin.coroutines.Continuation::class.java)
        assertEquals("api/v1/backup/upload/status", requireNotNull(method.getAnnotation(POST::class.java)).value)
        assertEquals("api/v1/backup/upload/cancel", requireNotNull(cancel.getAnnotation(POST::class.java)).value)
    }

    @Test fun albumAndMapUseTheServerContracts() {
        val create = MomentoApi::class.java.getMethod("createAlbum", AlbumCreateRequest::class.java, kotlin.coroutines.Continuation::class.java)
        val list = MomentoApi::class.java.getMethod("albums", kotlin.coroutines.Continuation::class.java)
        val get = MomentoApi::class.java.getMethod("album", AlbumIdRequest::class.java, kotlin.coroutines.Continuation::class.java)
        val update = MomentoApi::class.java.getMethod("updateAlbum", AlbumUpdateRequest::class.java, kotlin.coroutines.Continuation::class.java)
        val delete = MomentoApi::class.java.getMethod("deleteAlbum", AlbumIdRequest::class.java, kotlin.coroutines.Continuation::class.java)
        val add = MomentoApi::class.java.getMethod("addAlbumMedia", AlbumMediaRequest::class.java, kotlin.coroutines.Continuation::class.java)
        val remove = MomentoApi::class.java.getMethod("removeAlbumMedia", AlbumMediaRequest::class.java, kotlin.coroutines.Continuation::class.java)
        val reorder = MomentoApi::class.java.getMethod("reorderAlbumMedia", AlbumMediaRequest::class.java, kotlin.coroutines.Continuation::class.java)
        val map = MomentoApi::class.java.getMethod("mapClusters", MapClustersRequest::class.java, kotlin.coroutines.Continuation::class.java)
        assertEquals("api/v1/album/create", requireNotNull(create.getAnnotation(POST::class.java)).value)
        assertEquals("api/v1/album/list", requireNotNull(list.getAnnotation(POST::class.java)).value)
        assertEquals("api/v1/album/get", requireNotNull(get.getAnnotation(POST::class.java)).value)
        assertEquals("api/v1/album/update", requireNotNull(update.getAnnotation(POST::class.java)).value)
        assertEquals("api/v1/album/delete", requireNotNull(delete.getAnnotation(POST::class.java)).value)
        assertEquals("api/v1/album/add-media", requireNotNull(add.getAnnotation(POST::class.java)).value)
        assertEquals("api/v1/album/remove-media", requireNotNull(remove.getAnnotation(POST::class.java)).value)
        assertEquals("api/v1/album/reorder", requireNotNull(reorder.getAnnotation(POST::class.java)).value)
        assertEquals("api/v1/map/clusters", requireNotNull(map.getAnnotation(POST::class.java)).value)
        assertEquals(
            "{\"name\":\"Trip\",\"description\":null,\"mediaIds\":[7,8]}",
            Json.encodeToString(AlbumCreateRequest("Trip", null, listOf(7, 8))),
        )
    }

    @Test fun aiActionsUseBodylessAggregateAndExactFeatureRoutes() {
        val startAi = MomentoApi::class.java.getMethod("startAi", kotlin.coroutines.Continuation::class.java)
        val aiStatus = MomentoApi::class.java.getMethod("aiStatus", kotlin.coroutines.Continuation::class.java)
        val startFeature = MomentoApi::class.java.getMethod("startAiFeature", String::class.java, kotlin.coroutines.Continuation::class.java)
        val updateSchedule = MomentoApi::class.java.getMethod("updateAiSchedule", AiScheduleUpdateRequest::class.java, kotlin.coroutines.Continuation::class.java)
        val duplicates = MomentoApi::class.java.getMethod("duplicates", io.github.yzard.momento.core.model.PageRequest::class.java, kotlin.coroutines.Continuation::class.java)

        assertEquals("api/v1/ai/start", requireNotNull(startAi.getAnnotation(POST::class.java)).value)
        assertEquals("api/v1/ai/status", requireNotNull(aiStatus.getAnnotation(POST::class.java)).value)
        assertEquals("api/v1/ai/{feature}/start", requireNotNull(startFeature.getAnnotation(POST::class.java)).value)
        assertEquals("api/v1/ai/schedule/update", requireNotNull(updateSchedule.getAnnotation(POST::class.java)).value)
        assertEquals("api/v1/duplicates/list", requireNotNull(duplicates.getAnnotation(POST::class.java)).value)
    }

    @Test fun metadataActionsStillSendTheirRequiredEmptyJsonBody() {
        val metadata = MomentoApi::class.java.getMethod("metadataStatus", EmptyRequest::class.java, kotlin.coroutines.Continuation::class.java)

        assertEquals("api/v1/metadata/status", requireNotNull(metadata.getAnnotation(POST::class.java)).value)
        assertEquals("{}", Json.encodeToString(EmptyRequest()))
    }

    @Test fun collectionThumbnailsUseBinaryGetRoutes() {
        val placeThumbnail = LibraryApi::class.java.getMethod(
            "placeThumbnail",
            String::class.java,
            kotlin.coroutines.Continuation::class.java,
        )
        val faceThumbnail = LibraryApi::class.java.getMethod(
            "faceThumbnail",
            java.lang.Long.TYPE,
            kotlin.coroutines.Continuation::class.java,
        )

        assertEquals(
            "api/v1/places/{placeId}/thumbnail",
            requireNotNull(placeThumbnail.getAnnotation(GET::class.java)).value,
        )
        assertEquals(
            "api/v1/faces/groups/{faceGroupId}/thumbnail",
            requireNotNull(faceThumbnail.getAnnotation(GET::class.java)).value,
        )
    }
}
