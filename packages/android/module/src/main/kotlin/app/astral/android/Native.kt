package app.astral.android

object Native {
    @JvmStatic external fun preparePhoenixAccess(libraryDirectory: String, apks: Array<String>): String
    @JvmStatic external fun openPhoenixAccess(): String

    @JvmStatic external fun initialize(
        propertyNames: Array<String>?,
        dataDirectory: String,
        resourceDirectory: String,
        languages: Array<String>,
        discordVersion: String,
        environment: String,
    ): Int

    @JvmStatic external fun rewrite(ops: ByteArray, length: Int): ByteArray?
    @JvmStatic external fun renderSvg(svg: ByteArray, width: Int, height: Int): ByteArray?

    @JvmStatic external fun plugins(): String

    @JvmStatic external fun setEnabled(identifier: String, enabled: Boolean)

    @JvmStatic external fun eval(identifier: String, code: String): String

    @JvmStatic external fun pendingUpdates(): ByteArray?
    @JvmStatic external fun tweensActive(): Boolean
    @JvmStatic external fun tweenFrame(nowMs: Double): ByteArray?

    @JvmStatic external fun composerEvent(event: ByteArray): ByteArray?
    @JvmStatic external fun back(): Boolean
    @JvmStatic external fun refreshDecorations()
    @JvmStatic external fun pageOpen(): Boolean

    @JvmStatic external fun attachBridge(bridge: Class<*>)

    @JvmStatic external fun completeRequest(
        id: Long,
        status: Int,
        ok: Boolean,
        url: String,
        headers: String,
        body: String,
        bodyBase64: String?,
        error: String?,
    )

    @JvmStatic external fun completeFilePick(id: Long, name: String?, data: ByteArray?, error: String?)

    @JvmStatic external fun completeConfirm(id: Long, accepted: Boolean)
    @JvmStatic external fun sheetEvent(id: Long, action: String, value: String, closed: Boolean)
    @JvmStatic external fun completeWebAuth(id: Long, url: String?, cookies: String?, error: String?)

    @JvmStatic external fun completeTextEdit(id: Long, value: String?)

    @JvmStatic external fun emitEvent(event: String)

    @JvmStatic external fun processGatewayMessage(message: String): String?
    @JvmStatic external fun needsGatewayEvents(): Boolean
    @JvmStatic external fun rewriteHostCall(frame: ByteArray): ByteArray?
    @JvmStatic external fun hostRequestWanted(frame: ByteArray): Boolean
    @JvmStatic external fun interceptHostRequest(frame: ByteArray): ByteArray?

    @JvmStatic external fun recordExitReport(kind: String, timestamp: Long, reason: Int, description: String, trace: String)

    @JvmStatic external fun staffDevToolsEnabled(): Boolean

    @JvmStatic external fun withStaffReady(message: String): String?

    @JvmStatic external fun guestEvent(event: ByteArray): Int
    @JvmStatic external fun pickedMediaFetched(entries: String)
    @JvmStatic external fun webAuthInitialCookies(request: Long, header: String)
    @JvmStatic external fun webAuthIsCallback(request: Long, url: String): Boolean
    @JvmStatic external fun webAuthNewCookies(request: Long, header: String): String
    @JvmStatic external fun networkPlan(method: String, url: String): Int
    @JvmStatic external fun networkBegin(request: String, stages: Int): String
    @JvmStatic external fun networkFinish(exchange: Long, response: String): String?
    @JvmStatic external fun networkAbandon(exchange: Long)

    @JvmStatic external fun noteBlockedRequest()

    @JvmStatic external fun sinkUrl(): String

    @JvmStatic external fun iconPath(name: String): String

    @JvmStatic external fun anchoredClasses(apk: String, anchors: String): String

    @JvmStatic external fun emojiURL(codepoints: String): String?

    @JvmStatic external fun composerStyles(text: String): String

    @JvmStatic external fun markVisible()

    @JvmStatic external fun markHidden()

    @JvmStatic external fun markReady()
    @JvmStatic external fun openLink(url: String): Boolean

    @JvmStatic external fun exportBackup(): String

    @JvmStatic external fun applyBackup(contents: String): String?
}
