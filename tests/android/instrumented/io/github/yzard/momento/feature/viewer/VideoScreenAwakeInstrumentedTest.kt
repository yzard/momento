package io.github.yzard.momento.feature.viewer

import android.view.View
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleRegistry
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.LifecycleOwner
import androidx.compose.runtime.CompositionLocalProvider
import androidx.media3.common.Player
import androidx.media3.common.FlagSet
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import java.lang.reflect.Proxy

@androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
class VideoScreenAwakeInstrumentedTest {
    @get:Rule val composeRule = createComposeRule()

    @Test
    fun playbackKeepsScreenAwakeAcrossHiddenControlsAndReleasesItOnPauseStopOrClose() {
        val listeners = mutableListOf<Player.Listener>()
        var playing = true
        var state = Player.STATE_READY
        val player = Proxy.newProxyInstance(
            Player::class.java.classLoader,
            arrayOf(Player::class.java),
        ) { proxy, method, args ->
            when (method.name) {
                "getPlayWhenReady" -> playing
                "getPlaybackState" -> state
                "addListener" -> { listeners.add(args!![0] as Player.Listener); Unit }
                "removeListener" -> { listeners.remove(args!![0] as Player.Listener); Unit }
                "equals" -> proxy === args!![0]
                "hashCode" -> System.identityHashCode(proxy)
                "toString" -> "TestPlayer"
                else -> error("Unexpected Player method: ${method.name}")
            }
        } as Player
        val visible = mutableStateOf(true)
        val active = mutableStateOf(true)
        lateinit var view: View
        val owner = object : LifecycleOwner {
            val registry = LifecycleRegistry(this)
            override val lifecycle: Lifecycle get() = registry
        }
        InstrumentationRegistry.getInstrumentation().runOnMainSync {
            owner.registry.currentState = Lifecycle.State.RESUMED
        }
        composeRule.setContent {
            view = LocalView.current
            CompositionLocalProvider(LocalLifecycleOwner provides owner) {
                if (visible.value) VideoScreenAwake(player, active.value)
            }
        }
        fun notifyPlayback() {
            listeners.toList().forEach { it.onEvents(player, Player.Events(FlagSet.Builder().build())) }
        }
        composeRule.runOnIdle {
            assertTrue(view.keepScreenOn)
            playing = false
            notifyPlayback()
            assertFalse(view.keepScreenOn)
            playing = true
            state = Player.STATE_BUFFERING
            notifyPlayback()
            assertTrue(view.keepScreenOn)
            state = Player.STATE_ENDED
            notifyPlayback()
            assertFalse(view.keepScreenOn)
            state = Player.STATE_READY
            notifyPlayback()
            owner.registry.currentState = Lifecycle.State.CREATED
            assertFalse(view.keepScreenOn)
            owner.registry.currentState = Lifecycle.State.RESUMED
            assertTrue(view.keepScreenOn)
            active.value = false
        }
        composeRule.runOnIdle {
            assertFalse(view.keepScreenOn)
            assertTrue(listeners.isEmpty())
            active.value = true
        }
        composeRule.runOnIdle {
            assertTrue(view.keepScreenOn)
            visible.value = false
        }
        composeRule.runOnIdle {
            assertFalse(view.keepScreenOn)
            assertTrue(listeners.isEmpty())
        }
    }
}
