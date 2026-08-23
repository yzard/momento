package io.github.yzard.momento.app.navigation

import org.junit.Assert.assertTrue
import org.junit.Test

class RoutesTest { @Test fun collectionsDestinationsArePresent() { assertTrue(Destination.entries.containsAll(listOf(Destination.ALBUMS, Destination.MAP, Destination.PLACES, Destination.FACES, Destination.DEDUPLICATE, Destination.TRASH))) } }
