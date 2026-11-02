package io.github.yzard.momento.feature.viewer

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.platform.LocalView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.media3.common.Player

@Composable
fun VideoScreenAwake(player: Player, active: Boolean) {
    if (!active) return
    val view = LocalView.current
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    DisposableEffect(player, view, lifecycle) {
        val previous = view.keepScreenOn
        fun update() {
            view.keepScreenOn = previous || (
                lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED) &&
                    player.playWhenReady &&
                    (player.playbackState == Player.STATE_READY || player.playbackState == Player.STATE_BUFFERING)
                )
        }
        val playerListener = object : Player.Listener {
            override fun onEvents(player: Player, events: Player.Events) = update()
        }
        val lifecycleObserver = LifecycleEventObserver { _, _ -> update() }
        player.addListener(playerListener)
        lifecycle.addObserver(lifecycleObserver)
        update()
        onDispose {
            player.removeListener(playerListener)
            lifecycle.removeObserver(lifecycleObserver)
            view.keepScreenOn = previous
        }
    }
}
