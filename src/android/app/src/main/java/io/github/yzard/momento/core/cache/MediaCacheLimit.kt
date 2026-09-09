package io.github.yzard.momento.core.cache

enum class MediaCacheLimit(val bytes: Long, val label: String) {
    MIB_256(256L * 1024 * 1024, "256 MiB"),
    MIB_512(512L * 1024 * 1024, "512 MiB"),
    GIB_1(1024L * 1024 * 1024, "1 GiB"),
    GIB_2(2L * 1024 * 1024 * 1024, "2 GiB"),
}

fun parseMediaCacheLimit(value: String?): MediaCacheLimit =
    MediaCacheLimit.entries.firstOrNull { it.name == value } ?: MediaCacheLimit.MIB_512
