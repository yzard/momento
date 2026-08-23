package io.github.yzard.momento.feature.media

fun adaptiveGridColumns(widthDp: Int): Int = if (widthDp >= 600) 5 else 3
