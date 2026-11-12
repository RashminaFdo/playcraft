package dev.rashmina.playcraft.controller

import android.content.Context
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.os.Build
import android.view.Surface
import android.view.WindowManager
import kotlin.math.abs
import kotlin.math.atan2
import kotlin.math.hypot
import kotlin.math.pow
import kotlin.math.sign

/**
 * Gimbal-lock-free native Android hardware steering sensor engine.
 * Prioritizes 6DoF IMU Game Rotation Vector -> Rotation Vector -> Accelerometer Gravity fallback.
 * Never blocked by browser HTTPS rules; locks smoothly to display rotation (90 vs 270 landscape).
 */
class GyroWheelEngine(
    private val context: Context,
    private val config: ControllerConfig,
    private val onSteerUpdated: (axis: Float, deg: Float, sourceLabel: String) -> Unit
) : SensorEventListener {

    private val sensorManager = context.getSystemService(Context.SENSOR_SERVICE) as SensorManager
    private val gameRotSensor = sensorManager.getDefaultSensor(Sensor.TYPE_GAME_ROTATION_VECTOR)
    private val rotSensor = sensorManager.getDefaultSensor(Sensor.TYPE_ROTATION_VECTOR)
    private val accelSensor = sensorManager.getDefaultSensor(Sensor.TYPE_ACCELEROMETER)

    private var activeSensor: Sensor? = null
    private var started = false

    private var zeroOffsetDeg = 0f
    private var lastRawTiltDeg = 0f
    private var smoothedAxis = 0f

    val hasAnySteeringSensor: Boolean
        get() = gameRotSensor != null || rotSensor != null || accelSensor != null

    val sensorStatusLabel: String
        get() = when {
            gameRotSensor != null -> "6DoF IMU Gyro Ready"
            rotSensor != null -> "Rotation Fusion Ready"
            accelSensor != null -> "Accelerometer Gravity Steer Ready"
            else -> "No Motion Sensor (Use Touch Steer)"
        }

    fun start() {
        if (started) return
        activeSensor = gameRotSensor ?: rotSensor ?: accelSensor
        val s = activeSensor ?: return
        sensorManager.registerListener(this, s, SensorManager.SENSOR_DELAY_GAME)
        started = true
    }

    fun stop() {
        if (!started) return
        sensorManager.unregisterListener(this)
        started = false
    }

    fun calibrateZero() {
        zeroOffsetDeg = lastRawTiltDeg
        smoothedAxis = 0f
        onSteerUpdated(0f, 0f, "Zero Centered")
    }

    fun resetFilter() {
        smoothedAxis = 0f
    }

    private fun currentSurfaceRotation(): Int {
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            runCatching { context.display?.rotation ?: Surface.ROTATION_90 }.getOrDefault(Surface.ROTATION_90)
        } else {
            @Suppress("DEPRECATION")
            (context.getSystemService(Context.WINDOW_SERVICE) as WindowManager).defaultDisplay.rotation
        }
    }

    override fun onSensorChanged(event: SensorEvent) {
        val rawDeg: Float
        val label: String

        when (event.sensor.type) {
            Sensor.TYPE_GAME_ROTATION_VECTOR,
            Sensor.TYPE_ROTATION_VECTOR -> {
                val rotMat = FloatArray(9)
                val remapped = FloatArray(9)
                SensorManager.getRotationMatrixFromVector(rotMat, event.values)

                // World Up vector expressed in device coords is row 2 of rotMat: (rotMat[6], rotMat[7], rotMat[8])
                // (Wait: SensorManager matrix maps device -> world, so column vectors / row 2 give gravity-up!)
                val rot = currentSurfaceRotation()
                when (rot) {
                    Surface.ROTATION_90 -> SensorManager.remapCoordinateSystem(
                        rotMat,
                        SensorManager.AXIS_MINUS_Y,
                        SensorManager.AXIS_X,
                        remapped
                    )
                    Surface.ROTATION_270 -> SensorManager.remapCoordinateSystem(
                        rotMat,
                        SensorManager.AXIS_Y,
                        SensorManager.AXIS_MINUS_X,
                        remapped
                    )
                    Surface.ROTATION_180 -> SensorManager.remapCoordinateSystem(
                        rotMat,
                        SensorManager.AXIS_MINUS_X,
                        SensorManager.AXIS_MINUS_Y,
                        remapped
                    )
                    else -> System.arraycopy(rotMat, 0, remapped, 0, 9)
                }

                // In screen-aligned coords: World Z (Up) vector dot Screen-Right = remapped[6],
                // World Z dot Screen-Up = remapped[7], World Z dot Screen-Out = remapped[8].
                val rightUp = remapped[6]
                val upUp = remapped[7]
                val zUp = remapped[8]
                val rad = atan2(-rightUp.toDouble(), hypot(upUp.toDouble(), zUp.toDouble() * 0.45).coerceAtLeast(0.25))
                rawDeg = Math.toDegrees(rad).toFloat()
                label = if (event.sensor.type == Sensor.TYPE_GAME_ROTATION_VECTOR) "IMU Gyro" else "Fusion Gyro"
            }

            Sensor.TYPE_ACCELEROMETER -> {
                val gx = event.values[0]
                val gy = event.values[1]
                val gz = event.values[2]
                val (rightComp, upComp) = when (currentSurfaceRotation()) {
                    Surface.ROTATION_90 -> Pair(-gy, gx)
                    Surface.ROTATION_270 -> Pair(gy, -gx)
                    Surface.ROTATION_180 -> Pair(-gx, -gy)
                    else -> Pair(gx, gy)
                }
                val rad = atan2(
                    -rightComp.toDouble(),
                    hypot(upComp.toDouble(), gz.toDouble() * 0.45).coerceAtLeast(1.8)
                )
                rawDeg = Math.toDegrees(rad).toFloat()
                label = "Gravity Tilt"
            }

            else -> return
        }

        lastRawTiltDeg = rawDeg
        if (!config.gyroEnabled || config.steerInputMode == PlayCraftTheme.STEER_TOUCH_ONLY) {
            return
        }

        var relDeg = rawDeg - zeroOffsetDeg
        if (config.gyroInvert) relDeg = -relDeg

        val maxDeg = config.gyroMaxAngleDeg.toFloat().coerceAtLeast(15f)
        var norm = (relDeg / maxDeg).coerceIn(-1f, 1f)

        val dz = config.gyroDeadzonePct / 100f
        norm = if (abs(norm) <= dz) {
            0f
        } else {
            val rescaled = (abs(norm) - dz) / (1f - dz)
            sign(norm) * rescaled.pow(config.gyroGamma)
        }

        val alpha = config.gyroSmoothing
        smoothedAxis = smoothedAxis * (1f - alpha) + norm * alpha
        if (abs(smoothedAxis) < 0.008f && norm == 0f) {
            smoothedAxis = 0f
        }

        onSteerUpdated(smoothedAxis, smoothedAxis * maxDeg, label)
    }

    override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) {}
}
