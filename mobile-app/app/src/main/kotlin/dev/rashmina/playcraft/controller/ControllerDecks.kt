package dev.rashmina.playcraft.controller

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.Canvas
import android.graphics.LinearGradient
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RadialGradient
import android.graphics.RectF
import android.graphics.Shader
import android.graphics.Typeface
import android.view.MotionEvent
import android.view.View
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.hypot
import kotlin.math.min
import kotlin.math.sin

/**
 * Multi-touch Racing Wheel Cockpit rendered on Android Canvas for zero-lag 60-120fps telemetry.
 * Features realistic F1/GT racing steering wheel (carbon rim, metallic spokes, 6-bolt hub, shifting
 * paddle ears, live LED arc), textured aluminum Brake/Gas pedals, & responsive multi-size layout.
 */
@SuppressLint("ViewConstructor")
class WheelDeckView(
    context: Context,
    private val config: ControllerConfig,
    private val bridge: NetworkBridge,
    private val audio: PlayCraftAudio,
    private val onZeroGyro: () -> Unit,
    private val onToggleGyro: () -> Unit,
    private val onOpenSettings: () -> Unit
) : View(context) {

    private val pBg = Paint(Paint.ANTI_ALIAS_FLAG)
    private val pBorder = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.STROKE }
    private val pFill = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.FILL }
    private val pWheel = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.STROKE }
    private val pText = Paint(Paint.ANTI_ALIAS_FLAG).apply { textAlign = Paint.Align.CENTER; typeface = Typeface.DEFAULT_BOLD }
    private val pSub = Paint(Paint.ANTI_ALIAS_FLAG).apply { textAlign = Paint.Align.CENTER }

    private data class RectButton(
        val id: String,
        val label: String,
        val sub: String,
        val rect: RectF = RectF(),
        val activePointers: MutableSet<Int> = mutableSetOf()
    )

    private val buttons = listOf(
        RectButton("LB", "GEAR -", "LB / L1"),
        RectButton("HANDBRAKE", "E-BRAKE", "SPACE / A"),
        RectButton("Y", "REWIND", "Y / \u25B3"),
        RectButton("RB", "GEAR +", "RB / R1"),
        RectButton("OPTIONS", "MENU", "START"),
        RectButton("TOGGLE_GYRO", "GYRO", ""),
        RectButton("ZERO_GYRO", "ZERO", "CENTER"),
        RectButton("QUICK_SET", "TUNE", "SETTINGS")
    )

    private val brakeRect = RectF()
    private val gasRect = RectF()
    private val sliderRect = RectF()
    private val hubRect = RectF()
    private var brakePointerId = -1
    private var gasPointerId = -1
    private var sliderPointerId = -1
    private var brakeStartY = 0f
    private var gasStartY = 0f
    private var brakeSliding = false
    private var gasSliding = false

    private var brakeVal = 0f
    private var gasVal = 0f
    private var steerAxis = 0f
    private var steerDeg = 0f
    private var steerSource = "Gyro Ready"
    private var rumblePulseAlpha = 0f
    private var lastTouchSteerSentAt = 0L

    fun isTouchSteerHolding(): Boolean = sliderPointerId != -1

    fun updateSteerTelemetry(axis: Float, deg: Float, source: String) {
        if (isTouchSteerHolding()) return
        steerAxis = axis.coerceIn(-1f, 1f)
        steerDeg = deg
        steerSource = source
        postInvalidateOnAnimation()
    }

    fun setRumbleVisual(large: Int, small: Int) {
        rumblePulseAlpha = ((large * 0.65f + small * 0.45f) / 255f).coerceIn(0f, 1f)
        postInvalidateOnAnimation()
    }

    private fun keyFor(btn: String): String = config.loadKeymap()[btn] ?: ""

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        val dp = resources.displayMetrics.density
        val pad = (min(w, h) * 0.024f).coerceAtLeast(8f * dp)
        val colW = (w * 0.255f).coerceIn(135f * dp, 240f * dp)
        val gap = pad * 0.85f
        val paddleH = (h * 0.145f).coerceIn(38f * dp, 64f * dp)
        val leftX = pad
        val rightX = w - pad - colW
        val centerL = leftX + colW + gap
        val centerR = rightX - gap

        val halfCol = (colW - gap * 0.6f) / 2f
        buttons[0].rect.set(leftX, pad, leftX + halfCol, pad + paddleH)
        buttons[1].rect.set(leftX + halfCol + gap * 0.6f, pad, leftX + colW, pad + paddleH)
        buttons[2].rect.set(rightX, pad, rightX + halfCol, pad + paddleH)
        buttons[3].rect.set(rightX + halfCol + gap * 0.6f, pad, rightX + colW, pad + paddleH)

        val pedalTop = pad + paddleH + gap
        if (config.swapPedalSides) {
            gasRect.set(leftX, pedalTop, leftX + colW, h - pad)
            brakeRect.set(rightX, pedalTop, rightX + colW, h - pad)
        } else {
            brakeRect.set(leftX, pedalTop, leftX + colW, h - pad)
            gasRect.set(rightX, pedalTop, rightX + colW, h - pad)
        }

        val cw = (centerR - centerL).coerceAtLeast(180f)
        val pillH = (h * 0.12f).coerceIn(32f * dp, 52f * dp)
        val pillW = (cw - gap * 3f) / 4f
        buttons[5].rect.set(centerL, pad, centerL + pillW, pad + pillH)
        buttons[6].rect.set(centerL + pillW + gap, pad, centerL + pillW * 2f + gap, pad + pillH)
        buttons[7].rect.set(centerL + (pillW + gap) * 2f, pad, centerL + pillW * 3f + gap * 2f, pad + pillH)
        buttons[4].rect.set(centerL + (pillW + gap) * 3f, pad, centerR, pad + pillH)

        val sliderH = (h * 0.165f).coerceIn(42f * dp, 68f * dp)
        sliderRect.set(centerL, h - pad - sliderH, centerR, h - pad)
        hubRect.set(centerL, buttons[5].rect.bottom + gap * 0.8f, centerR, sliderRect.top - gap * 0.8f)
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val dp = resources.displayMetrics.density
        val accent = config.activeAccentColor()
        val accentSoft = if (config.shellTheme == PlayCraftTheme.SHELL_XBOX) {
            PlayCraftTheme.XBOX_SOFT
        } else {
            PlayCraftTheme.CYAN_SOFT
        }

        // Glass cockpit center container
        pBg.shader = LinearGradient(
            hubRect.left, hubRect.top, hubRect.left, hubRect.bottom,
            0xE610192D.toInt(), 0xE6090E1A.toInt(), Shader.TileMode.CLAMP
        )
        canvas.drawRoundRect(hubRect, 18f * dp, 18f * dp, pBg)
        pBg.shader = null
        pBorder.strokeWidth = 1.5f * dp
        pBorder.color = if (rumblePulseAlpha > 0.08f) PlayCraftTheme.BRAKE_RED else PlayCraftTheme.BORDER_HAIRLINE
        canvas.drawRoundRect(hubRect, 18f * dp, 18f * dp, pBorder)

        // Subtle ambient radial halo behind steering wheel (pulses brighter on Xbox/Forza rumble!)
        val wheelR = (min(hubRect.width() * 0.40f, hubRect.height() * 0.42f)).coerceAtLeast(36f * dp)
        val cx = hubRect.centerX()
        val cy = hubRect.centerY() - 2f * dp
        val glowAlpha = (38 + (rumblePulseAlpha * 110).toInt()).coerceIn(30, 175)
        val glowRgb = if (rumblePulseAlpha > 0.25f) 0x00FF5533 else (accent and 0x00FFFFFF)
        pFill.shader = RadialGradient(
            cx, cy, wheelR * 1.45f,
            (glowAlpha shl 24) or glowRgb, 0x00000000, Shader.TileMode.CLAMP
        )
        canvas.drawCircle(cx, cy, wheelR * 1.45f, pFill)
        pFill.shader = null

        // Realistic Textured Brake & Gas Pedals
        drawRealisticPedal(canvas, brakeRect, "BRAKE / REV", brakeVal, PlayCraftTheme.BRAKE_RED, dp)
        drawRealisticPedal(canvas, gasRect, "GAS / ACCEL", gasVal, PlayCraftTheme.GAS_GREEN, dp)

        // Top Paddles & Hub Action Pills
        for (btn in buttons) {
            val isPressed = btn.activePointers.isNotEmpty()
            val isGyroActive = btn.id == "TOGGLE_GYRO" && config.gyroEnabled
            pBg.shader = LinearGradient(
                btn.rect.left, btn.rect.top, btn.rect.left, btn.rect.bottom,
                when {
                    isPressed -> accentSoft
                    isGyroActive -> (accent and 0x00FFFFFF) or 0x2A000000
                    else -> 0xFF18233B.toInt()
                },
                0xFF0E1526.toInt(),
                Shader.TileMode.CLAMP
            )
            canvas.drawRoundRect(btn.rect, 11f * dp, 11f * dp, pBg)
            pBg.shader = null

            pBorder.strokeWidth = if (isPressed || isGyroActive) 2f * dp else 1.2f * dp
            pBorder.color = if (isPressed || isGyroActive) accent else PlayCraftTheme.BORDER_HAIRLINE
            canvas.drawRoundRect(btn.rect, 11f * dp, 11f * dp, pBorder)

            val label = if (btn.id == "TOGGLE_GYRO") {
                if (config.gyroEnabled) "GYRO: ON" else "GYRO: OFF"
            } else btn.label
            val sub = if (btn.id == "TOGGLE_GYRO") config.steerInputMode else btn.sub

            pText.color = if (isPressed || isGyroActive) accent else PlayCraftTheme.TEXT_WHITE
            pText.textSize = (btn.rect.height() * 0.34f).coerceIn(11f * dp, 17f * dp)
            val labelY = btn.rect.centerY() - if (sub.isNotEmpty()) btn.rect.height() * 0.06f else -pText.textSize * 0.35f
            canvas.drawText(label, btn.rect.centerX(), labelY, pText)
            if (sub.isNotEmpty()) {
                pSub.color = PlayCraftTheme.TEXT_MUTED
                pSub.textSize = (btn.rect.height() * 0.21f).coerceIn(8f * dp, 12f * dp)
                canvas.drawText(sub, btn.rect.centerX(), btn.rect.centerY() + btn.rect.height() * 0.27f, pSub)
            }
        }

        // Steering Deflection Arc Telemetry Ring (stationary outer gauge)
        val gaugeR = wheelR * 1.18f
        val gaugeRect = RectF(cx - gaugeR, cy - gaugeR, cx + gaugeR, cy + gaugeR)
        pWheel.strokeCap = Paint.Cap.ROUND
        pWheel.strokeWidth = 3.5f * dp
        pWheel.color = 0x26FFFFFF
        canvas.drawArc(gaugeRect, -165f, 150f, false, pWheel)
        if (abs(steerAxis) > 0.02f) {
            pWheel.color = accent
            canvas.drawArc(gaugeRect, -90f, steerAxis * 75f, false, pWheel)
        }

        // ========== REALISTIC GT / F1 STEERING WHEEL (ROTATING ASSET) ==========
        drawRealisticSteeringWheel(canvas, cx, cy, wheelR, steerAxis * 95f, accent, dp)

        // Centered Frosted Digital Angle Badge
        val badgeW = (wheelR * 0.86f).coerceAtLeast(54f * dp)
        val badgeH = (wheelR * 0.42f).coerceAtLeast(24f * dp)
        val badgeRect = RectF(cx - badgeW / 2f, cy - badgeH / 2f, cx + badgeW / 2f, cy + badgeH / 2f)
        pBg.color = 0xE8070B14.toInt()
        canvas.drawRoundRect(badgeRect, badgeH / 2f, badgeH / 2f, pBg)
        pBorder.strokeWidth = 1.6f * dp
        pBorder.color = accent
        canvas.drawRoundRect(badgeRect, badgeH / 2f, badgeH / 2f, pBorder)

        pText.color = accent
        pText.textSize = (badgeH * 0.56f).coerceIn(13f * dp, 24f * dp)
        canvas.drawText("%.1f\u00B0".format(steerDeg), cx, cy + pText.textSize * 0.34f, pText)

        pSub.color = PlayCraftTheme.TEXT_MUTED
        pSub.textSize = (hubRect.height() * 0.075f).coerceIn(9.5f * dp, 13f * dp)
        canvas.drawText(
            if (rumblePulseAlpha > 0.15f) "FFB RUMBLE ACTIVE \u2022 $steerSource" else steerSource,
            cx,
            hubRect.bottom - 7f * dp,
            pSub
        )

        // Modern Touch Steer Slider Bar
        drawTouchSteerSlider(canvas, accent, dp)
    }

    private fun drawRealisticSteeringWheel(
        canvas: Canvas,
        cx: Float,
        cy: Float,
        r: Float,
        angleDeg: Float,
        accent: Int,
        dp: Float
    ) {
        canvas.save()
        canvas.rotate(angleDeg, cx, cy)

        val rimStroke = r * 0.22f

        // 1. Carbon Paddle Shifter Ears behind spokes
        pBg.shader = LinearGradient(cx - r, cy - r * 0.4f, cx + r, cy, 0xFF28344B.toInt(), 0xFF131A29.toInt(), Shader.TileMode.CLAMP)
        val leftPaddle = RectF(cx - r * 0.86f, cy - r * 0.34f, cx - r * 0.48f, cy + r * 0.12f)
        val rightPaddle = RectF(cx + r * 0.48f, cy - r * 0.34f, cx + r * 0.86f, cy + r * 0.12f)
        canvas.drawRoundRect(leftPaddle, 6f * dp, 6f * dp, pBg)
        canvas.drawRoundRect(rightPaddle, 6f * dp, 6f * dp, pBg)
        pBg.shader = null
        pBorder.strokeWidth = 1f * dp
        pBorder.color = 0x44FFFFFF
        canvas.drawRoundRect(leftPaddle, 6f * dp, 6f * dp, pBorder)
        canvas.drawRoundRect(rightPaddle, 6f * dp, 6f * dp, pBorder)

        // 2. Brushed Titanium Y-Spoke Spider Frame
        pWheel.strokeCap = Paint.Cap.BUTT
        pWheel.strokeWidth = r * 0.19f
        pWheel.shader = LinearGradient(
            cx - r, cy - r * 0.3f, cx + r, cy + r * 0.5f,
            0xFF475569.toInt(), 0xFF1E293B.toInt(), Shader.TileMode.CLAMP
        )
        canvas.drawLine(cx - r * 0.88f, cy, cx + r * 0.88f, cy, pWheel)
        canvas.drawLine(cx, cy, cx, cy + r * 0.90f, pWheel)
        pWheel.shader = null

        // Spoke accent cut lines
        pWheel.strokeWidth = 1.6f * dp
        pWheel.color = 0x44FFFFFF
        canvas.drawLine(cx - r * 0.72f, cy - r * 0.06f, cx - r * 0.32f, cy - r * 0.06f, pWheel)
        canvas.drawLine(cx + r * 0.32f, cy - r * 0.06f, cx + r * 0.72f, cy - r * 0.06f, pWheel)

        // 3. Drop shadow under outer carbon-leather rim
        pWheel.strokeWidth = rimStroke + 4f * dp
        pWheel.color = 0xAA020409.toInt()
        canvas.drawCircle(cx, cy, r, pWheel)

        // 4. Sculpted Alcantara/Carbon Wheel Rim
        pWheel.strokeWidth = rimStroke
        pWheel.shader = LinearGradient(
            cx - r, cy - r, cx + r, cy + r,
            0xFF2B3548.toInt(), 0xFF0E131F.toInt(), Shader.TileMode.CLAMP
        )
        canvas.drawCircle(cx, cy, r, pWheel)
        pWheel.shader = null

        // Specular highlight along outer leather rim edge
        pWheel.strokeWidth = 2f * dp
        pWheel.color = 0x38FFFFFF
        canvas.drawCircle(cx, cy, r + rimStroke * 0.42f, pWheel)
        pWheel.color = 0x55000000
        canvas.drawCircle(cx, cy, r - rimStroke * 0.44f, pWheel)

        // 9-o'clock & 3-o'clock ergonomic thumb bolster grips
        val rimArc = RectF(cx - r, cy - r, cx + r, cy + r)
        pWheel.strokeWidth = rimStroke * 1.16f
        pWheel.strokeCap = Paint.Cap.ROUND
        pWheel.color = 0xFF1F2A3E.toInt()
        canvas.drawArc(rimArc, 152f, 44f, false, pWheel)
        canvas.drawArc(rimArc, -16f, 44f, false, pWheel)

        // Signature Glowing 12-o'clock Racing Center Stripe
        pWheel.strokeWidth = rimStroke * 0.94f
        pWheel.strokeCap = Paint.Cap.BUTT
        pWheel.color = accent
        canvas.drawArc(rimArc, -98f, 16f, false, pWheel)

        // 5. Central Metallic Hub Dish & 6 Hex Bolts
        val hubR = r * 0.39f
        pBg.shader = RadialGradient(cx - hubR * 0.25f, cy - hubR * 0.25f, hubR * 1.4f, 0xFF334155.toInt(), 0xFF0B111E.toInt(), Shader.TileMode.CLAMP)
        canvas.drawCircle(cx, cy, hubR, pBg)
        pBg.shader = null
        pBorder.strokeWidth = 2.2f * dp
        pBorder.color = 0x55FFFFFF
        canvas.drawCircle(cx, cy, hubR, pBorder)

        val boltOrbit = hubR * 0.78f
        pFill.color = 0xFF94A3B8.toInt()
        for (i in 0 until 6) {
            val rad = Math.toRadians((i * 60.0) - 30.0)
            canvas.drawCircle(
                cx + (cos(rad) * boltOrbit).toFloat(),
                cy + (sin(rad) * boltOrbit).toFloat(),
                2.2f * dp,
                pFill
            )
        }

        canvas.restore()
    }

    private fun drawRealisticPedal(canvas: Canvas, r: RectF, title: String, value: Float, color: Int, dp: Float) {
        // Metallic pedal well background
        pBg.shader = LinearGradient(r.left, r.top, r.right, r.bottom, 0xFF131D33.toInt(), 0xFF0A101E.toInt(), Shader.TileMode.CLAMP)
        canvas.drawRoundRect(r, 18f * dp, 18f * dp, pBg)
        pBg.shader = null

        // Anti-slip milled aluminum pedal slats
        canvas.save()
        val clip = Path().apply { addRoundRect(r, 18f * dp, 18f * dp, Path.Direction.CW) }
        canvas.clipPath(clip)

        val slatPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { this.color = 0x16FFFFFF }
        val slatH = 8f * dp
        val step = 22f * dp
        var yPos = r.top + 16f * dp
        while (yPos < r.bottom - 14f * dp) {
            val slat = RectF(r.left + 16f * dp, yPos, r.right - 16f * dp, yPos + slatH)
            canvas.drawRoundRect(slat, 4f * dp, 4f * dp, slatPaint)
            yPos += step
        }

        // Progressive liquid travel meter
        if (value > 0.01f) {
            val fillTop = r.bottom - r.height() * value
            pFill.shader = LinearGradient(
                0f, fillTop, 0f, r.bottom,
                (color and 0x00FFFFFF) or 0xCC000000.toInt(),
                (color and 0x00FFFFFF) or 0x38000000,
                Shader.TileMode.CLAMP
            )
            canvas.drawRect(r.left, fillTop, r.right, r.bottom, pFill)
            pFill.shader = null
        }
        canvas.restore()

        pBorder.strokeWidth = if (value > 0.01f) 2.5f * dp else 1.4f * dp
        pBorder.color = if (value > 0.01f) color else PlayCraftTheme.BORDER_HAIRLINE
        canvas.drawRoundRect(r, 18f * dp, 18f * dp, pBorder)

        // Centered Frosted Label Plate
        pText.color = PlayCraftTheme.TEXT_WHITE
        pText.textSize = (r.width() * 0.135f).coerceIn(13f * dp, 20f * dp)
        canvas.drawText(title, r.centerX(), r.centerY() - 6f * dp, pText)

        pSub.color = if (value > 0.01f) color else PlayCraftTheme.TEXT_MUTED
        pSub.textSize = (r.width() * 0.12f).coerceIn(12f * dp, 18f * dp)
        canvas.drawText("${(value * 100f).toInt()}%", r.centerX(), r.centerY() + 18f * dp, pSub)
    }

    private fun drawTouchSteerSlider(canvas: Canvas, accent: Int, dp: Float) {
        pBg.shader = LinearGradient(
            sliderRect.left, sliderRect.top, sliderRect.right, sliderRect.top,
            0xFF0D1527.toInt(), 0xFF15223B.toInt(), Shader.TileMode.CLAMP
        )
        canvas.drawRoundRect(sliderRect, 14f * dp, 14f * dp, pBg)
        pBg.shader = null
        pBorder.strokeWidth = 1.4f * dp
        pBorder.color = if (isTouchSteerHolding()) accent else PlayCraftTheme.BORDER_HAIRLINE
        canvas.drawRoundRect(sliderRect, 14f * dp, 14f * dp, pBorder)

        // Precision scale ticks across track
        pWheel.strokeWidth = 1.2f * dp
        pWheel.color = 0x33FFFFFF
        for (i in -4..4) {
            val tx = sliderRect.centerX() + (i / 4f) * (sliderRect.width() * 0.42f)
            val tickH = if (i == 0) sliderRect.height() * 0.55f else sliderRect.height() * 0.25f
            canvas.drawLine(tx, sliderRect.centerY() - tickH / 2f, tx, sliderRect.centerY() + tickH / 2f, pWheel)
        }

        val thumbW = (sliderRect.width() * 0.15f).coerceIn(44f * dp, 84f * dp)
        val thumbCenter = sliderRect.centerX() + steerAxis * ((sliderRect.width() - thumbW) * 0.46f)
        val thumbRect = RectF(
            thumbCenter - thumbW / 2f,
            sliderRect.top + 5f * dp,
            thumbCenter + thumbW / 2f,
            sliderRect.bottom - 5f * dp
        )
        pBg.color = accent
        canvas.drawRoundRect(thumbRect, 10f * dp, 10f * dp, pBg)

        pText.color = PlayCraftTheme.BG_OBSIDIAN
        pText.textSize = (sliderRect.height() * 0.30f).coerceIn(11f * dp, 16f * dp)
        canvas.drawText("${(steerAxis * 100f).toInt()}%", thumbRect.centerX(), thumbRect.centerY() + pText.textSize * 0.35f, pText)

        if (!isTouchSteerHolding()) {
            pSub.color = PlayCraftTheme.TEXT_MUTED
            pSub.textSize = (sliderRect.height() * 0.23f).coerceIn(9f * dp, 12f * dp)
            canvas.drawText("TOUCH STEER", sliderRect.left + 48f * dp, sliderRect.centerY() + pSub.textSize * 0.35f, pSub)
            canvas.drawText("SLIDE L / R", sliderRect.right - 48f * dp, sliderRect.centerY() + pSub.textSize * 0.35f, pSub)
        }
    }

    @SuppressLint("ClickableViewAccessibility")
    override fun onTouchEvent(event: MotionEvent): Boolean {
        val action = event.actionMasked
        val idx = event.actionIndex

        when (action) {
            MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> {
                val pid = event.getPointerId(idx)
                val x = event.getX(idx)
                val y = event.getY(idx)

                if (brakeRect.contains(x, y) && brakePointerId == -1) {
                    brakePointerId = pid
                    brakeStartY = y
                    brakeSliding = false
                    audio.hapticTap(14L, 150)
                    setBrake(1.0f)
                } else if (gasRect.contains(x, y) && gasPointerId == -1) {
                    gasPointerId = pid
                    gasStartY = y
                    gasSliding = false
                    audio.hapticTap(14L, 150)
                    setGas(1.0f)
                } else if (sliderRect.contains(x, y) && sliderPointerId == -1) {
                    sliderPointerId = pid
                    updateTouchSteer(x)
                } else {
                    for (b in buttons) {
                        if (b.rect.contains(x, y)) {
                            val wasEmpty = b.activePointers.isEmpty()
                            b.activePointers.add(pid)
                            if (wasEmpty) handleUiOrGameButton(b.id, true)
                            break
                        }
                    }
                }
                invalidate()
            }

            MotionEvent.ACTION_MOVE -> {
                for (i in 0 until event.pointerCount) {
                    val pid = event.getPointerId(i)
                    val x = event.getX(i)
                    val y = event.getY(i)
                    if (pid == brakePointerId) {
                        if (config.progressivePedals && (brakeSliding || abs(y - brakeStartY) > 18f)) {
                            brakeSliding = true
                            val ratio = ((brakeRect.bottom - y) / brakeRect.height().coerceAtLeast(40f)).coerceIn(0.12f, 1.0f)
                            setBrake(ratio)
                        }
                    } else if (pid == gasPointerId) {
                        if (config.progressivePedals && (gasSliding || abs(y - gasStartY) > 18f)) {
                            gasSliding = true
                            val ratio = ((gasRect.bottom - y) / gasRect.height().coerceAtLeast(40f)).coerceIn(0.12f, 1.0f)
                            setGas(ratio)
                        }
                    } else if (pid == sliderPointerId) {
                        updateTouchSteer(x)
                    }
                }
                invalidate()
            }

            MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP, MotionEvent.ACTION_CANCEL -> {
                val releaseAll = action == MotionEvent.ACTION_CANCEL
                val pid = event.getPointerId(idx)
                if (releaseAll || pid == brakePointerId) {
                    brakePointerId = -1
                    setBrake(0f)
                }
                if (releaseAll || pid == gasPointerId) {
                    gasPointerId = -1
                    setGas(0f)
                }
                if (releaseAll || pid == sliderPointerId) {
                    sliderPointerId = -1
                    steerAxis = 0f
                    steerDeg = 0f
                    steerSource = if (config.gyroEnabled) "Gyro Active" else "Centered"
                    bridge.sendGyroSteer(0f, 0f)
                }
                for (b in buttons) {
                    if (releaseAll && b.activePointers.isNotEmpty()) {
                        b.activePointers.clear()
                        handleUiOrGameButton(b.id, false)
                    } else if (b.activePointers.remove(pid) && b.activePointers.isEmpty()) {
                        handleUiOrGameButton(b.id, false)
                    }
                }
                invalidate()
            }
        }
        return true
    }

    private fun handleUiOrGameButton(id: String, pressed: Boolean) {
        when (id) {
            "TOGGLE_GYRO" -> if (pressed) { audio.playNavTick(); onToggleGyro() }
            "ZERO_GYRO" -> if (pressed) { audio.hapticTap(16L, 180); audio.playNavTick(); onZeroGyro() }
            "QUICK_SET" -> if (pressed) { audio.playNavTick(); onOpenSettings() }
            else -> {
                if (pressed) audio.hapticTap(12L, 140)
                bridge.sendButton(id, pressed, keyFor(id))
            }
        }
    }

    private fun setBrake(v: Float) {
        brakeVal = v.coerceIn(0f, 1f)
        bridge.sendTriggers(lt = brakeVal, rt = null)
    }

    private fun setGas(v: Float) {
        gasVal = v.coerceIn(0f, 1f)
        bridge.sendTriggers(lt = null, rt = gasVal)
    }

    private fun updateTouchSteer(x: Float) {
        if (config.steerInputMode == PlayCraftTheme.STEER_GYRO_ONLY) return
        val norm = ((x - sliderRect.centerX()) / (sliderRect.width() * 0.44f)).coerceIn(-1f, 1f)
        steerAxis = norm
        steerDeg = norm * config.gyroMaxAngleDeg
        steerSource = "Touch Steer"
        val now = System.currentTimeMillis()
        if (now - lastTouchSteerSentAt >= 12L) {
            lastTouchSteerSentAt = now
            bridge.sendGyroSteer(steerAxis, steerDeg)
        }
    }
}

/**
 * Full PS5 DualSense / Xbox Series X|S zero-overlap responsive Gamepad with realistic beveled
 * analog thumbsticks, colored face buttons, dot-matrix touchpad, & multi-screen layout safety.
 */
@SuppressLint("ViewConstructor")
class GamepadDeckView(
    context: Context,
    private val config: ControllerConfig,
    private val bridge: NetworkBridge,
    private val audio: PlayCraftAudio
) : View(context) {

    private val pBg = Paint(Paint.ANTI_ALIAS_FLAG)
    private val pBorder = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.STROKE }
    private val pText = Paint(Paint.ANTI_ALIAS_FLAG).apply { textAlign = Paint.Align.CENTER; typeface = Typeface.DEFAULT_BOLD }
    private val pSub = Paint(Paint.ANTI_ALIAS_FLAG).apply { textAlign = Paint.Align.CENTER }

    private data class PadBtn(
        val id: String,
        val psLabel: String,
        val xboxLabel: String,
        val psTint: Int = PlayCraftTheme.TEXT_WHITE,
        val xboxTint: Int = PlayCraftTheme.TEXT_WHITE,
        val circle: Boolean = false,
        val rect: RectF = RectF(),
        val activePointers: MutableSet<Int> = mutableSetOf()
    )

    private val buttons = listOf(
        PadBtn("L2", "L2", "LT"),
        PadBtn("L1", "L1", "LB"),
        PadBtn("R1", "R1", "RB"),
        PadBtn("R2", "R2", "RT"),
        PadBtn("UP", "\u25B2", "\u25B2"),
        PadBtn("DOWN", "\u25BC", "\u25BC"),
        PadBtn("LEFT", "\u25C0", "\u25C0"),
        PadBtn("RIGHT", "\u25B6", "\u25B6"),
        PadBtn("TRIANGLE", "\u25B3", "Y", 0xFF34D399.toInt(), 0xFFFACC15.toInt(), true),
        PadBtn("CIRCLE", "\u25CB", "B", 0xFFFB7185.toInt(), 0xFFEF4444.toInt(), true),
        PadBtn("CROSS", "\u2715", "A", 0xFF38BDF8.toInt(), 0xFF22C55E.toInt(), true),
        PadBtn("SQUARE", "\u25A1", "X", 0xFFC084FC.toInt(), 0xFF3B82F6.toInt(), true),
        PadBtn("SHARE", "SHARE", "VIEW"),
        PadBtn("PS", "PS", "XBOX", PlayCraftTheme.CYAN_GLOW, PlayCraftTheme.XBOX_GREEN, true),
        PadBtn("OPTIONS", "OPT", "MENU"),
        PadBtn("L3", "L3", "LS"),
        PadBtn("R3", "R3", "RS")
    )

    private val trackpadRect = RectF()
    private val lStickRect = RectF()
    private val rStickRect = RectF()

    private var lStickPid = -1
    private var lStickX = 0f
    private var lStickY = 0f
    private var rStickPid = -1
    private var rStickX = 0f
    private var rStickY = 0f
    private var lastStickSentAt = 0L

    private val trackPointers = mutableMapOf<Int, Pair<Float, Float>>()
    private var trackMaxFingers = 0
    private var trackMovedTotal = 0f
    private var trackDownAt = 0L

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        val dp = resources.displayMetrics.density
        val pad = (min(w, h) * 0.025f).coerceAtLeast(8f * dp)
        val sideW = ((w * 0.365f).coerceAtMost(h * 1.18f)).coerceAtLeast(180f * dp)
        val centerL = pad * 2f + sideW
        val centerR = w - pad * 2f - sideW
        val centerW = (centerR - centerL).coerceAtLeast(120f * dp)

        // Triggers & Bumpers
        val shH = (h * 0.135f).coerceIn(36f * dp, 58f * dp)
        val shW = (sideW - pad * 0.8f) / 2f
        buttons[0].rect.set(pad, pad, pad + shW, pad + shH)
        buttons[1].rect.set(pad + shW + pad * 0.8f, pad, pad + sideW, pad + shH)
        buttons[2].rect.set(w - pad - sideW, pad, w - pad - sideW + shW, pad + shH)
        buttons[3].rect.set(w - pad - shW, pad, w - pad, pad + shH)

        val lowerTop = pad + shH + pad
        val lowerH = (h - lowerTop - pad).coerceAtLeast(100f)

        // Strictly partition each side into two NON-OVERLAPPING sub-columns:
        // Left:  outerLeftCol (D-Pad)  | gap | innerLeftCol (L-Stick + L3)
        // Right: innerRightCol (R-Stick + R3) | gap | outerRightCol (Face Diamond)
        val colGap = (sideW * 0.05f).coerceAtLeast(8f * dp)
        val outerW = (sideW - colGap) * 0.51f
        val innerW = (sideW - colGap) * 0.49f
        val cyLower = lowerTop + lowerH * 0.46f

        // --- D-PAD inside outerLeftCol ---
        val dSide = min(outerW, lowerH) * 0.94f
        val dUnit = dSide / 3.15f
        val dCx = pad + outerW / 2f
        buttons[4].rect.set(dCx - dUnit / 2f, cyLower - dUnit * 1.54f, dCx + dUnit / 2f, cyLower - dUnit * 0.54f)
        buttons[5].rect.set(dCx - dUnit / 2f, cyLower + dUnit * 0.54f, dCx + dUnit / 2f, cyLower + dUnit * 1.54f)
        buttons[6].rect.set(dCx - dUnit * 1.54f, cyLower - dUnit / 2f, dCx - dUnit * 0.54f, cyLower + dUnit / 2f)
        buttons[7].rect.set(dCx + dUnit * 0.54f, cyLower - dUnit / 2f, dCx + dUnit * 1.54f, cyLower + dUnit / 2f)

        // --- LEFT STICK + L3 strictly inside innerLeftCol ---
        val lClickH = (24f * dp).coerceAtMost(lowerH * 0.16f)
        val stickAvailH = lowerH - lClickH - 8f * dp
        val stickSize = min(innerW * 0.92f, stickAvailH * 0.92f).coerceAtLeast(48f * dp)
        val lsCx = pad + outerW + colGap + innerW / 2f
        val lsCy = lowerTop + stickAvailH * 0.48f
        lStickRect.set(lsCx - stickSize / 2f, lsCy - stickSize / 2f, lsCx + stickSize / 2f, lsCy + stickSize / 2f)
        val clickHalfW = (stickSize * 0.46f).coerceAtMost(innerW * 0.46f)
        buttons[15].rect.set(lsCx - clickHalfW, lStickRect.bottom + 6f * dp, lsCx + clickHalfW, min(h - pad, lStickRect.bottom + 6f * dp + lClickH))

        // --- RIGHT STICK + R3 strictly inside innerRightCol ---
        val rightColStart = w - pad - sideW
        val rsCx = rightColStart + innerW / 2f
        rStickRect.set(rsCx - stickSize / 2f, lsCy - stickSize / 2f, rsCx + stickSize / 2f, lsCy + stickSize / 2f)
        buttons[16].rect.set(rsCx - clickHalfW, rStickRect.bottom + 6f * dp, rsCx + clickHalfW, min(h - pad, rStickRect.bottom + 6f * dp + lClickH))

        // --- FACE DIAMOND strictly inside outerRightCol (ZERO OVERLAP GUARANTEED!) ---
        val fSide = min(outerW, lowerH) * 0.94f
        val fSize = fSide / 3.05f
        val fGap = fSize * 1.02f
        val fCx = rightColStart + innerW + colGap + outerW / 2f
        buttons[8].rect.set(fCx - fSize / 2f, cyLower - fGap - fSize / 2f, fCx + fSize / 2f, cyLower - fGap + fSize / 2f)
        buttons[10].rect.set(fCx - fSize / 2f, cyLower + fGap - fSize / 2f, fCx + fSize / 2f, cyLower + fGap + fSize / 2f)
        buttons[11].rect.set(fCx - fGap - fSize / 2f, cyLower - fSize / 2f, fCx - fGap + fSize / 2f, cyLower + fSize / 2f)
        buttons[9].rect.set(fCx + fGap - fSize / 2f, cyLower - fSize / 2f, fCx + fGap + fSize / 2f, cyLower + fSize / 2f)

        // Center Touchpad & Menu Row
        val homeSize = (h * 0.145f).coerceIn(34f * dp, 54f * dp)
        val menuH = (h * 0.105f).coerceIn(28f * dp, 44f * dp)
        val mCy = h - pad - homeSize / 2f
        val sidePillW = ((centerW - homeSize - pad * 1.4f) / 2f).coerceAtLeast(38f * dp)
        buttons[12].rect.set(centerL, mCy - menuH / 2f, centerL + sidePillW, mCy + menuH / 2f)
        buttons[13].rect.set(centerL + centerW / 2f - homeSize / 2f, mCy - homeSize / 2f, centerL + centerW / 2f + homeSize / 2f, mCy + homeSize / 2f)
        buttons[14].rect.set(centerR - sidePillW, mCy - menuH / 2f, centerR, mCy + menuH / 2f)

        trackpadRect.set(centerL, pad, centerR, h - pad - homeSize - pad * 1.1f)
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val dp = resources.displayMetrics.density
        val accent = config.activeAccentColor()
        val accentSoft = if (config.shellTheme == PlayCraftTheme.SHELL_XBOX) {
            PlayCraftTheme.XBOX_SOFT
        } else {
            PlayCraftTheme.CYAN_SOFT
        }
        val isXbox = config.shellTheme == PlayCraftTheme.SHELL_XBOX

        // DualSense Textured Mouse Trackpad
        pBg.shader = LinearGradient(
            trackpadRect.left, trackpadRect.top, trackpadRect.left, trackpadRect.bottom,
            if (trackPointers.isNotEmpty()) 0x2800E5FF else 0xFF121A2E.toInt(),
            0xFF0B111E.toInt(), Shader.TileMode.CLAMP
        )
        canvas.drawRoundRect(trackpadRect, 16f * dp, 16f * dp, pBg)
        pBg.shader = null
        // Subtle tactile dot-matrix inside touchpad
        val dotP = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = 0x16FFFFFF }
        val step = 14f * dp
        var gy = trackpadRect.top + 16f * dp
        while (gy < trackpadRect.bottom - 14f * dp) {
            var gx = trackpadRect.left + 16f * dp
            while (gx < trackpadRect.right - 14f * dp) {
                canvas.drawCircle(gx, gy, 1.1f * dp, dotP)
                gx += step
            }
            gy += step
        }
        pBorder.strokeWidth = if (trackPointers.isNotEmpty()) 2f * dp else 1.4f * dp
        pBorder.color = if (trackPointers.isNotEmpty()) accent else PlayCraftTheme.BORDER_HAIRLINE
        canvas.drawRoundRect(trackpadRect, 16f * dp, 16f * dp, pBorder)

        pText.color = PlayCraftTheme.TEXT_WHITE
        pText.textSize = (trackpadRect.width() * 0.085f).coerceIn(11f * dp, 15f * dp)
        canvas.drawText("PC MOUSE TOUCHPAD", trackpadRect.centerX(), trackpadRect.centerY() - 4f * dp, pText)
        pSub.color = PlayCraftTheme.TEXT_MUTED
        pSub.textSize = (trackpadRect.width() * 0.055f).coerceIn(8.5f * dp, 11f * dp)
        canvas.drawText("Drag: Cursor \u2022 Tap: Click \u2022 2F: Right", trackpadRect.centerX(), trackpadRect.centerY() + 14f * dp, pSub)

        // Realistic Beveled Concave Thumbsticks
        drawRealisticStick(canvas, lStickRect, lStickX, lStickY, accent, accentSoft, dp)
        drawRealisticStick(canvas, rStickRect, rStickX, rStickY, accent, accentSoft, dp)

        // Buttons
        for (b in buttons) {
            val pressed = b.activePointers.isNotEmpty()
            val tintColor = if (isXbox) b.xboxTint else b.psTint
            pBg.shader = LinearGradient(
                b.rect.left, b.rect.top, b.rect.left, b.rect.bottom,
                if (pressed) ((tintColor and 0x00FFFFFF) or 0x44000000) else 0xFF1A2540.toInt(),
                0xFF0D1424.toInt(),
                Shader.TileMode.CLAMP
            )
            pBorder.strokeWidth = if (pressed || b.id == "PS") 2f * dp else 1.4f * dp
            pBorder.color = if (pressed) tintColor else if (b.id == "PS") accent else PlayCraftTheme.BORDER_HAIRLINE

            if (b.circle) {
                val r = min(b.rect.width(), b.rect.height()) / 2f
                canvas.drawCircle(b.rect.centerX(), b.rect.centerY(), r, pBg)
                canvas.drawCircle(b.rect.centerX(), b.rect.centerY(), r, pBorder)
            } else {
                canvas.drawRoundRect(b.rect, 11f * dp, 11f * dp, pBg)
                canvas.drawRoundRect(b.rect, 11f * dp, 11f * dp, pBorder)
            }
            pBg.shader = null

            pText.color = if (pressed) tintColor else if (b.circle) tintColor else PlayCraftTheme.TEXT_WHITE
            pText.textSize = (b.rect.height() * 0.40f).coerceIn(10f * dp, 19f * dp)
            val txt = if (isXbox) b.xboxLabel else b.psLabel
            canvas.drawText(txt, b.rect.centerX(), b.rect.centerY() + pText.textSize * 0.35f, pText)
        }
    }

    private fun drawRealisticStick(
        canvas: Canvas,
        r: RectF,
        sx: Float,
        sy: Float,
        accent: Int,
        accentSoft: Int,
        dp: Float
    ) {
        val radius = min(r.width(), r.height()) / 2f
        pBg.shader = RadialGradient(r.centerX(), r.centerY(), radius, 0xFF0A0F1B.toInt(), 0xFF162138.toInt(), Shader.TileMode.CLAMP)
        canvas.drawCircle(r.centerX(), r.centerY(), radius, pBg)
        pBg.shader = null
        pBorder.strokeWidth = 1.6f * dp
        pBorder.color = PlayCraftTheme.BORDER_HAIRLINE
        canvas.drawCircle(r.centerX(), r.centerY(), radius, pBorder)

        val maxOffset = radius * 0.48f
        val kx = r.centerX() + sx * maxOffset
        val ky = r.centerY() + sy * maxOffset
        val kr = radius * 0.47f

        // Knob elastomer ring
        pBg.shader = RadialGradient(kx - kr * 0.25f, ky - kr * 0.25f, kr * 1.25f, 0xFF334155.toInt(), 0xFF0F172A.toInt(), Shader.TileMode.CLAMP)
        canvas.drawCircle(kx, ky, kr, pBg)
        pBg.shader = null

        // Inner concave thumb dish
        pBg.color = if (sx != 0f || sy != 0f) accentSoft else 0xFF0D1422.toInt()
        canvas.drawCircle(kx, ky, kr * 0.72f, pBg)
        pBorder.strokeWidth = 1.8f * dp
        pBorder.color = accent
        canvas.drawCircle(kx, ky, kr, pBorder)
    }

    private fun keyFor(btn: String): String = config.loadKeymap()[btn] ?: ""

    @SuppressLint("ClickableViewAccessibility")
    override fun onTouchEvent(event: MotionEvent): Boolean {
        val action = event.actionMasked
        val idx = event.actionIndex

        when (action) {
            MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> {
                val pid = event.getPointerId(idx)
                val x = event.getX(idx)
                val y = event.getY(idx)
                if (lStickRect.contains(x, y) && lStickPid == -1) {
                    lStickPid = pid
                    updateStick(true, x, y)
                } else if (rStickRect.contains(x, y) && rStickPid == -1) {
                    rStickPid = pid
                    updateStick(false, x, y)
                } else if (trackpadRect.contains(x, y)) {
                    if (trackPointers.isEmpty()) {
                        trackMaxFingers = 0
                        trackMovedTotal = 0f
                        trackDownAt = System.currentTimeMillis()
                    }
                    trackPointers[pid] = Pair(x, y)
                    trackMaxFingers = maxOf(trackMaxFingers, trackPointers.size)
                } else {
                    for (b in buttons) {
                        if (b.rect.contains(x, y)) {
                            val wasEmpty = b.activePointers.isEmpty()
                            b.activePointers.add(pid)
                            if (wasEmpty) {
                                audio.hapticTap(10L, 130)
                                bridge.sendButton(b.id, true, keyFor(b.id))
                            }
                            break
                        }
                    }
                }
                invalidate()
            }

            MotionEvent.ACTION_MOVE -> {
                for (i in 0 until event.pointerCount) {
                    val pid = event.getPointerId(i)
                    val x = event.getX(i)
                    val y = event.getY(i)
                    if (pid == lStickPid) {
                        updateStick(true, x, y)
                    } else if (pid == rStickPid) {
                        updateStick(false, x, y)
                    } else if (trackPointers.containsKey(pid)) {
                        val prev = trackPointers[pid]!!
                        val dx = x - prev.first
                        val dy = y - prev.second
                        trackPointers[pid] = Pair(x, y)
                        trackMovedTotal += hypot(dx, dy)
                        if (trackPointers.size == 1) {
                            val sx = (dx * 1.4f).toInt()
                            val sy = (dy * 1.4f).toInt()
                            if (sx != 0 || sy != 0) bridge.sendMouseMove(sx, sy)
                        }
                    }
                }
                invalidate()
            }

            MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP, MotionEvent.ACTION_CANCEL -> {
                val releaseAll = action == MotionEvent.ACTION_CANCEL
                val pid = event.getPointerId(idx)
                if (releaseAll || pid == lStickPid) {
                    lStickPid = -1
                    lStickX = 0f
                    lStickY = 0f
                    bridge.sendStick(true, 0f, 0f)
                }
                if (releaseAll || pid == rStickPid) {
                    rStickPid = -1
                    rStickX = 0f
                    rStickY = 0f
                    bridge.sendStick(false, 0f, 0f)
                }
                if (releaseAll) {
                    trackPointers.clear()
                } else if (trackPointers.remove(pid) != null && trackPointers.isEmpty()) {
                    if (trackMovedTotal < 16f && System.currentTimeMillis() - trackDownAt < 280L) {
                        bridge.sendMouseClick(right = trackMaxFingers >= 2)
                    }
                }
                for (b in buttons) {
                    if (releaseAll && b.activePointers.isNotEmpty()) {
                        b.activePointers.clear()
                        bridge.sendButton(b.id, false, keyFor(b.id))
                    } else if (b.activePointers.remove(pid) && b.activePointers.isEmpty()) {
                        bridge.sendButton(b.id, false, keyFor(b.id))
                    }
                }
                invalidate()
            }
        }
        return true
    }

    private fun updateStick(left: Boolean, x: Float, y: Float) {
        val r = if (left) lStickRect else rStickRect
        val maxR = (min(r.width(), r.height()) * 0.34f).coerceAtLeast(16f)
        var dx = x - r.centerX()
        var dy = y - r.centerY()
        val d = hypot(dx, dy)
        if (d > maxR) {
            dx = dx / d * maxR
            dy = dy / d * maxR
        }
        val nx = (dx / maxR).coerceIn(-1f, 1f)
        val ny = (dy / maxR).coerceIn(-1f, 1f)
        if (left) {
            lStickX = nx
            lStickY = ny
        } else {
            rStickX = nx
            rStickY = ny
        }
        val now = System.currentTimeMillis()
        if (now - lastStickSentAt >= 14L) {
            lastStickSentAt = now
            bridge.sendStick(left, nx, ny)
        }
    }
}
