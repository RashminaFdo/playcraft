package dev.rashmina.playcraft.controller

import android.annotation.SuppressLint
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothSocket
import android.content.Context
import android.util.Base64
import org.json.JSONArray
import org.json.JSONObject
import java.io.BufferedInputStream
import java.io.BufferedOutputStream
import java.io.BufferedReader
import java.io.ByteArrayOutputStream
import java.io.InputStreamReader
import java.io.OutputStream
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.Inet4Address
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.NetworkInterface
import java.net.Socket
import java.security.SecureRandom
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
import kotlin.concurrent.thread

data class DiscoveredPc(
    val pcName: String,
    val ips: List<String>,
    val wsPort: Int,
    val btName: String,
    val btSpp: Boolean,
    val vigemActive: Boolean
)

enum class ConnectionState {
    DISCONNECTED,
    CONNECTING,
    RECONNECTING,
    CONNECTED,
    AUTH_ERROR
}

interface BridgeListener {
    fun onConnectionStateChanged(
        state: ConnectionState,
        transportLabel: String,
        detail: String,
        vigemActive: Boolean,
        pingMs: Int
    )

    fun onGameRumble(largeMotor: Int, smallMotor: Int)
}

/**
 * Unified multi-transport controller bridge supporting:
 *   - Wi-Fi LAN WebSocket
 *   - Phone Wi-Fi Hotspot WebSocket
 *   - USB Cable (ADB reverse 127.0.0.1 / RNDIS) WebSocket
 *   - Native Bluetooth Classic RFCOMM SPP (zero Wi-Fi required)
 * Includes UDP multi-interface PC auto-discovery, 2.5s ping/pong watchdog & infinite auto-reconnect.
 */
class NetworkBridge(
    private val context: Context,
    private val config: ControllerConfig,
    private val listener: BridgeListener
) {
    private val sendExec = Executors.newSingleThreadExecutor()
    private val scheduler = Executors.newSingleThreadScheduledExecutor()

    @Volatile private var wantConnected = false
    @Volatile private var activeTransport = PlayCraftTheme.TRANSPORT_WIFI
    @Volatile private var targetHost = ""
    @Volatile private var targetPort = PlayCraftTheme.DEFAULT_WS_PORT
    @Volatile private var targetPin = ""
    @Volatile private var targetBtMac = ""
    @Volatile private var targetBtName = ""

    @Volatile private var wsSocket: Socket? = null
    @Volatile private var wsOut: BufferedOutputStream? = null
    @Volatile private var btSocket: BluetoothSocket? = null
    @Volatile private var btOut: OutputStream? = null

    @Volatile private var isAuthed = false
    @Volatile private var serverHasVigem = true
    @Volatile private var lastPongAt = 0L
    @Volatile private var currentPingMs = 0
    @Volatile private var reconnectAttempt = 0
    private var heartbeatTask: ScheduledFuture<*>? = null
    private var reconnectTask: ScheduledFuture<*>? = null

    @Volatile private var currentState = ConnectionState.DISCONNECTED
    private val sendLock = Any()

    fun isConnectedAndAuthed(): Boolean = currentState == ConnectionState.CONNECTED && isAuthed

    fun connectWebSocketEndpoint(
        transportMode: String,
        host: String,
        port: Int,
        pin: String
    ) {
        activeTransport = transportMode
        targetHost = host.trim()
        targetPort = port
        targetPin = pin.trim()
        wantConnected = true
        reconnectAttempt = 0
        cancelTimers()
        closeSocketsOnly()
        startConnectingAsync()
    }

    fun connectBluetoothEndpoint(macAddress: String, deviceName: String, pin: String) {
        activeTransport = PlayCraftTheme.TRANSPORT_BLUETOOTH
        targetBtMac = macAddress.trim()
        targetBtName = deviceName.trim()
        targetPin = pin.trim()
        wantConnected = true
        reconnectAttempt = 0
        cancelTimers()
        closeSocketsOnly()
        startConnectingAsync()
    }

    fun disconnectManually() {
        wantConnected = false
        sendNeutralImmediate()
        cancelTimers()
        closeSocketsOnly()
        isAuthed = false
        listener.onGameRumble(0, 0)
        updateState(ConnectionState.DISCONNECTED, activeTransport, "Disconnected", serverHasVigem, 0)
    }

    fun triggerImmediateReconnectIfWanted() {
        if (!wantConnected) return
        if (currentState == ConnectionState.CONNECTED && System.currentTimeMillis() - lastPongAt < 5000L) {
            return
        }
        reconnectAttempt = 0
        closeSocketsOnly()
        startConnectingAsync()
    }

    fun sendButton(control: String, pressed: Boolean, customKey: String = "") {
        val obj = JSONObject()
        obj.put("packet_type", "button")
        obj.put("control", control)
        obj.put("state", if (pressed) "down" else "up")
        if (customKey.isNotEmpty()) obj.put("custom_key", customKey)
        sendPacket(obj)
    }

    fun sendTriggers(lt: Float?, rt: Float?) {
        val obj = JSONObject()
        obj.put("packet_type", "trigger")
        if (lt != null) obj.put("lt", lt.coerceIn(0f, 1f))
        if (rt != null) obj.put("rt", rt.coerceIn(0f, 1f))
        sendPacket(obj)
    }

    fun sendStick(left: Boolean, x: Float, y: Float) {
        val obj = JSONObject()
        obj.put("packet_type", if (left) "left_stick" else "right_stick")
        if (left) {
            obj.put("lx", x.coerceIn(-1f, 1f))
            obj.put("ly", y.coerceIn(-1f, 1f))
        } else {
            obj.put("rx", x.coerceIn(-1f, 1f))
            obj.put("ry", y.coerceIn(-1f, 1f))
        }
        sendPacket(obj)
    }

    fun sendGyroSteer(axis: Float, deg: Float) {
        val obj = JSONObject()
        obj.put("packet_type", "gyro_steer")
        obj.put("steer", axis.coerceIn(-1f, 1f))
        obj.put("tilt_deg", deg)
        sendPacket(obj)
    }

    fun sendMouseMove(dx: Int, dy: Int) {
        val obj = JSONObject()
        obj.put("packet_type", "mouse")
        obj.put("dx", dx)
        obj.put("dy", dy)
        sendPacket(obj)
    }

    fun sendMouseClick(right: Boolean) {
        val obj = JSONObject()
        obj.put("packet_type", "mouse")
        obj.put("control", if (right) "RIGHT_CLICK" else "LEFT_CLICK")
        sendPacket(obj)
    }

    fun sendNeutralImmediate() {
        val triggerObj = JSONObject().put("packet_type", "trigger").put("lt", 0f).put("rt", 0f)
        val steerObj = JSONObject().put("packet_type", "gyro_steer").put("steer", 0f).put("tilt_deg", 0f)
        sendPacket(triggerObj)
        sendPacket(steerObj)
    }

    private fun sendPacket(obj: JSONObject) {
        if (!isAuthed) return
        val driver = if (config.outputDriver == PlayCraftTheme.DRIVER_KEYS) "keyboard" else "x360"
        val style = if (config.deckMode == PlayCraftTheme.DECK_WHEEL) {
            "Racing Wheel App"
        } else if (config.shellTheme == PlayCraftTheme.SHELL_XBOX) {
            "Xbox App"
        } else {
            "DualSense App"
        }
        obj.put("input_mode", driver)
        obj.put("controller_style", style)
        obj.put("client_ts", System.currentTimeMillis())
        sendRawJson(obj.toString())
    }

    private fun sendRawJson(json: String) {
        sendExec.execute {
            synchronized(sendLock) {
                if (activeTransport == PlayCraftTheme.TRANSPORT_BLUETOOTH) {
                    val out = btOut ?: return@synchronized
                    runCatching {
                        out.write((json + "\n").toByteArray(Charsets.UTF_8))
                        out.flush()
                    }
                } else {
                    val out = wsOut ?: return@synchronized
                    runCatching {
                        writeWsTextFrame(out, json)
                    }
                }
            }
        }
    }

    private fun startConnectingAsync() {
        if (!wantConnected) return
        val label = transportDisplayName(activeTransport)
        val status = if (reconnectAttempt == 0) ConnectionState.CONNECTING else ConnectionState.RECONNECTING
        updateState(status, label, "Connecting via $label...", serverHasVigem, currentPingMs)

        thread(name = "playcraft-bridge-conn") {
            try {
                if (activeTransport == PlayCraftTheme.TRANSPORT_BLUETOOTH) {
                    runBluetoothLoop()
                } else {
                    runWebSocketLoop()
                }
            } catch (e: Exception) {
                if (wantConnected) {
                    scheduleReconnect(e.message ?: "Link dropped")
                }
            }
        }
    }

    private fun runWebSocketLoop() {
        val sock = Socket()
        sock.tcpNoDelay = true
        sock.soTimeout = 10000
        sock.connect(InetSocketAddress(targetHost, targetPort), 3500)
        wsSocket = sock

        val input = BufferedInputStream(sock.getInputStream())
        val output = BufferedOutputStream(sock.getOutputStream())
        wsOut = output

        // RFC 6455 HTTP/1.1 Upgrade handshake
        val keyBytes = ByteArray(16)
        SecureRandom().nextBytes(keyBytes)
        val wsKey = Base64.encodeToString(keyBytes, Base64.NO_WRAP)
        val req = buildString {
            append("GET /ws HTTP/1.1\r\n")
            append("Host: ").append(targetHost).append(":").append(targetPort).append("\r\n")
            append("Upgrade: websocket\r\n")
            append("Connection: Upgrade\r\n")
            append("Sec-WebSocket-Key: ").append(wsKey).append("\r\n")
            append("Sec-WebSocket-Version: 13\r\n\r\n")
        }
        output.write(req.toByteArray(Charsets.US_ASCII))
        output.flush()

        val headerBuf = ByteArrayOutputStream()
        var tail = 0
        while (headerBuf.size() < 4096) {
            val b = input.read()
            if (b < 0) throw IllegalStateException("Handshake closed")
            headerBuf.write(b)
            tail = ((tail shl 8) or (b and 0xFF))
            if (tail == 0x0D0A0D0A) break
        }
        val headers = headerBuf.toString("US-ASCII")
        if (!headers.contains("101")) {
            throw IllegalStateException("WebSocket rejected")
        }

        sock.soTimeout = 14000
        reconnectAttempt = 0
        lastPongAt = System.currentTimeMillis()
        isAuthed = false
        startHeartbeatWatchdog()

        // Authenticate immediately
        val authJson = JSONObject()
            .put("packet_type", "auth")
            .put("pin", targetPin)
            .put("controller_style", "PlayCraft Android App")
            .toString()
        synchronized(sendLock) { writeWsTextFrame(output, authJson) }

        while (wantConnected) {
            val frame = readWsFrame(input) ?: break
            lastPongAt = System.currentTimeMillis()
            if (frame.opcode == 0x8) { // Close
                break
            } else if (frame.opcode == 0x9) { // Ping -> Pong
                synchronized(sendLock) { writeWsFrame(output, 0xA, frame.payload) }
            } else if (frame.opcode == 0x1) { // Text
                handleServerMessage(String(frame.payload, Charsets.UTF_8))
            }
        }

        isAuthed = false
        closeSocketsOnly()
        if (wantConnected && currentState != ConnectionState.AUTH_ERROR) {
            scheduleReconnect("Reconnecting...")
        }
    }

    @SuppressLint("MissingPermission")
    private fun runBluetoothLoop() {
        val mgr = context.getSystemService(Context.BLUETOOTH_SERVICE) as? BluetoothManager
        val adapter = mgr?.adapter ?: throw IllegalStateException("Bluetooth unavailable")
        if (!adapter.isEnabled) throw IllegalStateException("Bluetooth is turned OFF")

        val device: BluetoothDevice = adapter.getRemoteDevice(targetBtMac)
        runCatching { adapter.cancelDiscovery() }

        val sock = try {
            device.createRfcommSocketToServiceRecord(PlayCraftTheme.BT_SPP_UUID).also { it.connect() }
        } catch (_: Exception) {
            device.createInsecureRfcommSocketToServiceRecord(PlayCraftTheme.BT_SPP_UUID).also { it.connect() }
        }
        btSocket = sock
        btOut = sock.outputStream

        reconnectAttempt = 0
        lastPongAt = System.currentTimeMillis()
        isAuthed = false
        startHeartbeatWatchdog()

        val authJson = JSONObject()
            .put("packet_type", "auth")
            .put("pin", targetPin)
            .put("controller_style", "PlayCraft Android Bluetooth")
            .toString()
        synchronized(sendLock) {
            btOut?.write((authJson + "\n").toByteArray(Charsets.UTF_8))
            btOut?.flush()
        }

        val reader = BufferedReader(InputStreamReader(sock.inputStream, Charsets.UTF_8))
        while (wantConnected) {
            val line = reader.readLine() ?: break
            lastPongAt = System.currentTimeMillis()
            if (line.isNotBlank()) handleServerMessage(line)
        }

        isAuthed = false
        closeSocketsOnly()
        if (wantConnected && currentState != ConnectionState.AUTH_ERROR) {
            scheduleReconnect("BT Reconnecting...")
        }
    }

    private fun handleServerMessage(raw: String) {
        val msg = runCatching { JSONObject(raw) }.getOrNull() ?: return
        when (msg.optString("type")) {
            "auth_ok" -> {
                isAuthed = true
                serverHasVigem = msg.optBoolean("vigem", true)
                val pcName = msg.optString("pc_name", config.savedPcName)
                val transport = msg.optString("transport", transportDisplayName(activeTransport))
                config.saveSuccessfulEndpoint(
                    transport = activeTransport,
                    host = targetHost,
                    port = targetPort,
                    pin = targetPin,
                    btMac = targetBtMac,
                    btName = targetBtName,
                    pcName = pcName
                )
                updateState(
                    ConnectionState.CONNECTED,
                    transport,
                    if (serverHasVigem) "X360 Ready • $pcName" else "Keys Mode • $pcName",
                    serverHasVigem,
                    currentPingMs
                )
            }
            "auth_err" -> {
                isAuthed = false
                wantConnected = false
                cancelTimers()
                closeSocketsOnly()
                updateState(
                    ConnectionState.AUTH_ERROR,
                    transportDisplayName(activeTransport),
                    msg.optString("message", "Wrong PIN"),
                    serverHasVigem,
                    0
                )
            }
            "pong" -> {
                serverHasVigem = msg.optBoolean("vigem", serverHasVigem)
                val sentTs = msg.optLong("ts", 0L)
                if (sentTs > 0L) {
                    currentPingMs = (System.currentTimeMillis() - sentTs).toInt().coerceIn(1, 999)
                }
                if (isAuthed) {
                    updateState(
                        ConnectionState.CONNECTED,
                        transportDisplayName(activeTransport),
                        if (serverHasVigem) "X360 Ready" else "Keys Mode",
                        serverHasVigem,
                        currentPingMs
                    )
                }
            }
            "rumble" -> {
                val large = msg.optInt("large", 0)
                val small = msg.optInt("small", 0)
                listener.onGameRumble(large, small)
            }
        }
    }

    private fun startHeartbeatWatchdog() {
        heartbeatTask?.cancel(false)
        heartbeatTask = scheduler.scheduleAtFixedRate({
            if (!wantConnected) return@scheduleAtFixedRate
            val silentMs = System.currentTimeMillis() - lastPongAt
            if (silentMs > 7000L) {
                closeSocketsOnly()
                return@scheduleAtFixedRate
            }
            val pingObj = JSONObject()
                .put("packet_type", "ping")
                .put("client_ts", System.currentTimeMillis())
                .toString()
            sendRawJson(pingObj)
        }, 2500L, 2500L, TimeUnit.MILLISECONDS)
    }

    private fun scheduleReconnect(reason: String) {
        if (!wantConnected) return
        heartbeatTask?.cancel(false)
        reconnectAttempt += 1
        val delayMs = (600L + reconnectAttempt * 450L).coerceAtMost(2500L)
        updateState(
            ConnectionState.RECONNECTING,
            transportDisplayName(activeTransport),
            "$reason (${delayMs}ms)",
            serverHasVigem,
            0
        )
        reconnectTask?.cancel(false)
        reconnectTask = scheduler.schedule({
            if (wantConnected) startConnectingAsync()
        }, delayMs, TimeUnit.MILLISECONDS)
    }

    private fun cancelTimers() {
        heartbeatTask?.cancel(false)
        reconnectTask?.cancel(false)
    }

    private fun closeSocketsOnly() {
        synchronized(sendLock) {
            runCatching { wsOut?.close() }
            runCatching { wsSocket?.close() }
            runCatching { btOut?.close() }
            runCatching { btSocket?.close() }
            wsOut = null
            wsSocket = null
            btOut = null
            btSocket = null
        }
    }

    private fun updateState(
        state: ConnectionState,
        transportLabel: String,
        detail: String,
        vigem: Boolean,
        pingMs: Int
    ) {
        currentState = state
        listener.onConnectionStateChanged(state, transportLabel, detail, vigem, pingMs)
    }

    private fun transportDisplayName(mode: String): String = when (mode) {
        PlayCraftTheme.TRANSPORT_HOTSPOT -> "Hotspot"
        PlayCraftTheme.TRANSPORT_BLUETOOTH -> "Bluetooth"
        PlayCraftTheme.TRANSPORT_USB -> "USB"
        else -> "Wi-Fi"
    }

    private data class WsFrame(val opcode: Int, val payload: ByteArray)

    private fun writeWsTextFrame(out: OutputStream, text: String) {
        writeWsFrame(out, 0x1, text.toByteArray(Charsets.UTF_8))
    }

    private fun writeWsFrame(out: OutputStream, opcode: Int, payload: ByteArray) {
        val hdr = ByteArrayOutputStream()
        hdr.write(0x80 or (opcode and 0x0F))
        val len = payload.size
        when {
            len <= 125 -> hdr.write(0x80 or len)
            len <= 65535 -> {
                hdr.write(0x80 or 126)
                hdr.write((len shr 8) and 0xFF)
                hdr.write(len and 0xFF)
            }
            else -> {
                hdr.write(0x80 or 127)
                for (shift in 56 downTo 0 step 8) {
                    hdr.write(((len.toLong() shr shift) and 0xFFL).toInt())
                }
            }
        }
        val mask = ByteArray(4)
        SecureRandom().nextBytes(mask)
        hdr.write(mask)
        val masked = ByteArray(len)
        for (i in 0 until len) {
            masked[i] = (payload[i].toInt() xor mask[i and 3].toInt()).toByte()
        }
        out.write(hdr.toByteArray())
        out.write(masked)
        out.flush()
    }

    private fun readWsFrame(inp: BufferedInputStream): WsFrame? {
        val b1 = inp.read()
        val b2 = inp.read()
        if (b1 < 0 || b2 < 0) return null
        val opcode = b1 and 0x0F
        val masked = (b2 and 0x80) != 0
        var len = (b2 and 0x7F).toLong()
        if (len == 126L) {
            val x1 = inp.read()
            val x2 = inp.read()
            if (x1 < 0 || x2 < 0) return null
            len = ((x1 shl 8) or x2).toLong()
        } else if (len == 127L) {
            var acc = 0L
            for (i in 0 until 8) {
                val x = inp.read()
                if (x < 0) return null
                acc = (acc shl 8) or x.toLong()
            }
            len = acc
        }
        val mask = if (masked) {
            ByteArray(4).also { m ->
                for (i in 0..3) {
                    val v = inp.read()
                    if (v < 0) return null
                    m[i] = v.toByte()
                }
            }
        } else null
        val size = len.toInt().coerceIn(0, 65536)
        val data = ByteArray(size)
        var off = 0
        while (off < size) {
            val r = inp.read(data, off, size - off)
            if (r <= 0) return null
            off += r
        }
        if (mask != null) {
            for (i in 0 until size) {
                data[i] = (data[i].toInt() xor mask[i and 3].toInt()).toByte()
            }
        }
        return WsFrame(opcode, data)
    }

    @SuppressLint("MissingPermission")
    fun listPairedBluetoothDevices(): List<BluetoothDevice> {
        val mgr = context.getSystemService(Context.BLUETOOTH_SERVICE) as? BluetoothManager ?: return emptyList()
        val adapter = mgr.adapter ?: return emptyList()
        return runCatching { adapter.bondedDevices?.toList() ?: emptyList() }.getOrDefault(emptyList())
    }

    /**
     * Multi-subnet UDP auto-finder scanning Wi-Fi LAN, Android Hotspot clients, USB RNDIS, and 127.0.0.1.
     */
    fun discoverPlayCraftPcs(timeoutMs: Int = 900): List<DiscoveredPc> {
        val results = linkedMapOf<String, DiscoveredPc>()
        val probeBytes = "PLAYCRAFT_DISCOVER".toByteArray(Charsets.UTF_8)
        runCatching {
            val udp = DatagramSocket()
            udp.broadcast = true
            udp.soTimeout = timeoutMs

            val targets = linkedSetOf<String>()
            targets.add("255.255.255.255")
            targets.add("127.0.0.1")
            targets.add("192.168.43.255")
            targets.add("192.168.137.1")
            targets.add("192.168.137.255")

            runCatching {
                val ifaces = NetworkInterface.getNetworkInterfaces()
                while (ifaces.hasMoreElements()) {
                    val nic = ifaces.nextElement()
                    if (!nic.isUp || nic.isLoopback) continue
                    for (ia in nic.interfaceAddresses) {
                        val bcast = ia.broadcast
                        if (bcast is Inet4Address) {
                            targets.add(bcast.hostAddress ?: continue)
                        } else if (ia.address is Inet4Address) {
                            val parts = ia.address.hostAddress?.split(".") ?: continue
                            if (parts.size == 4) {
                                targets.add("${parts[0]}.${parts[1]}.${parts[2]}.255")
                            }
                        }
                    }
                }
            }

            // Probe directly for USB/Hotspot common addresses too
            for (t in targets) {
                runCatching {
                    val addr = InetAddress.getByName(t)
                    udp.send(DatagramPacket(probeBytes, probeBytes.size, addr, config.savedPort.let { PlayCraftTheme.DEFAULT_DISCOVERY_PORT }))
                }
            }

            val endAt = System.currentTimeMillis() + timeoutMs
            val recvBuf = ByteArray(1536)
            while (System.currentTimeMillis() < endAt) {
                val pkt = DatagramPacket(recvBuf, recvBuf.size)
                runCatching { udp.receive(pkt) }.onFailure { break }
                val str = String(pkt.data, 0, pkt.length, Charsets.UTF_8)
                val json = runCatching { JSONObject(str) }.getOrNull() ?: continue
                if (json.optString("app") != "PlayCraft") continue
                val pcName = json.optString("pc_name", pkt.address.hostAddress ?: "PlayCraft-PC")
                val ipsArr = json.optJSONArray("ips") ?: JSONArray()
                val ips = mutableListOf<String>()
                pkt.address.hostAddress?.let { if (!ips.contains(it)) ips.add(it) }
                for (i in 0 until ipsArr.length()) {
                    val ip = ipsArr.optString(i)
                    if (ip.isNotBlank() && !ips.contains(ip)) ips.add(ip)
                }
                results[pcName] = DiscoveredPc(
                    pcName = pcName,
                    ips = ips,
                    wsPort = json.optInt("ws_port", PlayCraftTheme.DEFAULT_WS_PORT),
                    btName = json.optString("bt_name", ""),
                    btSpp = json.optBoolean("bt_spp", false),
                    vigemActive = json.optBoolean("vigem", false)
                )
            }
            udp.close()
        }
        return results.values.toList()
    }

    fun release() {
        disconnectManually()
        sendExec.shutdownNow()
        scheduler.shutdownNow()
    }
}
