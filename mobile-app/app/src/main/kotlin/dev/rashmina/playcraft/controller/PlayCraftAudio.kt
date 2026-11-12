package dev.rashmina.playcraft.controller

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.os.Build
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import java.util.concurrent.Executors
import kotlin.math.PI
import kotlin.math.exp
import kotlin.math.sin

/**
 * DualSense / Xbox-style haptic actuator & PS5 UI audio synthesizer.
 * Translates live PC game XInput force-feedback rumble (large/small motors) into Android waveforms.
 */
class PlayCraftAudio(context: Context, private val config: ControllerConfig) {
    private val audioPool = Executors.newSingleThreadExecutor()

    @Suppress("DEPRECATION")
    private val vibrator: Vibrator? = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        (context.getSystemService(Context.VIBRATOR_MANAGER_SERVICE) as? VibratorManager)?.defaultVibrator
    } else {
        context.getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
    }

    @Volatile private var lastRumbleAtMs = 0L
    @Volatile private var lastAmp = 0

    fun hapticTap(ms: Long = 11L, strength: Int = 135) {
        if (!config.hapticsEnabled) return
        val vib = vibrator ?: return
        if (!vib.hasVibrator()) return
        runCatching {
            val amp = strength.coerceIn(1, 255)
            if (vib.hasAmplitudeControl()) {
                vib.vibrate(VibrationEffect.createOneShot(ms, amp))
            } else {
                vib.vibrate(VibrationEffect.createOneShot(ms, VibrationEffect.DEFAULT_AMPLITUDE))
            }
        }
    }

    /**
     * Drives live PS5/Xbox controller rumble from Forza/PC games (0..255 per motor).
     */
    fun triggerGameRumble(largeMotor: Int, smallMotor: Int) {
        if (!config.hapticsEnabled || config.gameRumbleScalePct <= 0) {
            stopRumble()
            return
        }
        val vib = vibrator ?: return
        if (!vib.hasVibrator()) return

        val l = largeMotor.coerceIn(0, 255)
        val s = smallMotor.coerceIn(0, 255)
        if (l == 0 && s == 0) {
            stopRumble()
            return
        }

        val scale = config.gameRumbleScalePct / 100f
        // Combine deep bass large motor + sharp crisp small motor into nuanced amplitude
        val rawAmp = ((l * 0.72f + s * 0.45f) * scale).toInt().coerceIn(28, 255)
        val now = System.currentTimeMillis()
        if (now - lastRumbleAtMs < 48L && kotlin.math.abs(rawAmp - lastAmp) < 36) {
            return
        }
        lastRumbleAtMs = now
        lastAmp = rawAmp

        runCatching {
            if (vib.hasAmplitudeControl()) {
                if (s > l + 35 && s > 60) {
                    // Crisp rapid pulse texture for curb / gravel / gunshots
                    val hi = rawAmp
                    val lo = (rawAmp * 0.45f).toInt().coerceAtLeast(18)
                    vib.vibrate(
                        VibrationEffect.createWaveform(
                            longArrayOf(0L, 42L, 18L, 42L, 18L, 42L),
                            intArrayOf(0, hi, lo, hi, lo, hi),
                            -1
                        )
                    )
                } else {
                    // Deep heavy crash / engine torque vibration
                    vib.vibrate(VibrationEffect.createOneShot(175L, rawAmp))
                }
            } else {
                val dur = (30L + (rawAmp * 110L / 255L))
                vib.vibrate(VibrationEffect.createOneShot(dur, VibrationEffect.DEFAULT_AMPLITUDE))
            }
        }
    }

    fun stopRumble() {
        lastAmp = 0
        runCatching { vibrator?.cancel() }
    }

    fun playNavTick() {
        if (!config.uiSoundsEnabled) return
        playToneAsync(freqHz = 740.0, durationMs = 45, gain = 0.14)
    }

    fun playConnectChime() {
        if (!config.uiSoundsEnabled) return
        playToneAsync(freqHz = 523.25, durationMs = 95, gain = 0.20)
        playToneAsync(freqHz = 783.99, durationMs = 140, gain = 0.22)
    }

    fun playAlertTone() {
        if (!config.uiSoundsEnabled) return
        playToneAsync(freqHz = 320.0, durationMs = 110, gain = 0.18)
    }

    private fun playToneAsync(freqHz: Double, durationMs: Int, gain: Double) {
        audioPool.execute {
            runCatching {
                val sampleRate = 22050
                val count = (sampleRate * durationMs) / 1000
                val pcm = ShortArray(count)
                for (i in 0 until count) {
                    val t = i.toDouble() / sampleRate
                    val env = exp(-t * 16.0)
                    val s = sin(2.0 * PI * freqHz * t) * env * gain
                    pcm[i] = (s * Short.MAX_VALUE).toInt().coerceIn(-32768, 32767).toShort()
                }
                val attrs = AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_GAME)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                    .build()
                val fmt = AudioFormat.Builder()
                    .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                    .setSampleRate(sampleRate)
                    .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                    .build()
                val track = AudioTrack.Builder()
                    .setAudioAttributes(attrs)
                    .setAudioFormat(fmt)
                    .setTransferMode(AudioTrack.MODE_STATIC)
                    .setBufferSizeInBytes(pcm.size * 2)
                    .build()
                track.write(pcm, 0, pcm.size)
                track.play()
                Thread.sleep(durationMs.toLong() + 20L)
                track.stop()
                track.release()
            }
        }
    }

    fun release() {
        stopRumble()
        audioPool.shutdownNow()
    }
}
