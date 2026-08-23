package io.github.yzard.momento.core.network

import io.github.yzard.momento.core.model.BackupUploadIdRequest
import io.github.yzard.momento.core.model.AlbumMediaRequest
import io.github.yzard.momento.core.model.MapClustersRequest
import org.junit.Assert.assertEquals
import org.junit.Test
import retrofit2.http.POST

class MomentoApiContractTest {
    @Test fun uploadStatusUsesTheBodyBasedBackendRoute() {
        val method = MomentoApi::class.java.getMethod("uploadStatus", BackupUploadIdRequest::class.java, kotlin.coroutines.Continuation::class.java)
        assertEquals("api/v1/backup/upload/status", requireNotNull(method.getAnnotation(POST::class.java)).value)
    }

    @Test fun albumAndMapUseTheServerContracts() {
        val album = MomentoApi::class.java.getMethod("reorderAlbumMedia", AlbumMediaRequest::class.java, kotlin.coroutines.Continuation::class.java)
        val map = MomentoApi::class.java.getMethod("mapClusters", MapClustersRequest::class.java, kotlin.coroutines.Continuation::class.java)
        assertEquals("api/v1/album/reorder", requireNotNull(album.getAnnotation(POST::class.java)).value)
        assertEquals("api/v1/map/clusters", requireNotNull(map.getAnnotation(POST::class.java)).value)
    }
}
