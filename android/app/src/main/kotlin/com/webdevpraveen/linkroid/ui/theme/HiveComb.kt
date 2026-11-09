// SPDX-License-Identifier: GPL-3.0-or-later
package com.webdevpraveen.linkroid.ui.theme

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.PowerManager
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.LocalContext

/** Deprecated compatibility placeholder: Hive theme has been purged. */
val LocalHiveTheme = compositionLocalOf { false }

/** True while Android's battery saver is on: decorative animation rests. */
@Composable
fun rememberPowerSaver(): Boolean {
    val context = LocalContext.current
    val power = remember(context) { context.getSystemService(PowerManager::class.java) }
    var saving by remember { mutableStateOf(power?.isPowerSaveMode == true) }
    DisposableEffect(context) {
        val filter = IntentFilter(PowerManager.ACTION_POWER_SAVE_MODE_CHANGED)
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(c: Context?, intent: Intent?) {
                saving = power?.isPowerSaveMode == true
            }
        }
        context.registerReceiver(receiver, filter)
        onDispose { context.unregisterReceiver(receiver) }
    }
    return saving
}

/** Deprecated compatibility no-op: Hive honeycomb has been purged. */
@Composable
fun HiveComb(
    dark: Boolean,
    animate: Boolean,
    modifier: Modifier = Modifier,
    cellDp: Float = 0f,
    origin: (androidx.compose.ui.geometry.Size) -> Offset = { Offset.Zero },
) {
    // No-op
}
