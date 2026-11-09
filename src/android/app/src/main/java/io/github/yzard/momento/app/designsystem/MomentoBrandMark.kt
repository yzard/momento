package io.github.yzard.momento.app.designsystem

import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.Dp
import io.github.yzard.momento.R

@Composable
fun MomentoBrandMark(size: Dp, modifier: Modifier) {
    Image(
        painter = painterResource(R.drawable.momento_brand),
        contentDescription = "Momento",
        modifier = modifier.size(size).clip(RoundedCornerShape(size / 4)),
    )
}
