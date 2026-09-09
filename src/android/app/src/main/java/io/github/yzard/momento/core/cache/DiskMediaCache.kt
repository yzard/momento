package io.github.yzard.momento.core.cache

import java.io.Closeable
import java.io.File
import java.io.FileInputStream
import java.io.IOException
import java.util.Properties
import java.util.UUID
import okio.Buffer
import okio.BufferedSink
import okio.buffer
import okio.sink

internal data class CachedMediaMetadata(
    val contentType: String?,
    val etag: String?,
    val lastModified: String?,
    val validatedAt: Long,
)

internal class CachedMediaSnapshot(
    val key: String,
    val filename: String,
    val length: Long,
    val metadata: CachedMediaMetadata,
    val input: FileInputStream,
) : Closeable {
    override fun close() = input.close()
}

/** One process-wide owner; immutable data files and atomic metadata publication. */
internal class DiskMediaCache(private val directory: File, private var maxBytes: Long) {
    private data class Entry(val file: File, val metadata: CachedMediaMetadata, val cost: Long)
    private val entries = LinkedHashMap<String, Entry>(16, 0.75f, true)
    private val editors = mutableSetOf<Editor>()
    private var initialized = false
    private var storedBytes = 0L
    private var reservedBytes = 0L

    @Synchronized private fun initialize() {
        if (initialized) return
        if (!directory.isDirectory && !directory.mkdirs()) throw IOException("Could not open media cache")
        directory.listFiles().orEmpty().filter { it.extension == "meta" }.sortedBy { it.lastModified() }.forEach { file ->
            try {
                val properties = Properties().apply { file.inputStream().use { load(it) } }
                val filename = properties.getProperty("file") ?: throw IOException("Missing cached file")
                if (!filename.matches(Regex("[a-f0-9-]+\\.data"))) throw IOException("Invalid cached file")
                val data = File(directory, filename)
                val length = properties.getProperty("length")?.toLongOrNull()
                if (!data.isFile || data.length() != length) throw IOException("Incomplete cached file")
                entries[file.nameWithoutExtension] = Entry(data, CachedMediaMetadata(
                    properties.getProperty("type"), properties.getProperty("etag"), properties.getProperty("modified"),
                    properties.getProperty("validated")?.toLongOrNull() ?: 0,
                ), data.length() + file.length())
            } catch (_: IOException) {
                file.delete()
            } catch (_: IllegalArgumentException) {
                file.delete()
            }
        }
        val retained = entries.values.mapTo(mutableSetOf()) { it.file.name }
        directory.listFiles().orEmpty().filter {
            it.extension == "part" || (it.extension == "data" && it.name !in retained)
        }.forEach { it.delete() }
        storedBytes = entries.values.sumOf { it.cost }
        initialized = true
        trim(0)
    }

    @Synchronized fun resize(bytes: Long) {
        require(bytes > 0)
        initialize()
        maxBytes = bytes
        editors.toList().forEach { it.abort() }
        trim(0)
    }

    @Synchronized fun snapshot(key: String, now: Long): CachedMediaSnapshot? {
        initialize()
        val entry = entries[key] ?: return null
        return try {
            val input = FileInputStream(entry.file)
            File(directory, "$key.meta").setLastModified(now)
            CachedMediaSnapshot(key, entry.file.name, entry.file.length(), entry.metadata, input)
        } catch (_: IOException) { remove(key); null }
    }

    @Synchronized fun remove(key: String) {
        initialize()
        editors.filter { it.key == key }.toList().forEach { it.abort() }
        entries.remove(key)?.let { storedBytes -= it.cost; it.file.delete() }
        File(directory, "$key.meta").delete()
    }

    @Synchronized fun revalidated(snapshot: CachedMediaSnapshot, now: Long) {
        val entry = entries[snapshot.key] ?: return
        if (entry.file.name != snapshot.filename) return
        val metadata = entry.metadata.copy(validatedAt = now)
        val metadataBytes = publishMetadata(snapshot.key, entry.file, metadata, now)
        val cost = entry.file.length() + metadataBytes
        storedBytes += cost - entry.cost
        entries[snapshot.key] = entry.copy(metadata = metadata, cost = cost)
        trim(0)
    }

    @Synchronized fun begin(key: String, length: Long, metadata: CachedMediaMetadata): Editor? {
        initialize()
        val bodyBudget = if (length >= 0) length else 1024L * 1024
        if (bodyBudget > maxBytes - METADATA_BUDGET) return null
        val reservation = bodyBudget + METADATA_BUDGET
        if (reservation > maxBytes || editors.any { it.key == key }) return null
        trim(reservation)
        if (usedBytes() + reservation > maxBytes) return null
        val file = File(directory, "${UUID.randomUUID()}.part")
        return Editor(key, file, file.sink().buffer(), length, bodyBudget, reservation, metadata).also { editors.add(it); reservedBytes += reservation }
    }

    private fun usedBytes(): Long = storedBytes + reservedBytes
    private fun trim(incoming: Long) {
        while (entries.isNotEmpty() && usedBytes() + incoming > maxBytes) remove(entries.keys.first())
    }

    private fun publishMetadata(key: String, data: File, metadata: CachedMediaMetadata, now: Long): Long {
        val temporary = File(directory, "${UUID.randomUUID()}.part")
        val target = File(directory, "$key.meta")
        try {
            val properties = Properties().apply {
                setProperty("file", data.name); setProperty("length", data.length().toString())
                setProperty("validated", metadata.validatedAt.toString())
                metadata.contentType?.let { setProperty("type", it) }
                metadata.etag?.let { setProperty("etag", it) }
                metadata.lastModified?.let { setProperty("modified", it) }
            }
            temporary.outputStream().use { properties.store(it, null) }
            if (temporary.length() > METADATA_BUDGET) throw IOException("Cache metadata is too large")
            if (!temporary.renameTo(target)) throw IOException("Could not publish cache metadata")
            target.setLastModified(now)
            return target.length()
        } finally { temporary.delete() }
    }

    inner class Editor internal constructor(
        val key: String,
        private val temporary: File,
        private val output: BufferedSink,
        private val expectedLength: Long,
        private val bodyBudget: Long,
        val reservation: Long,
        private val metadata: CachedMediaMetadata,
    ) {
        private var written = 0L
        private var finished = false

        fun append(source: Buffer, offset: Long, count: Long) = synchronized(this@DiskMediaCache) {
            if (finished) return@synchronized
            if (written + count > bodyBudget) { abort(); return@synchronized }
            source.copyTo(output.buffer, offset, count)
            output.emitCompleteSegments()
            written += count
            if (written == expectedLength) complete()
        }

        fun complete() = synchronized(this@DiskMediaCache) {
            if (finished) return@synchronized
            if (expectedLength >= 0 && written != expectedLength) { abort(); return@synchronized }
            val data = File(directory, temporary.nameWithoutExtension + ".data")
            try {
                output.close()
                if (!temporary.renameTo(data)) throw IOException("Could not publish cached media")
                val metadataBytes = publishMetadata(key, data, metadata, metadata.validatedAt)
                val cost = written + metadataBytes
                val previous = entries.put(key, Entry(data, metadata, cost))
                storedBytes += cost - (previous?.cost ?: 0)
                previous?.file?.delete()
                finished = true
                if (editors.remove(this)) reservedBytes -= reservation
                trim(0)
            } catch (error: IOException) {
                data.delete()
                abort()
                throw error
            }
        }

        fun abort() = synchronized(this@DiskMediaCache) {
            if (finished) return@synchronized
            finished = true
            try { output.close() } finally { temporary.delete(); if (editors.remove(this)) reservedBytes -= reservation }
        }
    }

    private companion object { const val METADATA_BUDGET = 16L * 1024 }
}
