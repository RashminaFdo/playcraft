package dev.rashmina.playcraft.controller

import android.content.Context
import android.content.SharedPreferences
import org.json.JSONObject

/**
 * Persistent PlayCraft Controller configuration & PS5 Cosmic Cyan design constants.
 * Author: Rashmina Fernando (https://www.rashmina.dev)
 */
object PlayCraftTheme {
    const val BG_OBSIDIAN = 0xFF070B14.toInt()
    const val BG_SURFACE = 0xFF0F1626.toInt()
    const val BG_ELEVATED = 0xFF162036.toInt()
    const val BORDER_HAIRLINE = 0x26FFFFFF
    const val CYAN_GLOW = 0xFF00E5FF.toInt()
    const val CYAN_SOFT = 0x2E00E5FF
    const val XBOX_GREEN = 0xFF22C55E.toInt()
    const val XBOX_SOFT = 0x2E22C55E
    const val GAS_GREEN = 0xFF10B981.toInt()
    const val BRAKE_RED = 0xFFEF4444.toInt()
    const val WARN_AMBER = 0xFFF59E0B.toInt()
    const val TEXT_WHITE = 0xFFEEF3FB.toInt()
    const val TEXT_MUTED = 0x99EEF3FB.toInt()

    const val TRANSPORT_WIFI = "WIFI"
    const val TRANSPORT_HOTSPOT = "HOTSPOT"
    const val TRANSPORT_BLUETOOTH = "BLUETOOTH"
    const val TRANSPORT_USB = "USB"

    const val DECK_WHEEL = "WHEEL"
    const val DECK_PAD = "PAD"

    const val SHELL_PS5 = "PS5"
    const val SHELL_XBOX = "XBOX"

    const val DRIVER_X360 = "X360"
    const val DRIVER_KEYS = "KEYS"

    const val STEER_HYBRID = "HYBRID"
    const val STEER_GYRO_ONLY = "GYRO_ONLY"
    const val STEER_TOUCH_ONLY = "TOUCH_ONLY"

    val BT_SPP_UUID: java.util.UUID =
        java.util.UUID.fromString("7a9c1b20-4e3f-4c8d-9a10-706c61796372")
    const val DEFAULT_WS_PORT = 19550
    const val DEFAULT_DISCOVERY_PORT = 19552
}

class ControllerConfig(context: Context) {
    private val prefs: SharedPreferences =
        context.getSharedPreferences("playcraft_controller_cfg", Context.MODE_PRIVATE)

    var rememberMe: Boolean
        get() = prefs.getBoolean("remember_me", true)
        set(v) = prefs.edit().putBoolean("remember_me", v).apply()

    var hasSavedConnection: Boolean
        get() = prefs.getBoolean("has_saved_conn", false)
        set(v) = prefs.edit().putBoolean("has_saved_conn", v).apply()

    var transportMode: String
        get() = prefs.getString("transport_mode", PlayCraftTheme.TRANSPORT_WIFI)
            ?: PlayCraftTheme.TRANSPORT_WIFI
        set(v) = prefs.edit().putString("transport_mode", v).apply()

    var savedHost: String
        get() = prefs.getString("saved_host", "") ?: ""
        set(v) = prefs.edit().putString("saved_host", v.trim()).apply()

    var savedPort: Int
        get() = prefs.getInt("saved_port", PlayCraftTheme.DEFAULT_WS_PORT)
        set(v) = prefs.edit().putInt("saved_port", v).apply()

    var savedPin: String
        get() = prefs.getString("saved_pin", "") ?: ""
        set(v) = prefs.edit().putString("saved_pin", v.trim()).apply()

    var savedBtMac: String
        get() = prefs.getString("saved_bt_mac", "") ?: ""
        set(v) = prefs.edit().putString("saved_bt_mac", v).apply()

    var savedBtName: String
        get() = prefs.getString("saved_bt_name", "") ?: ""
        set(v) = prefs.edit().putString("saved_bt_name", v).apply()

    var savedPcName: String
        get() = prefs.getString("saved_pc_name", "PlayCraft PC") ?: "PlayCraft PC"
        set(v) = prefs.edit().putString("saved_pc_name", v).apply()

    var lastUpdateCheckMs: Long
        get() = prefs.getLong("last_update_check_ms", 0L)
        set(v) = prefs.edit().putLong("last_update_check_ms", v).apply()

    var latestKnownVersionTag: String
        get() = prefs.getString("latest_known_version_tag", "") ?: ""
        set(v) = prefs.edit().putString("latest_known_version_tag", v.trim()).apply()

    var latestReleaseUrl: String
        get() = prefs.getString("latest_release_url", "") ?: ""
        set(v) = prefs.edit().putString("latest_release_url", v.trim()).apply()

    var playBootIntro: Boolean
        get() = prefs.getBoolean("play_boot_intro", true)
        set(v) = prefs.edit().putBoolean("play_boot_intro", v).apply()

    var onboardingCompleted: Boolean
        get() = prefs.getBoolean("onboarding_completed", false)
        set(v) = prefs.edit().putBoolean("onboarding_completed", v).apply()

    var deckMode: String
        get() = prefs.getString("deck_mode", PlayCraftTheme.DECK_WHEEL)
            ?: PlayCraftTheme.DECK_WHEEL
        set(v) = prefs.edit().putString("deck_mode", v).apply()

    var shellTheme: String
        get() = prefs.getString("shell_theme", PlayCraftTheme.SHELL_PS5)
            ?: PlayCraftTheme.SHELL_PS5
        set(v) = prefs.edit().putString("shell_theme", v).apply()

    var outputDriver: String
        get() = prefs.getString("output_driver", PlayCraftTheme.DRIVER_X360)
            ?: PlayCraftTheme.DRIVER_X360
        set(v) = prefs.edit().putString("output_driver", v).apply()

    var gyroEnabled: Boolean
        get() = prefs.getBoolean("gyro_enabled", true)
        set(v) = prefs.edit().putBoolean("gyro_enabled", v).apply()

    var steerInputMode: String
        get() = prefs.getString("steer_input_mode", PlayCraftTheme.STEER_HYBRID)
            ?: PlayCraftTheme.STEER_HYBRID
        set(v) = prefs.edit().putString("steer_input_mode", v).apply()

    var gyroMaxAngleDeg: Int
        get() = prefs.getInt("gyro_max_deg", 38).coerceIn(20, 90)
        set(v) = prefs.edit().putInt("gyro_max_deg", v.coerceIn(20, 90)).apply()

    var gyroDeadzonePct: Int
        get() = prefs.getInt("gyro_deadzone_pct", 4).coerceIn(0, 15)
        set(v) = prefs.edit().putInt("gyro_deadzone_pct", v.coerceIn(0, 15)).apply()

    var gyroGamma: Float
        get() = prefs.getFloat("gyro_gamma", 1.20f).coerceIn(0.70f, 2.20f)
        set(v) = prefs.edit().putFloat("gyro_gamma", v.coerceIn(0.70f, 2.20f)).apply()

    var gyroSmoothing: Float
        get() = prefs.getFloat("gyro_smoothing", 0.42f).coerceIn(0.15f, 0.90f)
        set(v) = prefs.edit().putFloat("gyro_smoothing", v.coerceIn(0.15f, 0.90f)).apply()

    var gyroInvert: Boolean
        get() = prefs.getBoolean("gyro_invert", false)
        set(v) = prefs.edit().putBoolean("gyro_invert", v).apply()

    var swapPedalSides: Boolean
        get() = prefs.getBoolean("swap_pedal_sides", false)
        set(v) = prefs.edit().putBoolean("swap_pedal_sides", v).apply()

    var progressivePedals: Boolean
        get() = prefs.getBoolean("progressive_pedals", true)
        set(v) = prefs.edit().putBoolean("progressive_pedals", v).apply()

    var hapticsEnabled: Boolean
        get() = prefs.getBoolean("haptics_enabled", true)
        set(v) = prefs.edit().putBoolean("haptics_enabled", v).apply()

    var gameRumbleScalePct: Int
        get() = prefs.getInt("game_rumble_pct", 100).coerceIn(0, 100)
        set(v) = prefs.edit().putInt("game_rumble_pct", v.coerceIn(0, 100)).apply()

    var uiSoundsEnabled: Boolean
        get() = prefs.getBoolean("ui_sounds_enabled", true)
        set(v) = prefs.edit().putBoolean("ui_sounds_enabled", v).apply()

    var keepScreenAwake: Boolean
        get() = prefs.getBoolean("keep_screen_awake", true)
        set(v) = prefs.edit().putBoolean("keep_screen_awake", v).apply()

    val defaultKeymap: Map<String, String> = linkedMapOf(
        "CROSS" to "SPACE",
        "CIRCLE" to "C",
        "TRIANGLE" to "R",
        "SQUARE" to "SHIFT",
        "L1" to "Q",
        "R1" to "E",
        "L2" to "S",
        "R2" to "W",
        "HANDBRAKE" to "SPACE",
        "OPTIONS" to "ESC",
        "SHARE" to "TAB",
        "PS" to "ENTER"
    )

    fun loadKeymap(): MutableMap<String, String> {
        val out = defaultKeymap.toMutableMap()
        val raw = prefs.getString("custom_keymap", null) ?: return out
        runCatching {
            val obj = JSONObject(raw)
            val keys = obj.keys()
            while (keys.hasNext()) {
                val k = keys.next()
                val v = obj.optString(k, "").trim().uppercase()
                if (v.isNotEmpty()) out[k] = v
            }
        }
        return out
    }

    fun saveKeymap(map: Map<String, String>) {
        val obj = JSONObject()
        map.forEach { (k, v) -> obj.put(k, v.trim().uppercase()) }
        prefs.edit().putString("custom_keymap", obj.toString()).apply()
    }

    fun activeAccentColor(): Int {
        return if (shellTheme == PlayCraftTheme.SHELL_XBOX) {
            PlayCraftTheme.XBOX_GREEN
        } else {
            PlayCraftTheme.CYAN_GLOW
        }
    }

    fun saveSuccessfulEndpoint(
        transport: String,
        host: String,
        port: Int,
        pin: String,
        btMac: String,
        btName: String,
        pcName: String
    ) {
        transportMode = transport
        if (host.isNotBlank()) savedHost = host
        savedPort = port
        if (pin.isNotBlank()) savedPin = pin
        if (btMac.isNotBlank()) savedBtMac = btMac
        if (btName.isNotBlank()) savedBtName = btName
        if (pcName.isNotBlank()) savedPcName = pcName
        hasSavedConnection = rememberMe
    }
}
