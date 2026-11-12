package dev.rashmina.playcraft.controller

import android.annotation.SuppressLint
import android.app.Activity
import android.content.Context
import android.content.Intent
import android.graphics.Matrix
import android.graphics.RectF
import android.graphics.SurfaceTexture
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CameraMetadata
import android.hardware.camera2.CaptureRequest
import android.media.ImageReader
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.util.Size
import android.view.Gravity
import android.view.Surface
import android.view.TextureView
import android.widget.Button
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import com.google.zxing.BinaryBitmap
import com.google.zxing.MultiFormatReader
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.common.HybridBinarizer
import kotlin.math.abs
import kotlin.math.max

/**
 * Dedicated Portrait QR Scanner Activity with proper Camera2 sensor rotation & center-crop transform
 * so QR scanning matches the phone screen aspect ratio with zero stretching.
 */
class QrScanActivity : Activity() {

    private lateinit var textureView: TextureView
    private var cameraDevice: CameraDevice? = null
    private var captureSession: CameraCaptureSession? = null
    private var previewBuilder: CaptureRequest.Builder? = null
    private var imageReader: ImageReader? = null
    private var bgThread: HandlerThread? = null
    private var bgHandler: Handler? = null
    private var previewSize: Size = Size(1280, 720)
    private var sensorOrientation = 90
    private var torchEnabled = false
    @Volatile private var foundResult = false
    private val zxingReader = MultiFormatReader()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        WindowCompat.setDecorFitsSystemWindows(window, false)
        WindowInsetsControllerCompat(window, window.decorView).apply {
            systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            hide(WindowInsetsCompat.Type.systemBars())
        }

        val root = FrameLayout(this).apply {
            setBackgroundColor(PlayCraftTheme.BG_OBSIDIAN)
        }

        textureView = TextureView(this).apply {
            layoutParams = FrameLayout.LayoutParams(-1, -1)
        }
        root.addView(textureView)

        val overlay = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER_HORIZONTAL
            setPadding(48, 84, 48, 64)
        }

        val headerTxt = TextView(this).apply {
            text = "PLAYCRAFT QR PAIR"
            setTextColor(PlayCraftTheme.CYAN_GLOW)
            textSize = 18f
            typeface = Typeface.DEFAULT_BOLD
            gravity = Gravity.CENTER
        }
        val subTxt = TextView(this).apply {
            text = "Align the PlayCraft PC QR code inside the frame"
            setTextColor(PlayCraftTheme.TEXT_WHITE)
            textSize = 13f
            gravity = Gravity.CENTER
            setPadding(0, 8, 0, 32)
        }

        val spacerTop = android.view.View(this).apply {
            layoutParams = LinearLayout.LayoutParams(1, 0, 0.35f)
        }

        val reticleSizePx = (resources.displayMetrics.widthPixels * 0.66f).toInt().coerceIn(380, 780)
        val reticle = android.view.View(this).apply {
            layoutParams = LinearLayout.LayoutParams(reticleSizePx, reticleSizePx)
            background = GradientDrawable().apply {
                setColor(0x1200E5FF)
                setStroke(6, PlayCraftTheme.CYAN_GLOW)
                cornerRadius = 32f
            }
        }

        val spacerBot = android.view.View(this).apply {
            layoutParams = LinearLayout.LayoutParams(1, 0, 0.65f)
        }

        val controlsRow = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER
            layoutParams = LinearLayout.LayoutParams(-1, -2)
        }

        lateinit var torchBtn: Button
        torchBtn = Button(this).apply {
            text = "Torch: OFF"
            isAllCaps = false
            setTextColor(PlayCraftTheme.TEXT_WHITE)
            typeface = Typeface.DEFAULT_BOLD
            background = GradientDrawable().apply {
                setColor(PlayCraftTheme.BG_SURFACE)
                setStroke(2, PlayCraftTheme.BORDER_HAIRLINE)
                cornerRadius = 24f
            }
            setPadding(32, 16, 32, 16)
            layoutParams = LinearLayout.LayoutParams(0, -2, 1f)
            setOnClickListener {
                torchEnabled = !torchEnabled
                text = if (torchEnabled) "Torch: ON" else "Torch: OFF"
                applyTorchState()
            }
        }

        val gap = android.view.View(this).apply { layoutParams = LinearLayout.LayoutParams(24, 1) }

        val cancelBtn = Button(this).apply {
            text = "Cancel Scan"
            isAllCaps = false
            setTextColor(PlayCraftTheme.BG_OBSIDIAN)
            typeface = Typeface.DEFAULT_BOLD
            background = GradientDrawable().apply {
                setColor(PlayCraftTheme.CYAN_GLOW)
                cornerRadius = 24f
            }
            setPadding(32, 16, 32, 16)
            layoutParams = LinearLayout.LayoutParams(0, -2, 1f)
            setOnClickListener {
                setResult(Activity.RESULT_CANCELED)
                finish()
            }
        }

        controlsRow.addView(torchBtn)
        controlsRow.addView(gap)
        controlsRow.addView(cancelBtn)

        overlay.addView(headerTxt)
        overlay.addView(subTxt)
        overlay.addView(spacerTop)
        overlay.addView(reticle)
        overlay.addView(spacerBot)
        overlay.addView(controlsRow)
        root.addView(overlay, FrameLayout.LayoutParams(-1, -1))
        setContentView(root)

        textureView.surfaceTextureListener = object : TextureView.SurfaceTextureListener {
            override fun onSurfaceTextureAvailable(st: SurfaceTexture, w: Int, h: Int) {
                openCamera(w, h)
            }
            override fun onSurfaceTextureSizeChanged(st: SurfaceTexture, w: Int, h: Int) {
                configurePreviewTransform(w, h)
            }
            override fun onSurfaceTextureDestroyed(st: SurfaceTexture): Boolean = true
            override fun onSurfaceTextureUpdated(st: SurfaceTexture) {}
        }
    }

    @SuppressLint("MissingPermission")
    private fun openCamera(viewW: Int, viewH: Int) {
        bgThread = HandlerThread("playcraft-portrait-qr").apply { start() }
        bgHandler = Handler(bgThread!!.looper)
        val camMgr = getSystemService(Context.CAMERA_SERVICE) as CameraManager
        val camId = camMgr.cameraIdList.firstOrNull { id ->
            camMgr.getCameraCharacteristics(id).get(CameraCharacteristics.LENS_FACING) ==
                CameraCharacteristics.LENS_FACING_BACK
        } ?: camMgr.cameraIdList.firstOrNull() ?: return

        val chars = camMgr.getCameraCharacteristics(camId)
        sensorOrientation = chars.get(CameraCharacteristics.SENSOR_ORIENTATION) ?: 90
        val map = chars.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
        val choices = map?.getOutputSizes(SurfaceTexture::class.java)?.toList() ?: listOf(Size(1280, 720))

        // Match camera stream aspect ratio to Portrait view orientation cleanly
        val targetRatio = viewH.toFloat() / max(1, viewW).toFloat()
        previewSize = choices
            .filter { it.width in 640..1920 && it.height in 480..1440 }
            .minByOrNull { abs((it.width.toFloat() / it.height.toFloat()) - targetRatio) }
            ?: Size(1280, 720)

        configurePreviewTransform(viewW, viewH)

        val st = textureView.surfaceTexture ?: return
        st.setDefaultBufferSize(previewSize.width, previewSize.height)
        val previewSurface = Surface(st)

        // Decode frames using ImageFormat.YUV_420_888 at matched resolution
        imageReader = ImageReader.newInstance(
            previewSize.width.coerceAtMost(1280),
            previewSize.height.coerceAtMost(960),
            android.graphics.ImageFormat.YUV_420_888,
            2
        ).apply {
            setOnImageAvailableListener({ reader ->
                val img = reader.acquireLatestImage() ?: return@setOnImageAvailableListener
                try {
                    if (!foundResult) {
                        val yPlane = img.planes[0]
                        val buf = yPlane.buffer
                        val bytes = ByteArray(buf.remaining())
                        buf.get(bytes)
                        val src = PlanarYUVLuminanceSource(
                            bytes, yPlane.rowStride, img.height,
                            0, 0, img.width, img.height, false
                        )
                        val bmp = BinaryBitmap(HybridBinarizer(src))
                        runCatching { zxingReader.decode(bmp) }.getOrNull()?.let { res ->
                            val text = res.text ?: ""
                            if (text.isNotBlank()) {
                                foundResult = true
                                runOnUiThread {
                                    setResult(Activity.RESULT_OK, Intent().putExtra("qr_result", text))
                                    finish()
                                }
                            }
                        }
                    }
                } finally {
                    img.close()
                }
            }, bgHandler)
        }

        runCatching {
            camMgr.openCamera(camId, object : CameraDevice.StateCallback() {
                override fun onOpened(camera: CameraDevice) {
                    cameraDevice = camera
                    val surfaces = listOf(previewSurface, imageReader!!.surface)
                    camera.createCaptureSession(surfaces, object : CameraCaptureSession.StateCallback() {
                        override fun onConfigured(session: CameraCaptureSession) {
                            captureSession = session
                            previewBuilder = camera.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW).apply {
                                addTarget(previewSurface)
                                addTarget(imageReader!!.surface)
                                set(CaptureRequest.CONTROL_AF_MODE, CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_PICTURE)
                            }
                            previewBuilder?.build()?.let { session.setRepeatingRequest(it, null, bgHandler) }
                        }
                        override fun onConfigureFailed(session: CameraCaptureSession) {}
                    }, bgHandler)
                }
                override fun onDisconnected(camera: CameraDevice) { camera.close() }
                override fun onError(camera: CameraDevice, error: Int) { camera.close() }
            }, bgHandler)
        }
    }

    private fun configurePreviewTransform(viewW: Int, viewH: Int) {
        if (viewW == 0 || viewH == 0) return
        val matrix = Matrix()
        val viewRect = RectF(0f, 0f, viewW.toFloat(), viewH.toFloat())
        val cx = viewRect.centerX()
        val cy = viewRect.centerY()

        // Account for camera sensor natural landscape vs Portrait view (90deg rotated)
        val swapped = sensorOrientation == 90 || sensorOrientation == 270
        val bufW = if (swapped) previewSize.height.toFloat() else previewSize.width.toFloat()
        val bufH = if (swapped) previewSize.width.toFloat() else previewSize.height.toFloat()

        // Scale buffer uniformly to center-crop fill view without distortion
        val scale = max(viewW / bufW, viewH / bufH)
        matrix.postScale((bufW * scale) / viewW, (bufH * scale) / viewH, cx, cy)
        runOnUiThread { textureView.setTransform(matrix) }
    }

    private fun applyTorchState() {
        val builder = previewBuilder ?: return
        val session = captureSession ?: return
        builder.set(
            CaptureRequest.FLASH_MODE,
            if (torchEnabled) CameraMetadata.FLASH_MODE_TORCH else CameraMetadata.FLASH_MODE_OFF
        )
        runCatching { session.setRepeatingRequest(builder.build(), null, bgHandler) }
    }

    override fun onDestroy() {
        runCatching { captureSession?.close() }
        runCatching { cameraDevice?.close() }
        runCatching { imageReader?.close() }
        runCatching { bgThread?.quitSafely() }
        super.onDestroy()
    }
}

