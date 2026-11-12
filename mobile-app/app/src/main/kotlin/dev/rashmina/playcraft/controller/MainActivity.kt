package dev.rashmina.playcraft.controller

import android.Manifest
import android.annotation.SuppressLint
import android.app.Activity
import android.app.AlertDialog
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.graphics.Matrix
import android.graphics.SurfaceTexture
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.media.MediaPlayer
import android.net.ConnectivityManager
import android.net.Network
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.text.InputType
import android.view.Gravity
import android.view.Surface
import android.view.TextureView
import android.view.View
import android.view.WindowManager
import android.widget.Button
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.HorizontalScrollView
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.SeekBar
import android.widget.Switch
import android.widget.TextView
import android.widget.Toast
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import kotlin.concurrent.thread
import kotlin.math.max

/**
 * PlayCraft Native Mobile Controller (Wi-Fi, Phone Hotspot, Bluetooth RFCOMM SPP & USB)
 * Features zoomed center-crop boot intro, Portrait QR scanner, Xbox/PS5 live game rumble feedback,
 * responsive frosted glass UI, and zero-overlap multi-screen controller layouts.
 * Author: Rashmina Fernando (https://www.rashmina.dev)
 */
class MainActivity : Activity(), BridgeListener {

    private lateinit var config: ControllerConfig
    private lateinit var audio: PlayCraftAudio
    private lateinit var bridge: NetworkBridge
    private lateinit var gyroEngine: GyroWheelEngine

    private val mainHandler = Handler(Looper.getMainLooper())
    private val REQ_ALL_PERMS = 9001
    private val REQ_QR_SCAN = 9002

    private lateinit var rootFrame: FrameLayout
    private lateinit var hubLayer: LinearLayout
    private lateinit var deckLayer: LinearLayout
    private lateinit var deckContainer: FrameLayout
    private lateinit var permLayer: LinearLayout
    private lateinit var bootLayer: FrameLayout
    private lateinit var settingsOverlay: FrameLayout

    private var bootTextureView: TextureView? = null
    private var bootMediaPlayer: MediaPlayer? = null

    private lateinit var wheelDeck: WheelDeckView
    private lateinit var gamepadDeck: GamepadDeckView

    // Active Controller Top Bar Controls
    private lateinit var padTabBtn: Button
    private lateinit var wheelTabBtn: Button
    private lateinit var ps5TabBtn: Button
    private lateinit var xboxTabBtn: Button
    private lateinit var x360TabBtn: Button
    private lateinit var keysTabBtn: Button
    private lateinit var topUpdateBtn: Button
    private lateinit var topSettingsBtn: Button
    private lateinit var topHubBtn: Button
    private lateinit var statusTextView: TextView
    private lateinit var statusDotView: View

    // Connection Hub Fields
    private lateinit var transportButtons: MutableMap<String, Button>
    private lateinit var hostEdit: EditText
    private lateinit var portEdit: EditText
    private lateinit var pinEdit: EditText
    private lateinit var rememberSwitch: Switch
    private lateinit var discoverListLayout: LinearLayout
    private lateinit var btDevicesLayout: LinearLayout
    private lateinit var wifiFieldsPanel: LinearLayout
    private lateinit var btFieldsPanel: LinearLayout
    private lateinit var hubStatusBanner: TextView
    private lateinit var hubUpdateBanner: LinearLayout
    private lateinit var hubUpdateTitleText: TextView

    private var selectedBtMac: String = ""
    private var selectedBtName: String = ""
    private var autoConnectTriggeredOnStart = false
    private var networkCallback: ConnectivityManager.NetworkCallback? = null
    private var latestUpdateInfo: AndroidUpdateInfo? = null
    private var settingsUpdateStatusView: TextView? = null
    private val dailyUpdateCheckRunnable = object : Runnable {
        override fun run() {
            triggerSilentDailyUpdateCheck(force = false)
            mainHandler.postDelayed(this, 6L * 60L * 60L * 1000L)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        config = ControllerConfig(this)
        audio = PlayCraftAudio(this, config)
        bridge = NetworkBridge(this, config, this)
        gyroEngine = GyroWheelEngine(this, config) { axis, deg, label ->
            if (config.deckMode == PlayCraftTheme.DECK_WHEEL && !wheelDeck.isTouchSteerHolding()) {
                wheelDeck.updateSteerTelemetry(axis, deg, label)
                if (bridge.isConnectedAndAuthed()) {
                    bridge.sendGyroSteer(axis, deg)
                }
            }
        }

        applyImmersiveGameWindow()
        buildRootUi()
        parseIncomingPairIntent(intent)
        registerNetworkWatcher()
        mainHandler.postDelayed(dailyUpdateCheckRunnable, 2400L)

        if (config.playBootIntro) {
            showBootIntro()
        } else {
            onBootFinished()
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        parseIncomingPairIntent(intent)
    }

    private fun applyImmersiveGameWindow() {
        WindowCompat.setDecorFitsSystemWindows(window, false)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            window.attributes = window.attributes.apply {
                layoutInDisplayCutoutMode =
                    WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
            }
        }
        val controller = WindowInsetsControllerCompat(window, window.decorView)
        controller.systemBarsBehavior =
            WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        controller.hide(WindowInsetsCompat.Type.systemBars())
        updateScreenAwakeFlag()
    }

    private fun updateScreenAwakeFlag() {
        if (config.keepScreenAwake) {
            window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        } else {
            window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        }
    }

    private fun requiredRuntimePermissions(): Array<String> {
        val list = mutableListOf<String>()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            list.add(Manifest.permission.BLUETOOTH_SCAN)
            list.add(Manifest.permission.BLUETOOTH_CONNECT)
        } else {
            list.add(Manifest.permission.ACCESS_FINE_LOCATION)
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            list.add(Manifest.permission.NEARBY_WIFI_DEVICES)
        }
        list.add(Manifest.permission.CAMERA)
        return list.toTypedArray()
    }

    private fun hasMissingPermissions(): Boolean {
        return requiredRuntimePermissions().any {
            ContextCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED
        }
    }

    private fun buildRootUi() {
        rootFrame = FrameLayout(this).apply {
            setBackgroundColor(PlayCraftTheme.BG_OBSIDIAN)
            keepScreenOn = config.keepScreenAwake
        }

        wheelDeck = WheelDeckView(
            context = this,
            config = config,
            bridge = bridge,
            audio = audio,
            onZeroGyro = {
                gyroEngine.calibrateZero()
                toast("Steering centered cleanly")
            },
            onToggleGyro = {
                config.gyroEnabled = !config.gyroEnabled
                if (!config.gyroEnabled) {
                    gyroEngine.resetFilter()
                    bridge.sendGyroSteer(0f, 0f)
                    wheelDeck.updateSteerTelemetry(0f, 0f, "Gyro OFF")
                } else {
                    gyroEngine.calibrateZero()
                }
                wheelDeck.invalidate()
            },
            onOpenSettings = { showControllerSettingsOverlay() }
        )
        gamepadDeck = GamepadDeckView(this, config, bridge, audio)

        hubLayer = buildConnectionHubLayer()
        deckLayer = buildActiveDeckLayer()
        permLayer = buildPermissionsOnboardingLayer()
        bootLayer = buildBootIntroLayer()
        settingsOverlay = FrameLayout(this).apply { visibility = View.GONE }

        hubLayer.visibility = View.GONE
        deckLayer.visibility = View.GONE
        permLayer.visibility = View.GONE
        bootLayer.visibility = View.GONE

        rootFrame.addView(hubLayer, FrameLayout.LayoutParams(-1, -1))
        rootFrame.addView(deckLayer, FrameLayout.LayoutParams(-1, -1))
        rootFrame.addView(permLayer, FrameLayout.LayoutParams(-1, -1))
        rootFrame.addView(bootLayer, FrameLayout.LayoutParams(-1, -1))
        rootFrame.addView(settingsOverlay, FrameLayout.LayoutParams(-1, -1))
        setContentView(rootFrame)
    }

    // ================== FULL-BLEED CENTER-CROP BOOT INTRO OVERLAY ==================

    private fun buildBootIntroLayer(): FrameLayout {
        val wrap = FrameLayout(this).apply {
            setBackgroundColor(Color.BLACK)
        }
        val tv = TextureView(this).apply {
            layoutParams = FrameLayout.LayoutParams(-1, -1)
        }
        bootTextureView = tv
        wrap.addView(tv)

        val dp = resources.displayMetrics.density
        val hud = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.BOTTOM or Gravity.CENTER_VERTICAL
            setPadding((28 * dp).toInt(), (16 * dp).toInt(), (28 * dp).toInt(), (20 * dp).toInt())
            layoutParams = FrameLayout.LayoutParams(-1, -2, Gravity.BOTTOM)
        }

        val brand = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            background = cardDrawable(0xC8070B14.toInt(), PlayCraftTheme.CYAN_GLOW, 14f * dp)
            setPadding((14 * dp).toInt(), (8 * dp).toInt(), (18 * dp).toInt(), (8 * dp).toInt())
            layoutParams = LinearLayout.LayoutParams(0, -2, 1f)
        }
        val logo = ImageView(this).apply {
            setImageResource(R.drawable.playcraft_logo)
            layoutParams = LinearLayout.LayoutParams((44 * dp).toInt(), (22 * dp).toInt())
            scaleType = ImageView.ScaleType.FIT_CENTER
        }
        val label = TextView(this).apply {
            text = "  PlayCraft Mobile Controller \u2022 Rashmina Fernando"
            setTextColor(PlayCraftTheme.TEXT_WHITE)
            textSize = 12.5f
            typeface = Typeface.DEFAULT_BOLD
        }
        brand.addView(logo)
        brand.addView(label)

        val skipBtn = styledButton("Skip Intro \u25B6", PlayCraftTheme.CYAN_GLOW, PlayCraftTheme.BG_OBSIDIAN).apply {
            setOnClickListener {
                audio.playNavTick()
                finishBootIntro()
            }
        }
        hud.addView(brand)
        hud.addView(spaceH(12))
        hud.addView(skipBtn)
        wrap.addView(hud)
        return wrap
    }

    private fun applyCenterCropVideoTransform(tv: TextureView, videoW: Int, videoH: Int) {
        val viewW = tv.width.toFloat()
        val viewH = tv.height.toFloat()
        if (viewW <= 0f || viewH <= 0f || videoW <= 0 || videoH <= 0) return

        val viewRatio = viewW / viewH
        val vidRatio = videoW.toFloat() / videoH.toFloat()
        var scaleX = 1f
        var scaleY = 1f
        if (vidRatio > viewRatio) {
            scaleX = vidRatio / viewRatio
        } else {
            scaleY = viewRatio / vidRatio
        }
        val matrix = Matrix()
        matrix.setScale(scaleX, scaleY, viewW / 2f, viewH / 2f)
        tv.setTransform(matrix)
    }

    private fun showBootIntro() {
        bootLayer.visibility = View.VISIBLE
        val tv = bootTextureView ?: run { finishBootIntro(); return }

        fun startPlayback(surfaceTexture: SurfaceTexture) {
            runCatching {
                val mp = MediaPlayer()
                bootMediaPlayer = mp
                val uri = Uri.parse("android.resource://$packageName/${R.raw.boot_intro}")
                mp.setDataSource(this, uri)
                mp.setSurface(Surface(surfaceTexture))
                mp.setOnVideoSizeChangedListener { _, vw, vh ->
                    applyCenterCropVideoTransform(tv, vw, vh)
                }
                mp.setOnPreparedListener {
                    applyCenterCropVideoTransform(tv, mp.videoWidth, mp.videoHeight)
                    mp.start()
                }
                mp.setOnCompletionListener { finishBootIntro() }
                mp.setOnErrorListener { _, _, _ -> finishBootIntro(); true }
                mp.prepareAsync()
            }.onFailure {
                finishBootIntro()
            }
        }

        if (tv.isAvailable && tv.surfaceTexture != null) {
            startPlayback(tv.surfaceTexture!!)
        } else {
            tv.surfaceTextureListener = object : TextureView.SurfaceTextureListener {
                override fun onSurfaceTextureAvailable(st: SurfaceTexture, w: Int, h: Int) {
                    startPlayback(st)
                }
                override fun onSurfaceTextureSizeChanged(st: SurfaceTexture, w: Int, h: Int) {
                    bootMediaPlayer?.let { mp -> applyCenterCropVideoTransform(tv, mp.videoWidth, mp.videoHeight) }
                }
                override fun onSurfaceTextureDestroyed(st: SurfaceTexture): Boolean = true
                override fun onSurfaceTextureUpdated(st: SurfaceTexture) {}
            }
        }
        mainHandler.postDelayed({ if (bootLayer.visibility == View.VISIBLE) finishBootIntro() }, 6500L)
    }

    private fun finishBootIntro() {
        if (bootLayer.visibility == View.GONE) return
        runCatching { bootMediaPlayer?.stop() }
        runCatching { bootMediaPlayer?.release() }
        bootMediaPlayer = null
        bootLayer.visibility = View.GONE
        onBootFinished()
    }

    private fun onBootFinished() {
        if (!config.onboardingCompleted || hasMissingPermissions()) {
            permLayer.visibility = View.VISIBLE
        } else {
            enterConnectionHubAndMaybeAutoConnect()
        }
    }

    // ================== PERMISSIONS ONBOARDING + SYSTEM REDIRECTS ==================

    private fun buildPermissionsOnboardingLayer(): LinearLayout {
        val scroll = ScrollView(this)
        val dp = resources.displayMetrics.density
        val outer = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(PlayCraftTheme.BG_OBSIDIAN)
            setPadding((28 * dp).toInt(), (20 * dp).toInt(), (28 * dp).toInt(), (20 * dp).toInt())
            gravity = Gravity.CENTER_HORIZONTAL
        }

        val card = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            background = cardDrawable(PlayCraftTheme.BG_SURFACE, PlayCraftTheme.BORDER_HAIRLINE, 18f * dp)
            setPadding((24 * dp).toInt(), (20 * dp).toInt(), (24 * dp).toInt(), (20 * dp).toInt())
            layoutParams = LinearLayout.LayoutParams(-1, -2)
        }

        val header = TextView(this).apply {
            text = "Permissions & Gaming Optimization Setup"
            setTextColor(PlayCraftTheme.CYAN_GLOW)
            textSize = 18f
            typeface = Typeface.DEFAULT_BOLD
        }
        val desc = TextView(this).apply {
            text = "PlayCraft requests all controller permissions upfront for lowest latency:\n\n" +
                "\u2022 Nearby Devices & Bluetooth: Fast discovery & pairing across Wi-Fi, Hotspot & Bluetooth SPP\n" +
                "\u2022 Camera: Portrait QR pairing scanner\n" +
                "\u2022 Unthrottled Battery & Haptics: Keeps 60Hz+ gyro steering & live PS5/Xbox rumble smooth mid-race"
            setTextColor(PlayCraftTheme.TEXT_WHITE)
            textSize = 12.5f
            setPadding(0, (10 * dp).toInt(), 0, (14 * dp).toInt())
        }

        val btnRow = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        val grantBtn = styledButton("Grant Permissions", PlayCraftTheme.CYAN_GLOW, PlayCraftTheme.BG_OBSIDIAN).apply {
            layoutParams = LinearLayout.LayoutParams(0, -2, 1f)
            setOnClickListener {
                audio.playNavTick()
                ActivityCompat.requestPermissions(this@MainActivity, requiredRuntimePermissions(), REQ_ALL_PERMS)
            }
        }
        val appSetBtn = styledButton("App Settings", PlayCraftTheme.BG_ELEVATED, PlayCraftTheme.TEXT_WHITE).apply {
            layoutParams = LinearLayout.LayoutParams(0, -2, 1f)
            setOnClickListener { openAndroidAppSettings() }
        }
        val batSetBtn = styledButton("Battery", PlayCraftTheme.BG_ELEVATED, PlayCraftTheme.TEXT_WHITE).apply {
            layoutParams = LinearLayout.LayoutParams(0, -2, 0.9f)
            setOnClickListener { openBatteryOptimizationSettings() }
        }
        val continueBtn = styledButton("Continue \u2192", PlayCraftTheme.GAS_GREEN, PlayCraftTheme.BG_OBSIDIAN).apply {
            layoutParams = LinearLayout.LayoutParams(0, -2, 1f)
            setOnClickListener {
                audio.playNavTick()
                config.onboardingCompleted = true
                permLayer.visibility = View.GONE
                enterConnectionHubAndMaybeAutoConnect()
            }
        }

        btnRow.addView(grantBtn); btnRow.addView(spaceH(8))
        btnRow.addView(appSetBtn); btnRow.addView(spaceH(8))
        btnRow.addView(batSetBtn); btnRow.addView(spaceH(8))
        btnRow.addView(continueBtn)

        card.addView(header)
        card.addView(desc)
        card.addView(btnRow)
        scroll.addView(card)

        return LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            setBackgroundColor(PlayCraftTheme.BG_OBSIDIAN)
            addView(scroll, LinearLayout.LayoutParams(-1, -1))
        }
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == REQ_ALL_PERMS) {
            config.onboardingCompleted = true
            permLayer.visibility = View.GONE
            if (hasMissingPermissions()) {
                toast("Some permissions skipped — use Permissions or App Settings anytime")
            }
            enterConnectionHubAndMaybeAutoConnect()
        }
    }

    // ================== CONNECTION HUB (RESPONSIVE + MULTI-TRANSPORT + REMEMBER ME) ==================

    private fun buildConnectionHubLayer(): LinearLayout {
        val dp = resources.displayMetrics.density
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(PlayCraftTheme.BG_OBSIDIAN)
            setPadding((18 * dp).toInt(), (10 * dp).toInt(), (18 * dp).toInt(), (10 * dp).toInt())
        }

        val topRow = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, 0, 0, (6 * dp).toInt())
        }
        val logo = ImageView(this).apply {
            setImageResource(R.drawable.playcraft_logo)
            layoutParams = LinearLayout.LayoutParams((42 * dp).toInt(), (24 * dp).toInt())
            scaleType = ImageView.ScaleType.FIT_CENTER
        }
        val titleTxt = TextView(this).apply {
            text = "  PlayCraft Controller Hub  \u2022  ${gyroEngine.sensorStatusLabel}"
            setTextColor(PlayCraftTheme.TEXT_WHITE)
            textSize = 14f
            typeface = Typeface.DEFAULT_BOLD
            layoutParams = LinearLayout.LayoutParams(0, -2, 1f)
        }
        val permsShortcutBtn = styledButton("Permissions", PlayCraftTheme.BG_ELEVATED, PlayCraftTheme.TEXT_WHITE).apply {
            textSize = 11.5f
            setOnClickListener { openAndroidAppSettings() }
        }
        val quickSettingsBtn = styledButton("Settings & Tuning", PlayCraftTheme.BG_ELEVATED, PlayCraftTheme.CYAN_GLOW).apply {
            textSize = 11.5f
            setOnClickListener { showControllerSettingsOverlay() }
        }
        topRow.addView(logo)
        topRow.addView(titleTxt)
        topRow.addView(permsShortcutBtn)
        topRow.addView(spaceH(8))
        topRow.addView(quickSettingsBtn)
        root.addView(topRow)

        hubStatusBanner = TextView(this).apply {
            text = "Pick a transport mode, tap an auto-discovered PC, or scan QR in Portrait mode"
            setTextColor(PlayCraftTheme.TEXT_WHITE)
            textSize = 11.5f
            background = cardDrawable(PlayCraftTheme.BG_SURFACE, PlayCraftTheme.BORDER_HAIRLINE, 10f * dp)
            setPadding((12 * dp).toInt(), (6 * dp).toInt(), (12 * dp).toInt(), (6 * dp).toInt())
        }
        root.addView(hubStatusBanner)
        root.addView(spaceV(5))

        hubUpdateBanner = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            background = cardDrawable(0x1F00E5FF, PlayCraftTheme.CYAN_GLOW, 10f * dp)
            setPadding((12 * dp).toInt(), (6 * dp).toInt(), (10 * dp).toInt(), (6 * dp).toInt())
            visibility = View.GONE
        }
        hubUpdateTitleText = TextView(this).apply {
            text = "Update available on GitHub"
            setTextColor(PlayCraftTheme.CYAN_GLOW)
            textSize = 12f
            typeface = Typeface.DEFAULT_BOLD
            layoutParams = LinearLayout.LayoutParams(0, -2, 1f)
        }
        val hubGetUpdateBtn = styledButton("Get Update", PlayCraftTheme.CYAN_GLOW, PlayCraftTheme.BG_OBSIDIAN).apply {
            textSize = 11f
            minHeight = 0
            minimumHeight = 0
            setPadding((12 * dp).toInt(), (5 * dp).toInt(), (12 * dp).toInt(), (5 * dp).toInt())
            setOnClickListener { openLatestGithubReleasePage() }
        }
        hubUpdateBanner.addView(hubUpdateTitleText)
        hubUpdateBanner.addView(spaceH(8))
        hubUpdateBanner.addView(hubGetUpdateBtn)
        root.addView(hubUpdateBanner)
        root.addView(spaceV(6))

        val body = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            layoutParams = LinearLayout.LayoutParams(-1, 0, 1f)
        }

        val leftScroll = ScrollView(this).apply {
            layoutParams = LinearLayout.LayoutParams(0, -1, 1.15f)
        }
        val leftCol = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            background = cardDrawable(PlayCraftTheme.BG_SURFACE, PlayCraftTheme.BORDER_HAIRLINE, 14f * dp)
            setPadding((14 * dp).toInt(), (10 * dp).toInt(), (14 * dp).toInt(), (10 * dp).toInt())
        }

        leftCol.addView(sectionTitle("1. CONNECTION TRANSPORT"))
        val tRow = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        transportButtons = linkedMapOf()
        val transports = listOf(
            PlayCraftTheme.TRANSPORT_WIFI to "Same Wi-Fi",
            PlayCraftTheme.TRANSPORT_HOTSPOT to "Hotspot",
            PlayCraftTheme.TRANSPORT_BLUETOOTH to "Bluetooth",
            PlayCraftTheme.TRANSPORT_USB to "USB Cable"
        )
        for ((mode, lbl) in transports) {
            val b = styledButton(lbl, PlayCraftTheme.BG_ELEVATED, PlayCraftTheme.TEXT_WHITE).apply {
                layoutParams = LinearLayout.LayoutParams(0, -2, 1f)
                textSize = 11f
                setPadding((4 * dp).toInt(), (8 * dp).toInt(), (4 * dp).toInt(), (8 * dp).toInt())
                setOnClickListener {
                    audio.playNavTick()
                    selectTransportMode(mode)
                }
            }
            transportButtons[mode] = b
            tRow.addView(b)
            tRow.addView(spaceH(5))
        }
        leftCol.addView(tRow)
        leftCol.addView(spaceV(6))

        val directRow = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        directRow.addView(miniSysButton("Wi-Fi Settings") { startActivitySafe(Intent(Settings.ACTION_WIFI_SETTINGS)) })
        directRow.addView(spaceH(5))
        directRow.addView(miniSysButton("Hotspot Settings") { openHotspotSettings() })
        directRow.addView(spaceH(5))
        directRow.addView(miniSysButton("BT Settings") { startActivitySafe(Intent(Settings.ACTION_BLUETOOTH_SETTINGS)) })
        leftCol.addView(directRow)
        leftCol.addView(spaceV(7))

        wifiFieldsPanel = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        val hpRow = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        hostEdit = styledInput("PC IP (e.g. 192.168.1.29)", config.savedHost).apply {
            layoutParams = LinearLayout.LayoutParams(0, -2, 2f)
        }
        portEdit = styledInput("Port", config.savedPort.toString()).apply {
            layoutParams = LinearLayout.LayoutParams(0, -2, 0.9f)
            inputType = InputType.TYPE_CLASS_NUMBER
        }
        hpRow.addView(hostEdit)
        hpRow.addView(spaceH(6))
        hpRow.addView(portEdit)
        wifiFieldsPanel.addView(hpRow)
        leftCol.addView(wifiFieldsPanel)

        btFieldsPanel = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            visibility = View.GONE
        }
        btFieldsPanel.addView(TextView(this).apply {
            text = "Pair phone with Windows PC in BT Settings once, then select your PC on the right."
            setTextColor(PlayCraftTheme.TEXT_MUTED)
            textSize = 11f
        })
        leftCol.addView(btFieldsPanel)
        leftCol.addView(spaceV(6))

        val pinRow = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        pinEdit = styledInput("4-Digit PIN (on PC)", config.savedPin).apply {
            layoutParams = LinearLayout.LayoutParams(0, -2, 1f)
            inputType = InputType.TYPE_CLASS_NUMBER
        }
        rememberSwitch = Switch(this).apply {
            text = "Remember Me  "
            setTextColor(PlayCraftTheme.TEXT_WHITE)
            textSize = 12f
            isChecked = config.rememberMe
            setOnCheckedChangeListener { _, checked ->
                config.rememberMe = checked
                if (!checked) config.hasSavedConnection = false
            }
        }
        pinRow.addView(pinEdit)
        pinRow.addView(spaceH(8))
        pinRow.addView(rememberSwitch)
        leftCol.addView(pinRow)
        leftCol.addView(spaceV(7))

        val actionRow = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        val qrBtn = styledButton("Scan QR (Portrait)", PlayCraftTheme.BG_ELEVATED, PlayCraftTheme.CYAN_GLOW).apply {
            layoutParams = LinearLayout.LayoutParams(0, -2, 1f)
            setOnClickListener { launchPortraitQrScanner() }
        }
        val connectBtn = styledButton("CONNECT TO PC", PlayCraftTheme.CYAN_GLOW, PlayCraftTheme.BG_OBSIDIAN).apply {
            layoutParams = LinearLayout.LayoutParams(0, -2, 1.25f)
            setOnClickListener { connectUsingCurrentInputs() }
        }
        actionRow.addView(qrBtn)
        actionRow.addView(spaceH(6))
        actionRow.addView(connectBtn)
        leftCol.addView(actionRow)

        leftScroll.addView(leftCol)
        body.addView(leftScroll)
        body.addView(spaceH(10))

        val rightScroll = ScrollView(this).apply {
            layoutParams = LinearLayout.LayoutParams(0, -1, 0.95f)
        }
        val rightCol = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            background = cardDrawable(PlayCraftTheme.BG_SURFACE, PlayCraftTheme.BORDER_HAIRLINE, 14f * dp)
            setPadding((14 * dp).toInt(), (10 * dp).toInt(), (14 * dp).toInt(), (10 * dp).toInt())
        }
        val scanHead = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        val discTitle = sectionTitle("2. AUTO-DISCOVERED PCs & BT DEVICES").apply {
            layoutParams = LinearLayout.LayoutParams(0, -2, 1f)
        }
        val refreshBtn = styledButton("Scan Now", PlayCraftTheme.CYAN_SOFT, PlayCraftTheme.CYAN_GLOW).apply {
            textSize = 11f
            setPadding((12 * dp).toInt(), (5 * dp).toInt(), (12 * dp).toInt(), (5 * dp).toInt())
            setOnClickListener {
                audio.playNavTick()
                scanAvailableTargets()
            }
        }
        scanHead.addView(discTitle)
        scanHead.addView(refreshBtn)
        rightCol.addView(scanHead)

        discoverListLayout = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        btDevicesLayout = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        rightCol.addView(discoverListLayout)
        rightCol.addView(spaceV(6))
        rightCol.addView(btDevicesLayout)

        rightScroll.addView(rightCol)
        body.addView(rightScroll)
        root.addView(body)

        selectTransportMode(config.transportMode)
        return root
    }

    private fun selectTransportMode(mode: String) {
        selectTransportModeNoScan(mode)
        if (mode == PlayCraftTheme.TRANSPORT_USB && hostEdit.text.isNullOrBlank()) {
            hostEdit.setText("127.0.0.1")
        }
        scanAvailableTargets()
    }

    private fun selectTransportModeNoScan(mode: String) {
        val dp = resources.displayMetrics.density
        config.transportMode = mode
        transportButtons.forEach { (m, btn) ->
            val active = m == mode
            btn.background = cardDrawable(
                if (active) PlayCraftTheme.CYAN_GLOW else PlayCraftTheme.BG_ELEVATED,
                if (active) PlayCraftTheme.CYAN_GLOW else PlayCraftTheme.BORDER_HAIRLINE,
                10f * dp
            )
            btn.setTextColor(if (active) PlayCraftTheme.BG_OBSIDIAN else PlayCraftTheme.TEXT_WHITE)
        }
        val isBt = mode == PlayCraftTheme.TRANSPORT_BLUETOOTH
        wifiFieldsPanel.visibility = if (isBt) View.GONE else View.VISIBLE
        btFieldsPanel.visibility = if (isBt) View.VISIBLE else View.GONE
    }

    private fun enterConnectionHubAndMaybeAutoConnect() {
        hubLayer.visibility = View.VISIBLE
        deckLayer.visibility = View.GONE
        selectedBtMac = config.savedBtMac
        selectedBtName = config.savedBtName
        scanAvailableTargets()

        if (!autoConnectTriggeredOnStart && config.rememberMe && config.hasSavedConnection && config.savedPin.isNotBlank()) {
            autoConnectTriggeredOnStart = true
            hubStatusBanner.text = "Remember Me active \u2022 Auto-connecting to ${config.savedPcName}..."
            mainHandler.postDelayed({
                if (hubLayer.visibility == View.VISIBLE) {
                    if (config.transportMode == PlayCraftTheme.TRANSPORT_BLUETOOTH && config.savedBtMac.isNotBlank()) {
                        bridge.connectBluetoothEndpoint(config.savedBtMac, config.savedBtName, config.savedPin)
                    } else if (config.savedHost.isNotBlank()) {
                        bridge.connectWebSocketEndpoint(
                            config.transportMode,
                            config.savedHost,
                            config.savedPort,
                            config.savedPin
                        )
                    }
                }
            }, 350L)
        }
    }

    @SuppressLint("MissingPermission")
    private fun scanAvailableTargets() {
        discoverListLayout.removeAllViews()
        btDevicesLayout.removeAllViews()

        discoverListLayout.addView(TextView(this).apply {
            text = "Scanning Wi-Fi LAN, Hotspot & USB endpoints..."
            setTextColor(PlayCraftTheme.TEXT_MUTED)
            textSize = 11.5f
        })

        thread {
            val found = bridge.discoverPlayCraftPcs(900)
            val btDevices = bridge.listPairedBluetoothDevices()
            mainHandler.post {
                discoverListLayout.removeAllViews()
                if (found.isEmpty()) {
                    discoverListLayout.addView(TextView(this).apply {
                        text = "No PC broadcast detected yet. Open PlayCraft on PC or tap Scan QR / Bluetooth."
                        setTextColor(PlayCraftTheme.TEXT_MUTED)
                        textSize = 11.5f
                        setPadding(0, 6, 0, 6)
                    })
                } else {
                    for (pc in found) {
                        for (ip in pc.ips) {
                            val btn = styledButton(
                                "${pc.pcName} \u2022 $ip:${pc.wsPort} ${if (pc.vigemActive) "(X360)" else ""}",
                                PlayCraftTheme.BG_ELEVATED,
                                PlayCraftTheme.CYAN_GLOW
                            ).apply {
                                layoutParams = LinearLayout.LayoutParams(-1, -2).apply { bottomMargin = 6 }
                                gravity = Gravity.START or Gravity.CENTER_VERTICAL
                                textSize = 11.5f
                                setOnClickListener {
                                    audio.playNavTick()
                                    hostEdit.setText(ip)
                                    portEdit.setText(pc.wsPort.toString())
                                    config.savedPcName = pc.pcName
                                    if (ip == "127.0.0.1") selectTransportModeNoScan(PlayCraftTheme.TRANSPORT_USB)
                                    else if (ip.startsWith("192.168.43.") || ip.startsWith("192.168.137.")) selectTransportModeNoScan(PlayCraftTheme.TRANSPORT_HOTSPOT)
                                    else selectTransportModeNoScan(PlayCraftTheme.TRANSPORT_WIFI)
                                    if (!pinEdit.text.isNullOrBlank()) connectUsingCurrentInputs()
                                    else toast("PC Selected — Enter PC PIN & tap CONNECT")
                                }
                            }
                            discoverListLayout.addView(btn)
                        }
                    }
                }

                btDevicesLayout.removeAllViews()
                if (btDevices.isNotEmpty()) {
                    btDevicesLayout.addView(sectionTitle("PAIRED BLUETOOTH DEVICES (DIRECT SPP)"))
                    for (dev in btDevices) {
                        val name = runCatching { dev.name }.getOrNull() ?: dev.address
                        val btBtn = styledButton(
                            "BT: $name (${dev.address})",
                            if (dev.address == selectedBtMac) PlayCraftTheme.CYAN_SOFT else PlayCraftTheme.BG_ELEVATED,
                            PlayCraftTheme.TEXT_WHITE
                        ).apply {
                            layoutParams = LinearLayout.LayoutParams(-1, -2).apply { topMargin = 6 }
                            gravity = Gravity.START or Gravity.CENTER_VERTICAL
                            textSize = 11f
                            setOnClickListener {
                                audio.playNavTick()
                                selectedBtMac = dev.address
                                selectedBtName = name
                                selectTransportModeNoScan(PlayCraftTheme.TRANSPORT_BLUETOOTH)
                                if (!pinEdit.text.isNullOrBlank()) connectUsingCurrentInputs()
                                else toast("Selected BT '$name' — Enter PC PIN & tap CONNECT")
                            }
                        }
                        btDevicesLayout.addView(btBtn)
                    }
                }
            }
        }
    }

    private fun connectUsingCurrentInputs() {
        val pin = pinEdit.text?.toString()?.trim().orEmpty()
        if (pin.length < 4) {
            toast("Enter the 4-digit PIN displayed on PlayCraft Desktop")
            return
        }
        val mode = config.transportMode
        if (mode == PlayCraftTheme.TRANSPORT_BLUETOOTH) {
            if (selectedBtMac.isBlank()) {
                toast("Select a paired Bluetooth PC on the right first")
                return
            }
            bridge.connectBluetoothEndpoint(selectedBtMac, selectedBtName, pin)
        } else {
            val host = hostEdit.text?.toString()?.trim().orEmpty()
            val port = portEdit.text?.toString()?.toIntOrNull() ?: PlayCraftTheme.DEFAULT_WS_PORT
            if (host.isBlank()) {
                toast("Tap Scan Now or enter your PC IP address")
                return
            }
            bridge.connectWebSocketEndpoint(mode, host, port, pin)
        }
    }

    private fun handleScannedPairingUri(raw: String) {
        runCatching {
            val uri = Uri.parse(raw.trim())
            if (uri.scheme == "playcraft") {
                val ips = (uri.getQueryParameter("ips") ?: "").split(",").filter { it.isNotBlank() }
                val port = uri.getQueryParameter("port")?.toIntOrNull() ?: PlayCraftTheme.DEFAULT_WS_PORT
                val pin = uri.getQueryParameter("pin") ?: ""
                if (ips.isNotEmpty() && pin.isNotBlank()) {
                    hostEdit.setText(ips.first())
                    portEdit.setText(port.toString())
                    pinEdit.setText(pin)
                    bridge.connectWebSocketEndpoint(PlayCraftTheme.TRANSPORT_WIFI, ips.first(), port, pin)
                }
            } else {
                toast("Scanned code is not a PlayCraft pairing QR")
            }
        }
    }

    private fun parseIncomingPairIntent(incoming: Intent?) {
        val uri = incoming?.data ?: return
        if (uri.scheme == "playcraft") handleScannedPairingUri(uri.toString())
    }

    private fun launchPortraitQrScanner() {
        if (ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED) {
            AlertDialog.Builder(this, android.R.style.Theme_DeviceDefault_Dialog_Alert)
                .setTitle("Camera Permission Required")
                .setMessage("Allow Camera permission in App Settings to scan the PlayCraft QR code.")
                .setPositiveButton("Open App Settings") { _, _ -> openAndroidAppSettings() }
                .setNegativeButton("Cancel", null)
                .show()
            return
        }
        @Suppress("DEPRECATION")
        startActivityForResult(Intent(this, QrScanActivity::class.java), REQ_QR_SCAN)
    }

    @Deprecated("Handled cleanly for Activity result without extra AppCompat dependency")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == REQ_QR_SCAN && resultCode == Activity.RESULT_OK) {
            val scanned = data?.getStringExtra("qr_result") ?: return
            handleScannedPairingUri(scanned)
        }
    }

    // ================== MODERN RESPONSIVE TOP BAR + ACTIVE DECK ==================

    private fun buildActiveDeckLayer(): LinearLayout {
        val dp = resources.displayMetrics.density
        val shell = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(PlayCraftTheme.BG_OBSIDIAN)
        }

        val bar = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setBackgroundColor(0xEB070B14.toInt())
            setPadding((10 * dp).toInt(), (5 * dp).toInt(), (10 * dp).toInt(), (5 * dp).toInt())
        }

        val leftScroll = HorizontalScrollView(this).apply {
            isHorizontalScrollBarEnabled = false
            layoutParams = LinearLayout.LayoutParams(0, -2, 1f)
        }
        val tabsRow = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }

        padTabBtn = miniTabBtn("Pad") { setDeckMode(PlayCraftTheme.DECK_PAD) }
        wheelTabBtn = miniTabBtn("Wheel") { setDeckMode(PlayCraftTheme.DECK_WHEEL) }
        ps5TabBtn = miniTabBtn("PS5") { setShellTheme(PlayCraftTheme.SHELL_PS5) }
        xboxTabBtn = miniTabBtn("Xbox") { setShellTheme(PlayCraftTheme.SHELL_XBOX) }
        x360TabBtn = miniTabBtn("X360") { setOutputDriver(PlayCraftTheme.DRIVER_X360) }
        keysTabBtn = miniTabBtn("Keys") { setOutputDriver(PlayCraftTheme.DRIVER_KEYS) }

        topUpdateBtn = miniTabBtn("Update") { openLatestGithubReleasePage() }.apply {
            visibility = View.GONE
            background = cardDrawable(0x2800E5FF, PlayCraftTheme.CYAN_GLOW, 9f * dp)
            setTextColor(PlayCraftTheme.CYAN_GLOW)
        }
        topSettingsBtn = miniTabBtn("Settings") { showControllerSettingsOverlay() }
        topHubBtn = miniTabBtn("Hub / Link") {
            audio.playNavTick()
            bridge.disconnectManually()
            gyroEngine.stop()
            deckLayer.visibility = View.GONE
            hubLayer.visibility = View.VISIBLE
        }

        tabsRow.addView(padTabBtn); tabsRow.addView(spaceH(4))
        tabsRow.addView(wheelTabBtn); tabsRow.addView(spaceH(8))
        tabsRow.addView(ps5TabBtn); tabsRow.addView(spaceH(4))
        tabsRow.addView(xboxTabBtn); tabsRow.addView(spaceH(8))
        tabsRow.addView(x360TabBtn); tabsRow.addView(spaceH(4))
        tabsRow.addView(keysTabBtn); tabsRow.addView(spaceH(8))
        tabsRow.addView(topUpdateBtn); tabsRow.addView(spaceH(5))
        tabsRow.addView(topSettingsBtn); tabsRow.addView(spaceH(5))
        tabsRow.addView(topHubBtn)
        leftScroll.addView(tabsRow)

        val statusWrap = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            background = cardDrawable(0x1810B981, PlayCraftTheme.GAS_GREEN, 99f * dp)
            setPadding((10 * dp).toInt(), (4 * dp).toInt(), (12 * dp).toInt(), (4 * dp).toInt())
        }
        statusDotView = View(this).apply {
            val d = (8 * dp).toInt()
            layoutParams = LinearLayout.LayoutParams(d, d)
            background = GradientDrawable().apply { shape = GradientDrawable.OVAL; setColor(PlayCraftTheme.GAS_GREEN) }
        }
        statusTextView = TextView(this).apply {
            text = " Connected"
            setTextColor(PlayCraftTheme.GAS_GREEN)
            textSize = 11f
            typeface = Typeface.DEFAULT_BOLD
        }
        statusWrap.addView(statusDotView)
        statusWrap.addView(statusTextView)

        bar.addView(leftScroll)
        bar.addView(spaceH(8))
        bar.addView(statusWrap)

        shell.addView(bar, LinearLayout.LayoutParams(-1, -2))

        deckContainer = FrameLayout(this).apply {
            layoutParams = LinearLayout.LayoutParams(-1, 0, 1f)
        }
        deckContainer.addView(gamepadDeck, FrameLayout.LayoutParams(-1, -1))
        deckContainer.addView(wheelDeck, FrameLayout.LayoutParams(-1, -1))
        shell.addView(deckContainer)

        refreshDeckHeaderState()
        return shell
    }

    private fun setDeckMode(mode: String) {
        audio.playNavTick()
        config.deckMode = mode
        refreshDeckHeaderState()
        if (mode == PlayCraftTheme.DECK_WHEEL) {
            gyroEngine.start()
            gyroEngine.calibrateZero()
        } else {
            gyroEngine.stop()
            bridge.sendGyroSteer(0f, 0f)
        }
    }

    private fun setShellTheme(shell: String) {
        audio.playNavTick()
        config.shellTheme = shell
        refreshDeckHeaderState()
        wheelDeck.invalidate()
        gamepadDeck.invalidate()
    }

    private fun setOutputDriver(driver: String) {
        audio.playNavTick()
        config.outputDriver = driver
        refreshDeckHeaderState()
    }

    private fun refreshDeckHeaderState() {
        val dp = resources.displayMetrics.density
        val accent = config.activeAccentColor()
        styleSegment(padTabBtn, config.deckMode == PlayCraftTheme.DECK_PAD, accent, dp)
        styleSegment(wheelTabBtn, config.deckMode == PlayCraftTheme.DECK_WHEEL, accent, dp)
        styleSegment(ps5TabBtn, config.shellTheme == PlayCraftTheme.SHELL_PS5, accent, dp)
        styleSegment(xboxTabBtn, config.shellTheme == PlayCraftTheme.SHELL_XBOX, accent, dp)
        styleSegment(x360TabBtn, config.outputDriver == PlayCraftTheme.DRIVER_X360, accent, dp)
        styleSegment(keysTabBtn, config.outputDriver == PlayCraftTheme.DRIVER_KEYS, accent, dp)

        topSettingsBtn.background = cardDrawable(0xFF162036.toInt(), PlayCraftTheme.BORDER_HAIRLINE, 9f * dp)
        topSettingsBtn.setTextColor(PlayCraftTheme.CYAN_GLOW)
        topHubBtn.background = cardDrawable(0xFF162036.toInt(), PlayCraftTheme.BORDER_HAIRLINE, 9f * dp)
        topHubBtn.setTextColor(PlayCraftTheme.TEXT_WHITE)

        val isWheel = config.deckMode == PlayCraftTheme.DECK_WHEEL
        wheelDeck.visibility = if (isWheel) View.VISIBLE else View.GONE
        gamepadDeck.visibility = if (isWheel) View.GONE else View.VISIBLE
    }

    private fun styleSegment(btn: Button, active: Boolean, accent: Int, dp: Float) {
        btn.background = cardDrawable(
            if (active) accent else 0xFF121A2E.toInt(),
            if (active) accent else PlayCraftTheme.BORDER_HAIRLINE,
            9f * dp
        )
        btn.setTextColor(if (active) PlayCraftTheme.BG_OBSIDIAN else PlayCraftTheme.TEXT_WHITE)
    }

    // ================== MODERN RESPONSIVE IN-APP SETTINGS SHEET ==================

    private fun showControllerSettingsOverlay() {
        val dp = resources.displayMetrics.density
        settingsOverlay.removeAllViews()
        settingsOverlay.setBackgroundColor(0xC803060E.toInt())
        settingsOverlay.isClickable = true
        settingsOverlay.setOnClickListener { settingsOverlay.visibility = View.GONE }

        val cardW = (resources.displayMetrics.widthPixels * 0.86f).toInt().coerceAtMost((760 * dp).toInt())
        val cardH = (resources.displayMetrics.heightPixels * 0.90f).toInt()

        val card = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            background = cardDrawable(PlayCraftTheme.BG_SURFACE, PlayCraftTheme.CYAN_GLOW, 18f * dp)
            setPadding((20 * dp).toInt(), (16 * dp).toInt(), (20 * dp).toInt(), (14 * dp).toInt())
            isClickable = true
        }

        // Top Header + Save/Close button (never clipped!)
        val hdrRow = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        val titleBlock = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = LinearLayout.LayoutParams(0, -2, 1f)
        }
        titleBlock.addView(TextView(this).apply {
            text = "PlayCraft Controller Settings & Racing Tuning"
            setTextColor(PlayCraftTheme.CYAN_GLOW)
            textSize = 16.5f
            typeface = Typeface.DEFAULT_BOLD
        })
        titleBlock.addView(TextView(this).apply {
            text = "Sensor: ${gyroEngine.sensorStatusLabel}  \u2022  Rashmina Fernando (www.rashmina.dev)"
            setTextColor(PlayCraftTheme.TEXT_MUTED)
            textSize = 11f
        })
        val closeSheetBtn = styledButton("Done", PlayCraftTheme.CYAN_GLOW, PlayCraftTheme.BG_OBSIDIAN).apply {
            setOnClickListener {
                audio.playNavTick()
                settingsOverlay.visibility = View.GONE
            }
        }
        hdrRow.addView(titleBlock)
        hdrRow.addView(closeSheetBtn)
        card.addView(hdrRow)
        card.addView(spaceV(8))

        val scroll = ScrollView(this).apply {
            layoutParams = LinearLayout.LayoutParams(-1, 0, 1f)
        }
        val col = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }

        col.addView(settingSwitch("Remember Me & Auto-Connect on Startup", config.rememberMe) {
            config.rememberMe = it
            if (!it) config.hasSavedConnection = false
        })
        col.addView(settingSwitch("Play Intro Video on App Launch", config.playBootIntro) { config.playBootIntro = it })
        col.addView(settingSwitch("PS5 / Xbox Haptic Rumble Feedback", config.hapticsEnabled) { config.hapticsEnabled = it })
        col.addView(settingSwitch("PS5 UI Click Audio Effects", config.uiSoundsEnabled) { config.uiSoundsEnabled = it })
        col.addView(settingSwitch("Keep Screen Always Awake While Playing", config.keepScreenAwake) {
            config.keepScreenAwake = it; updateScreenAwakeFlag()
        })
        col.addView(settingSwitch("Progressive Slide Pedals (vs Instant 100%)", config.progressivePedals) { config.progressivePedals = it })
        col.addView(settingSwitch("Swap Brake (Left) & Gas (Right) Sides", config.swapPedalSides) {
            config.swapPedalSides = it
            wheelDeck.requestLayout()
            wheelDeck.invalidate()
        })
        col.addView(settingSwitch("Invert Gyro Steering Axis", config.gyroInvert) { config.gyroInvert = it })

        // Steering Input Mode + Test Rumble Row
        val modeTestRow = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            setPadding(0, (8 * dp).toInt(), 0, (8 * dp).toInt())
        }
        val modeBtn = styledButton("Steering Mode: ${config.steerInputMode}", PlayCraftTheme.BG_ELEVATED, PlayCraftTheme.CYAN_GLOW).apply {
            layoutParams = LinearLayout.LayoutParams(0, -2, 1f)
            setOnClickListener {
                config.steerInputMode = when (config.steerInputMode) {
                    PlayCraftTheme.STEER_HYBRID -> PlayCraftTheme.STEER_GYRO_ONLY
                    PlayCraftTheme.STEER_GYRO_ONLY -> PlayCraftTheme.STEER_TOUCH_ONLY
                    else -> PlayCraftTheme.STEER_HYBRID
                }
                text = "Steering Mode: ${config.steerInputMode}"
            }
        }
        val testRumbleBtn = styledButton("Test Rumble", PlayCraftTheme.BG_ELEVATED, PlayCraftTheme.GAS_GREEN).apply {
            layoutParams = LinearLayout.LayoutParams(0, -2, 0.7f)
            setOnClickListener {
                audio.triggerGameRumble(230, 180)
                wheelDeck.setRumbleVisual(230, 180)
                mainHandler.postDelayed({ wheelDeck.setRumbleVisual(0, 0) }, 260L)
            }
        }
        modeTestRow.addView(modeBtn); modeTestRow.addView(spaceH(8)); modeTestRow.addView(testRumbleBtn)
        col.addView(modeTestRow)

        col.addView(settingSlider("Game Rumble Intensity (Forza / PC FFB)", 0, 100, config.gameRumbleScalePct, "%") {
            config.gameRumbleScalePct = it
        })
        col.addView(settingSlider("Max Steering Turn Angle", 20, 90, config.gyroMaxAngleDeg, "\u00B0") {
            config.gyroMaxAngleDeg = it
        })
        col.addView(settingSlider("Center Deadzone", 0, 15, config.gyroDeadzonePct, "%") {
            config.gyroDeadzonePct = it
        })
        col.addView(settingSlider("Steering Linearity Gamma (100 = 1.0x)", 70, 200, (config.gyroGamma * 100).toInt(), "%") {
            config.gyroGamma = it / 100f
        })
        col.addView(settingSlider("Gyro Filter Responsiveness", 20, 85, (config.gyroSmoothing * 100).toInt(), "%") {
            config.gyroSmoothing = it / 100f
        })

        col.addView(spaceV(10))
        col.addView(sectionTitle("DIRECT ANDROID SETTINGS REDIRECTS"))
        val redRow1 = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        redRow1.addView(miniSysButton("App Permissions") { openAndroidAppSettings() }); redRow1.addView(spaceH(6))
        redRow1.addView(miniSysButton("Wi-Fi Settings") { startActivitySafe(Intent(Settings.ACTION_WIFI_SETTINGS)) }); redRow1.addView(spaceH(6))
        redRow1.addView(miniSysButton("Hotspot Settings") { openHotspotSettings() })
        col.addView(redRow1)

        col.addView(spaceV(6))
        val redRow2 = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        redRow2.addView(miniSysButton("Bluetooth Settings") { startActivitySafe(Intent(Settings.ACTION_BLUETOOTH_SETTINGS)) }); redRow2.addView(spaceH(6))
        redRow2.addView(miniSysButton("Battery Optimization") { openBatteryOptimizationSettings() }); redRow2.addView(spaceH(6))
        redRow2.addView(miniSysButton("Edit Key Map") { showKeymapEditorDialog() })
        col.addView(redRow2)

        col.addView(spaceV(12))
        col.addView(sectionTitle("APP UPDATES (GITHUB RELEASES)"))
        val verName = AppUpdater.currentVersion(this)
        settingsUpdateStatusView = TextView(this).apply {
            val cached = latestUpdateInfo
            text = when {
                cached?.hasUpdate == true -> "Installed v$verName  |  Update ${cached.latestVersion} Ready!"
                cached != null && !cached.offlineOrError -> "Installed v$verName  |  ${cached.statusMessage}"
                else -> "Installed v$verName  |  Checks daily in background when online"
            }
            setTextColor(if (cached?.hasUpdate == true) PlayCraftTheme.CYAN_GLOW else PlayCraftTheme.TEXT_MUTED)
            textSize = 11.5f
            setPadding(0, (2 * dp).toInt(), 0, (6 * dp).toInt())
        }
        col.addView(settingsUpdateStatusView)
        val updRow = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        val checkUpdBtn = styledButton("Check for Updates", PlayCraftTheme.BG_ELEVATED, PlayCraftTheme.CYAN_GLOW).apply {
            layoutParams = LinearLayout.LayoutParams(0, -2, 1f)
            setOnClickListener {
                audio.playNavTick()
                settingsUpdateStatusView?.text = "Installed v$verName  |  Checking GitHub..."
                triggerSilentDailyUpdateCheck(force = true)
            }
        }
        val githubRelBtn = styledButton("Latest GitHub Release", PlayCraftTheme.CYAN_SOFT, PlayCraftTheme.CYAN_GLOW).apply {
            layoutParams = LinearLayout.LayoutParams(0, -2, 1f)
            setOnClickListener { openLatestGithubReleasePage() }
        }
        updRow.addView(checkUpdBtn); updRow.addView(spaceH(8)); updRow.addView(githubRelBtn)
        col.addView(updRow)
        col.addView(spaceV(12))

        scroll.addView(col)
        card.addView(scroll)

        settingsOverlay.addView(card, FrameLayout.LayoutParams(cardW, cardH, Gravity.CENTER))
        settingsOverlay.visibility = View.VISIBLE
    }

    private fun showKeymapEditorDialog() {
        val scroll = ScrollView(this)
        val col = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(PlayCraftTheme.BG_SURFACE)
            setPadding(24, 20, 24, 20)
        }
        val currentMap = config.loadKeymap()
        val edits = mutableMapOf<String, EditText>()
        for ((btnName, keyVal) in currentMap) {
            val row = LinearLayout(this).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
            }
            val lbl = TextView(this).apply {
                text = btnName
                setTextColor(PlayCraftTheme.TEXT_WHITE)
                layoutParams = LinearLayout.LayoutParams(0, -2, 1f)
            }
            val inp = styledInput(keyVal, keyVal).apply {
                layoutParams = LinearLayout.LayoutParams(0, -2, 1f)
            }
            edits[btnName] = inp
            row.addView(lbl)
            row.addView(inp)
            col.addView(row)
        }
        scroll.addView(col)
        AlertDialog.Builder(this, android.R.style.Theme_DeviceDefault_Dialog_Alert)
            .setTitle("PC Keyboard Fallback Mapping")
            .setView(scroll)
            .setPositiveButton("Save Keys") { _, _ ->
                val updated = mutableMapOf<String, String>()
                edits.forEach { (k, e) ->
                    val v = e.text?.toString()?.trim()?.uppercase().orEmpty()
                    if (v.isNotEmpty()) updated[k] = v
                }
                config.saveKeymap(updated)
                toast("Saved keyboard bindings")
            }
            .setNegativeButton("Cancel", null)
            .show()
    }

    // ================== BRIDGE STATE & LIVE RUMBLE CALLBACKS ==================

    override fun onConnectionStateChanged(
        state: ConnectionState,
        transportLabel: String,
        detail: String,
        vigemActive: Boolean,
        pingMs: Int
    ) {
        mainHandler.post {
            hubStatusBanner.text = "$transportLabel: $detail"
            when (state) {
                ConnectionState.CONNECTED -> {
                    if (deckLayer.visibility != View.VISIBLE) {
                        audio.playConnectChime()
                        hubLayer.visibility = View.GONE
                        deckLayer.visibility = View.VISIBLE
                        refreshDeckHeaderState()
                        if (config.deckMode == PlayCraftTheme.DECK_WHEEL) {
                            gyroEngine.start()
                            gyroEngine.calibrateZero()
                        }
                    }
                    if (!vigemActive && config.outputDriver == PlayCraftTheme.DRIVER_X360) {
                        config.outputDriver = PlayCraftTheme.DRIVER_KEYS
                        refreshDeckHeaderState()
                    }
                    val msLabel = if (pingMs > 0) " \u2022 ${pingMs}ms" else ""
                    val drv = if (config.outputDriver == PlayCraftTheme.DRIVER_X360) "X360" else "Keys"
                    statusTextView.text = " $drv \u2022 $transportLabel$msLabel"
                    statusTextView.setTextColor(PlayCraftTheme.GAS_GREEN)
                    (statusDotView.background as? GradientDrawable)?.setColor(PlayCraftTheme.GAS_GREEN)
                }

                ConnectionState.CONNECTING, ConnectionState.RECONNECTING -> {
                    statusTextView.text = " $detail"
                    statusTextView.setTextColor(PlayCraftTheme.WARN_AMBER)
                    (statusDotView.background as? GradientDrawable)?.setColor(PlayCraftTheme.WARN_AMBER)
                }

                ConnectionState.AUTH_ERROR -> {
                    audio.playAlertTone()
                    audio.stopRumble()
                    gyroEngine.stop()
                    deckLayer.visibility = View.GONE
                    hubLayer.visibility = View.VISIBLE
                    statusTextView.text = " PIN Error"
                    statusTextView.setTextColor(PlayCraftTheme.BRAKE_RED)
                    (statusDotView.background as? GradientDrawable)?.setColor(PlayCraftTheme.BRAKE_RED)
                    toast(detail)
                }

                ConnectionState.DISCONNECTED -> {
                    audio.stopRumble()
                    gyroEngine.stop()
                    statusTextView.text = " Offline"
                    statusTextView.setTextColor(PlayCraftTheme.TEXT_MUTED)
                    (statusDotView.background as? GradientDrawable)?.setColor(PlayCraftTheme.BRAKE_RED)
                }
            }
        }
    }

    override fun onGameRumble(largeMotor: Int, smallMotor: Int) {
        audio.triggerGameRumble(largeMotor, smallMotor)
        if (config.deckMode == PlayCraftTheme.DECK_WHEEL) {
            wheelDeck.setRumbleVisual(largeMotor, smallMotor)
        }
    }

    // ================== LIFECYCLE & NETWORK WATCHERS ==================

    private fun triggerSilentDailyUpdateCheck(force: Boolean) {
        AppUpdater.checkForUpdate(this, config, force) { info ->
            mainHandler.post {
                latestUpdateInfo = info
                if (info.hasUpdate) {
                    if (::hubUpdateBanner.isInitialized) {
                        hubUpdateTitleText.text = "PlayCraft ${info.latestVersion} Update Has Arrived! (Installed v${info.currentVersion})"
                        hubUpdateBanner.visibility = View.VISIBLE
                    }
                    if (::topUpdateBtn.isInitialized) {
                        topUpdateBtn.text = "Update ${info.latestVersion}"
                        topUpdateBtn.visibility = View.VISIBLE
                    }
                    settingsUpdateStatusView?.apply {
                        text = "Installed v${info.currentVersion}  |  Update ${info.latestVersion} Available!"
                        setTextColor(PlayCraftTheme.CYAN_GLOW)
                    }
                    if (force) toast("Update ${info.latestVersion} found on GitHub!")
                } else {
                    if (::hubUpdateBanner.isInitialized) hubUpdateBanner.visibility = View.GONE
                    if (::topUpdateBtn.isInitialized) topUpdateBtn.visibility = View.GONE
                    settingsUpdateStatusView?.apply {
                        text = "Installed v${info.currentVersion}  |  ${info.statusMessage}"
                        setTextColor(PlayCraftTheme.TEXT_MUTED)
                    }
                    if (force) {
                        toast(info.statusMessage)
                    }
                }
            }
        }
    }

    private fun openLatestGithubReleasePage() {
        audio.playNavTick()
        val url = latestUpdateInfo?.releaseUrl?.takeIf { it.startsWith("https://") }
            ?: config.latestReleaseUrl.takeIf { it.startsWith("https://") }
            ?: AppUpdater.RELEASES_PAGE_URL
        startActivitySafe(Intent(Intent.ACTION_VIEW, Uri.parse(url)))
    }

    private fun registerNetworkWatcher() {
        val cm = getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager ?: return
        val cb = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                bridge.triggerImmediateReconnectIfWanted()
                triggerSilentDailyUpdateCheck(force = false)
            }
        }
        networkCallback = cb
        runCatching { cm.registerDefaultNetworkCallback(cb) }
    }

    override fun onResume() {
        super.onResume()
        applyImmersiveGameWindow()
        if (deckLayer.visibility == View.VISIBLE && config.deckMode == PlayCraftTheme.DECK_WHEEL) {
            gyroEngine.start()
        }
        bridge.triggerImmediateReconnectIfWanted()
    }

    override fun onPause() {
        super.onPause()
        audio.stopRumble()
        gyroEngine.stop()
        bridge.sendNeutralImmediate()
    }

    @Deprecated("Handled cleanly for Activity without extra AppCompat dependency")
    override fun onBackPressed() {
        if (::settingsOverlay.isInitialized && settingsOverlay.visibility == View.VISIBLE) {
            settingsOverlay.visibility = View.GONE
            return
        }
        if (deckLayer.visibility == View.VISIBLE) {
            bridge.disconnectManually()
            gyroEngine.stop()
            deckLayer.visibility = View.GONE
            hubLayer.visibility = View.VISIBLE
            return
        }
        @Suppress("DEPRECATION")
        super.onBackPressed()
    }

    override fun onDestroy() {
        mainHandler.removeCallbacks(dailyUpdateCheckRunnable)
        networkCallback?.let { cb ->
            runCatching {
                (getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager).unregisterNetworkCallback(cb)
            }
        }
        runCatching { bootMediaPlayer?.release() }
        gyroEngine.stop()
        bridge.release()
        audio.release()
        super.onDestroy()
    }

    // ================== ANDROID SYSTEM SETTINGS REDIRECTS ==================

    private fun openAndroidAppSettings() {
        val i = Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS).apply {
            data = Uri.fromParts("package", packageName, null)
        }
        startActivitySafe(i)
    }

    private fun openHotspotSettings() {
        val tetherIntent = Intent("android.settings.TETHER_SETTINGS")
        if (tetherIntent.resolveActivity(packageManager) != null) {
            startActivitySafe(tetherIntent)
        } else {
            startActivitySafe(Intent(Settings.ACTION_WIRELESS_SETTINGS))
        }
    }

    private fun openBatteryOptimizationSettings() {
        val i = Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS)
        if (i.resolveActivity(packageManager) != null) {
            startActivitySafe(i)
        } else {
            openAndroidAppSettings()
        }
    }

    private fun startActivitySafe(i: Intent) {
        runCatching { startActivity(i) }.onFailure { toast("Cannot open that settings page on this device") }
    }

    // ================== UI HELPER FACTORIES ==================

    private fun toast(msg: String) {
        mainHandler.post { Toast.makeText(this, msg, Toast.LENGTH_SHORT).show() }
    }

    private fun sectionTitle(title: String) = TextView(this).apply {
        text = title
        setTextColor(PlayCraftTheme.CYAN_GLOW)
        textSize = 11f
        typeface = Typeface.DEFAULT_BOLD
        setPadding(0, 4, 0, 5)
    }

    private fun styledButton(label: String, bg: Int, fg: Int): Button {
        val dp = resources.displayMetrics.density
        return Button(this).apply {
            text = label
            isAllCaps = false
            setTextColor(fg)
            typeface = Typeface.DEFAULT_BOLD
            textSize = 12f
            minHeight = 0
            minimumHeight = 0
            minWidth = 0
            minimumWidth = 0
            stateListAnimator = null
            background = cardDrawable(bg, PlayCraftTheme.BORDER_HAIRLINE, 11f * dp)
            setPadding((14 * dp).toInt(), (8 * dp).toInt(), (14 * dp).toInt(), (8 * dp).toInt())
        }
    }

    private fun miniSysButton(label: String, onClick: () -> Unit): Button {
        val dp = resources.displayMetrics.density
        return Button(this).apply {
            text = label
            isAllCaps = false
            setTextColor(PlayCraftTheme.CYAN_GLOW)
            textSize = 10.5f
            minHeight = 0
            minimumHeight = 0
            minWidth = 0
            minimumWidth = 0
            stateListAnimator = null
            layoutParams = LinearLayout.LayoutParams(0, -2, 1f)
            background = cardDrawable(PlayCraftTheme.BG_ELEVATED, PlayCraftTheme.CYAN_SOFT, 9f * dp)
            setPadding((6 * dp).toInt(), (8 * dp).toInt(), (6 * dp).toInt(), (8 * dp).toInt())
            setOnClickListener { audio.playNavTick(); onClick() }
        }
    }

    private fun miniTabBtn(label: String, onClick: () -> Unit): Button {
        val dp = resources.displayMetrics.density
        return Button(this).apply {
            text = label
            isAllCaps = false
            textSize = 11f
            typeface = Typeface.DEFAULT_BOLD
            minHeight = 0
            minimumHeight = 0
            minWidth = 0
            minimumWidth = 0
            stateListAnimator = null
            setPadding((13 * dp).toInt(), (6 * dp).toInt(), (13 * dp).toInt(), (6 * dp).toInt())
            setOnClickListener { onClick() }
        }
    }

    private fun styledInput(hintText: String, initial: String): EditText {
        val dp = resources.displayMetrics.density
        return EditText(this).apply {
            hint = hintText
            setText(initial)
            setTextColor(PlayCraftTheme.TEXT_WHITE)
            setHintTextColor(PlayCraftTheme.TEXT_MUTED)
            textSize = 12.5f
            background = cardDrawable(PlayCraftTheme.BG_OBSIDIAN, PlayCraftTheme.BORDER_HAIRLINE, 9f * dp)
            setPadding((12 * dp).toInt(), (8 * dp).toInt(), (12 * dp).toInt(), (8 * dp).toInt())
            isSingleLine = true
        }
    }

    private fun settingSwitch(title: String, initial: Boolean, onChange: (Boolean) -> Unit): Switch {
        return Switch(this).apply {
            text = title
            setTextColor(PlayCraftTheme.TEXT_WHITE)
            textSize = 13f
            isChecked = initial
            setPadding(0, 10, 0, 10)
            setOnCheckedChangeListener { _, isChecked -> audio.playNavTick(); onChange(isChecked) }
        }
    }

    private fun settingSlider(
        title: String,
        minVal: Int,
        maxVal: Int,
        current: Int,
        suffix: String,
        onChange: (Int) -> Unit
    ): LinearLayout {
        val box = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(0, 8, 0, 6) }
        val lbl = TextView(this).apply {
            text = "$title: $current$suffix"
            setTextColor(PlayCraftTheme.TEXT_WHITE)
            textSize = 12.5f
        }
        val sb = SeekBar(this).apply {
            max = maxVal - minVal
            progress = (current - minVal).coerceIn(0, max)
            setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
                override fun onProgressChanged(s: SeekBar?, p: Int, fromUser: Boolean) {
                    val v = minVal + p
                    lbl.text = "$title: $v$suffix"
                    onChange(v)
                }
                override fun onStartTrackingTouch(s: SeekBar?) {}
                override fun onStopTrackingTouch(s: SeekBar?) {}
            })
        }
        box.addView(lbl)
        box.addView(sb)
        return box
    }

    private fun cardDrawable(fill: Int, stroke: Int, radiusPx: Float): GradientDrawable {
        return GradientDrawable().apply {
            setColor(fill)
            setStroke(2, stroke)
            cornerRadius = radiusPx
        }
    }

    private fun spaceH(dpVal: Int) = View(this).apply {
        layoutParams = LinearLayout.LayoutParams((dpVal * resources.displayMetrics.density).toInt(), 1)
    }

    private fun spaceV(dpVal: Int) = View(this).apply {
        layoutParams = LinearLayout.LayoutParams(1, (dpVal * resources.displayMetrics.density).toInt())
    }
}
