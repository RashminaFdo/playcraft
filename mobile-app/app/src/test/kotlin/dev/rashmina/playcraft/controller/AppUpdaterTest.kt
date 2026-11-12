package dev.rashmina.playcraft.controller

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AppUpdaterTest {
    @Test
    fun semverComparisonHandlesTagsAndPrereleases() {
        assertTrue(AppUpdater.isRemoteVersionNewer("1.0.0", "v1.0.1"))
        assertTrue(AppUpdater.isRemoteVersionNewer("1.0.0", "v1.1.0"))
        assertTrue(AppUpdater.isRemoteVersionNewer("1.9.9", "2.0.0"))
        assertFalse(AppUpdater.isRemoteVersionNewer("1.0.0", "v1.0.0"))
        assertFalse(AppUpdater.isRemoteVersionNewer("1.2.0", "v1.1.9"))
        assertTrue(AppUpdater.isRemoteVersionNewer("1.0.0-beta.1", "v1.0.0"))
    }
}

