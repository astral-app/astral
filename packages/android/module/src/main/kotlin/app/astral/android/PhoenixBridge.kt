package app.astral.android

import android.app.Activity
import android.app.AlertDialog
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Intent
import android.net.Uri
import java.util.Locale
import android.speech.tts.TextToSpeech
import android.media.MediaPlayer
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.os.SystemClock
import android.view.HapticFeedbackConstants
import android.text.InputFilter
import android.text.InputType
import android.graphics.BitmapFactory
import android.widget.EditText
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.Toast
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import io.github.libxposed.api.XposedInterface.ExceptionMode
import io.github.libxposed.api.XposedModule
import java.lang.reflect.Method
import java.lang.reflect.Proxy
import java.lang.ref.WeakReference
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicBoolean

object PhoenixBridge {
    private const val GUEST_HOST = "com.discord.nativeapp.bridge.GuestHost"
    private const val RETRY_INTERVAL_MS = 1000L
    private const val TWEEN_FRAME_MS = 16L
    private const val NOTIFICATION_CHANNEL = "astral"
    private var speech: TextToSpeech? = null
    private var speechReady = false
    private val speechQueue = mutableListOf<(TextToSpeech) -> Unit>()
    private val speechIds = java.util.concurrent.atomic.AtomicInteger(0)
    private var player: MediaPlayer? = null
    private val notificationIds = java.util.concurrent.atomic.AtomicInteger(0x7a57)
    private val tweening = java.util.concurrent.atomic.AtomicBoolean(false)

    private const val GUEST_EVENT_PASS = 0
    private const val GUEST_EVENT_CLOSE_MENU = 2
    private const val FUNCTION0 = "kotlin.jvm.functions.Function0"
    private const val MAIN_ACTIVITY = "com.discord.nativeapp.MainActivity"
    private const val EXPORT_BACKUP_REQUEST = 0xA571
    private const val IMPORT_BACKUP_REQUEST = 0xA572
    private const val SYNC_FOLDER_REQUEST = 0xA573
    private const val PICK_FILE_REQUEST = 0xA573
    private const val MAXIMUM_BACKUP_BYTES = 1 shl 20

    @Volatile private var guestHost: Any? = null
    private var applyOps: Method? = null
    private val delivering = ThreadLocal.withInitial { false }
    private val worker = HandlerThread("astral-wire").apply { start() }
    private val handler = Handler(worker.looper)
    private val mainHandler = Handler(Looper.getMainLooper())
    private data class PendingWebAuth(val activity: WeakReference<Activity>, val finish: (String?, String?, String?) -> Unit)
    @Volatile private var currentActivity: WeakReference<Activity>? = null
    @Volatile private var pendingExport: String? = null
    @Volatile private var pendingFilePick: Long? = null
    private val pendingWebAuth = ConcurrentHashMap<Long, PendingWebAuth>()

    fun install(module: XposedModule, loader: ClassLoader) {
        val type = loader.loadClass(GUEST_HOST)
        val method = type.getDeclaredMethod("applyOps", ByteArray::class.java, Int::class.javaPrimitiveType)
        method.isAccessible = true
        applyOps = method
        module.hook(method)
            .setExceptionMode(ExceptionMode.PROTECTIVE)
            .intercept { chain ->
                guestHost = chain.thisObject
                if (delivering.get() == true) {
                    Capture.record(chain.getArg(0) as ByteArray, chain.getArg(1) as Int)
                    return@intercept chain.proceed()
                }
                val ops = chain.getArg(0) as ByteArray
                val length = chain.getArg(1) as Int
                Capture.record(ops, length)
                val rewritten = Native.rewrite(ops, length)
                NavigationBack.refresh()
                if (rewritten != null) Capture.record(rewritten, rewritten.size)
                val result = if (rewritten == null) chain.proceed() else chain.proceed(arrayOf<Any>(rewritten, rewritten.size))
                scheduleTween()
                result
            }
        hookHostCalls(module, type)
        hookEvents(module, type, loader)
        installBackupActions(module, loader)
        handler.postDelayed(::retry, RETRY_INTERVAL_MS)
        Log.info("Hooked GuestHost.applyOps")
    }

    private val resending = ThreadLocal.withInitial { false }
    private val requests = java.util.concurrent.Executors.newCachedThreadPool()

    private fun resendRequest(host: Any?, invoke: Method, frame: ByteArray) {
        val outgoing = runCatching { Native.interceptHostRequest(frame) }
            .onFailure { Log.error("Failed to run plugins on a Discord request", it) }
            .getOrNull() ?: frame
        resending.set(true)
        try {
            invoke.invoke(host, outgoing)
        } catch (error: Throwable) {
            Log.error("Failed to send a Discord request after plugins ran", error)
        } finally {
            resending.set(false)
        }
    }

    private fun hookHostCalls(module: XposedModule, type: Class<*>) {
        val invoke = runCatching { type.getDeclaredMethod("invoke", ByteArray::class.java) }.getOrNull()
            ?: return Log.warning("GuestHost has no invoke method; host calls are not rewritten")
        module.hook(invoke)
            .setExceptionMode(ExceptionMode.PROTECTIVE)
            .intercept { chain ->
                val frame = chain.getArg(0) as? ByteArray
                if (frame == null || frame.size < 16 || frame[0].toInt() != 1 || resending.get() == true) return@intercept chain.proceed()
                val host = chain.thisObject
                if (runCatching { Native.hostRequestWanted(frame) }.getOrDefault(false)) {
                    requests.execute { resendRequest(host, invoke, frame) }
                    return@intercept null
                }
                val rewritten = Native.rewriteHostCall(frame)
                if (rewritten == null) chain.proceed() else chain.proceed(arrayOf<Any>(rewritten))
            }
        Log.info("Hooked GuestHost.invoke for host calls")
    }

    private fun hookEvents(module: XposedModule, type: Class<*>, loader: ClassLoader) {
        val function0 = loader.loadClass(FUNCTION0)
        val enqueue = type.declaredMethods.firstOrNull {
            it.returnType == Void.TYPE && it.parameterTypes.contentEquals(arrayOf(ByteArray::class.java, function0))
        } ?: return Log.warning("GuestHost has no event method; astral's settings rows stay inert")
        module.hook(enqueue)
            .setExceptionMode(ExceptionMode.PROTECTIVE)
            .intercept { chain ->
                val event = chain.getArg(0) as? ByteArray
                event?.let(Native::composerEvent)?.let { before -> deliverEvent(chain.thisObject, enqueue, function0, before) }
                if (event != null && handled(event)) {
                    chain.getArg(1)?.let { done -> handler.post { runCatching { done.javaClass.getMethod("invoke").invoke(done) } } }
                    null
                } else {
                    chain.proceed()
                }
            }
        Log.info("Hooked GuestHost.${enqueue.name} for events")
    }

    private fun deliverEvent(host: Any?, enqueue: Method, function0: Class<*>, event: ByteArray) {
        val done = Proxy.newProxyInstance(function0.classLoader, arrayOf(function0)) { proxy, method, arguments ->
            when (method.name) {
                "equals" -> proxy === arguments?.firstOrNull()
                "hashCode" -> System.identityHashCode(proxy)
                "toString" -> "astral.event"
                else -> Unit
            }
        }
        runCatching { enqueue.invoke(host, event, done) }.onFailure { Log.error("Failed to deliver the rewritten message text", it) }
    }

    private fun handled(event: ByteArray): Boolean {
        val outcome = runCatching { Native.guestEvent(event) }.getOrDefault(GUEST_EVENT_PASS)
        if (outcome == GUEST_EVENT_CLOSE_MENU) mainHandler.post { dismissMessageMenu() }
        return outcome != GUEST_EVENT_PASS
    }

    private fun dismissMessageMenu() {
        val activity = activeActivity() ?: return
        runCatching {
            @Suppress("DEPRECATION")
            activity.onBackPressed()
        }.onFailure { Log.error("Failed to close the message menu", it) }
    }

    private fun installBackupActions(module: XposedModule, loader: ClassLoader) {
        val mainActivity = loader.loadClass(MAIN_ACTIVITY)
        val onResume = activityMethod(mainActivity, "onResume", emptyArray())
        if (onResume != null) {
            module.hook(onResume)
                .setExceptionMode(ExceptionMode.PROTECTIVE)
                .intercept { chain ->
                    val result = chain.proceed()
                    val activity = chain.thisObject as? Activity
                    if (activity != null && activity.javaClass.name == MAIN_ACTIVITY) {
                        currentActivity = WeakReference(activity)
                        Native.emitEvent("app.foreground")
                    }
                    result
                }
        }
        val onPause = activityMethod(mainActivity, "onPause", emptyArray())
        if (onPause != null) {
            module.hook(onPause)
                .setExceptionMode(ExceptionMode.PROTECTIVE)
                .intercept { chain ->
                    val result = chain.proceed()
                    val activity = chain.thisObject as? Activity
                    if (activity != null && activity.javaClass.name == MAIN_ACTIVITY) Native.emitEvent("app.background")
                    result
                }
        }
        val onDestroy = activityMethod(mainActivity, "onDestroy", emptyArray())
        if (onDestroy != null) {
            module.hook(onDestroy)
                .setExceptionMode(ExceptionMode.PROTECTIVE)
                .intercept { chain ->
                    val activity = chain.thisObject as? Activity
                    if (activity != null) {
                        pendingWebAuth.entries.filter { it.value.activity.get() === activity }.forEach { entry ->
                            entry.value.finish(null, null, "cancelled")
                        }
                    }
                    val result = chain.proceed()
                    if (activity != null && currentActivity?.get() === activity) currentActivity = null
                    result
                }
        }
        val resultMethod = activityMethod(
            mainActivity,
            "onActivityResult",
            arrayOf(Int::class.javaPrimitiveType!!, Int::class.javaPrimitiveType!!, Intent::class.java),
        ) ?: return Log.warning("Phoenix activity has no document result callback; backups stay unavailable")
        module.hook(resultMethod)
            .setExceptionMode(ExceptionMode.PROTECTIVE)
            .intercept { chain ->
                val request = chain.getArg(0) as? Int ?: return@intercept chain.proceed()
                if (request != EXPORT_BACKUP_REQUEST && request != IMPORT_BACKUP_REQUEST && request != PICK_FILE_REQUEST && request != SYNC_FOLDER_REQUEST) {
                    return@intercept chain.proceed()
                }
                val resultValue = chain.proceed()
                val activity = chain.thisObject as? Activity ?: return@intercept resultValue
                val result = chain.getArg(1) as? Int ?: Activity.RESULT_CANCELED
                val data = chain.getArg(2) as? Intent
                if (request == SYNC_FOLDER_REQUEST) {
                    val tree = if (result == Activity.RESULT_OK) data?.data else null
                    if (tree != null) runCatching { SyncFolder.chosen(activity, tree) }.onFailure { Log.error("Failed to keep access to the sync folder", it) }
                    return@intercept resultValue
                }
                if (request == PICK_FILE_REQUEST) {
                    val requestID = pendingFilePick
                    pendingFilePick = null
                    if (requestID != null) {
                        val uri = if (result == Activity.RESULT_OK) data?.data else null
                        if (uri == null) {
                            Native.completeFilePick(requestID, null, null, "cancelled")
                        } else {
                            completeFilePick(activity, requestID, uri)
                        }
                    }
                    return@intercept resultValue
                }
                if (result == Activity.RESULT_OK) {
                    val uri = data?.data
                    if (uri == null) {
                        showBackupStatus(activity, "No document was selected")
                    } else if (request == EXPORT_BACKUP_REQUEST) {
                        saveBackup(activity, uri)
                    } else {
                        loadBackup(activity, uri)
                    }
                }
                if (request == EXPORT_BACKUP_REQUEST) pendingExport = null
                resultValue
            }
        Log.info("Hooked Phoenix document picker results")
    }

    private fun activityMethod(type: Class<*>, name: String, parameters: Array<Class<*>>): Method? {
        var current: Class<*>? = type
        while (current != null) {
            val method = runCatching { current.getDeclaredMethod(name, *parameters) }.getOrNull()
            if (method != null) return method.apply { isAccessible = true }
            current = current.superclass
        }
        return null
    }

    private fun activeActivity(): Activity? = currentActivity?.get()

    private fun startDocumentPicker(intent: Intent, request: Int) {
        mainHandler.post {
            val activity = activeActivity()
            if (activity == null) {
                Log.warning("Phoenix is not visible; the document picker could not open")
                return@post
            }
            runCatching { activity.startActivityForResult(intent, request) }
                .onFailure {
                    if (request == EXPORT_BACKUP_REQUEST) pendingExport = null
                    showBackupStatus(activity, "Could not open the document picker")
                }
        }
    }

    private fun saveBackup(activity: Activity, uri: Uri) {
        val contents = pendingExport ?: runCatching { Native.exportBackup() }.getOrNull()
        if (contents.isNullOrEmpty()) return showBackupStatus(activity, "Could not create the backup")
        runCatching {
            activity.contentResolver.openOutputStream(uri)?.bufferedWriter(Charsets.UTF_8)?.use { it.write(contents) }
                ?: throw IllegalStateException("The selected document could not be opened")
        }.onSuccess { showBackupStatus(activity, "Backup exported") }
            .onFailure { showBackupStatus(activity, "Could not write the backup") }
    }

    private fun loadBackup(activity: Activity, uri: Uri) {
        runCatching {
            val bytes = activity.contentResolver.openInputStream(uri)?.use { input -> input.readNBytes(MAXIMUM_BACKUP_BYTES + 1) }
                ?: throw IllegalStateException("The selected document could not be opened")
            if (bytes.size > MAXIMUM_BACKUP_BYTES) throw IllegalArgumentException("Backup exceeds 1 MiB")
            val failure = Native.applyBackup(bytes.toString(Charsets.UTF_8))
            if (failure != null) throw IllegalArgumentException(failure)
        }.onSuccess { showBackupStatus(activity, "Settings imported") }
            .onFailure { error -> showBackupStatus(activity, error.message ?: "Could not import the backup") }
    }

    private fun showBackupStatus(activity: Activity, message: String) {
        mainHandler.post { Toast.makeText(activity, message, Toast.LENGTH_LONG).show() }
    }

    private fun completeFilePick(activity: Activity, request: Long, uri: Uri) {
        runCatching {
            val name = activity.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
                if (cursor.moveToFirst()) cursor.getString(0) else null
            } ?: uri.lastPathSegment?.substringAfterLast('/') ?: "file"
            val bytes = activity.contentResolver.openInputStream(uri)?.use { input -> input.readNBytes(MAXIMUM_BACKUP_BYTES + 1) }
                ?: throw IllegalStateException("file_unreadable")
            if (bytes.size > MAXIMUM_BACKUP_BYTES) throw IllegalArgumentException("file_too_large")
            Native.completeFilePick(request, name, bytes, null)
        }.onFailure { error -> Native.completeFilePick(request, null, null, error.message ?: "file_unreadable") }
    }

    @JvmStatic
    fun showAlert(title: String, message: String) {
        mainHandler.post {
            val activity = activeActivity() ?: return@post
            if (activity.isFinishing || activity.isDestroyed) return@post
            AlertDialog.Builder(activity).setTitle(title).setMessage(message).setPositiveButton("OK", null).show()
        }
    }

    @JvmStatic
    fun environmentFacts(): String = EnvironmentFacts.collect()

    @JvmStatic
    fun openSheet(sheet: Long, spec: String) {
        val options = runCatching { org.json.JSONObject(spec) }.getOrNull()
        mainHandler.post {
            val activity = activeActivity()
            if (options == null || activity == null || activity.isFinishing || activity.isDestroyed) {
                Native.sheetEvent(sheet, "", "", true)
                return@post
            }
            PluginSheet.open(activity, sheet, options) { action, value, closed -> Native.sheetEvent(sheet, action, value, closed) }
        }
    }

    @JvmStatic
    fun updateSheet(sheet: Long, spec: String) {
        val options = runCatching { org.json.JSONObject(spec) }.getOrNull() ?: return
        mainHandler.post { PluginSheet.update(sheet, options) }
    }

    @JvmStatic
    fun closeSheet(sheet: Long) {
        mainHandler.post { PluginSheet.close(sheet) }
    }

    @JvmStatic
    fun showConfirm(request: Long, spec: String) {
        val options = runCatching { org.json.JSONObject(spec) }.getOrNull()
        mainHandler.post {
            val activity = activeActivity()
            if (options == null || activity == null || activity.isFinishing || activity.isDestroyed) {
                Native.completeConfirm(request, false)
                return@post
            }
            var answered = false
            fun answer(accepted: Boolean) {
                if (answered) return
                answered = true
                Native.completeConfirm(request, accepted)
            }
            val builder = AlertDialog.Builder(activity)
                .setTitle(options.optString("title"))
                .setPositiveButton(options.optString("confirm", "OK")) { _, _ -> answer(true) }
                .setNegativeButton(options.optString("cancel", "Cancel")) { _, _ -> answer(false) }
                .setOnDismissListener { answer(false) }
            val message = options.optString("message")
            if (message.isNotEmpty()) builder.setMessage(message)
            val image = options.optString("image")
            if (image.isNotEmpty()) builder.setView(confirmImage(activity, image))
            builder.show()
        }
    }

    private fun confirmImage(activity: Activity, address: String): LinearLayout {
        val side = (160 * activity.resources.displayMetrics.density).toInt()
        val view = ImageView(activity).apply {
            scaleType = ImageView.ScaleType.FIT_CENTER
            layoutParams = LinearLayout.LayoutParams(side, side)
        }
        Thread {
            val bitmap = runCatching {
                java.net.URL(address).openStream().use { BitmapFactory.decodeStream(it) }
            }.getOrNull() ?: return@Thread
            mainHandler.post { view.setImageBitmap(bitmap) }
        }.start()
        return LinearLayout(activity).apply {
            gravity = android.view.Gravity.CENTER
            setPadding(0, side / 8, 0, 0)
            addView(view)
        }
    }

    @JvmStatic
    fun showToast(title: String, message: String) {
        mainHandler.post {
            val activity = activeActivity() ?: return@post
            val text = if (title.isEmpty()) message else "$title\n$message"
            Toast.makeText(activity, text, Toast.LENGTH_LONG).show()
        }
    }

    @JvmStatic
    fun openWebAuth(request: Long, url: String, callbackURL: String) {
        mainHandler.post {
            val activity = activeActivity()
            if (activity == null || activity.isFinishing || activity.isDestroyed) {
                Native.completeWebAuth(request, null, null, "unavailable")
                return@post
            }
            val webView = WebView(activity)
            webView.layoutParams = android.view.ViewGroup.LayoutParams(
                android.view.ViewGroup.LayoutParams.MATCH_PARENT,
                (activity.resources.displayMetrics.heightPixels * 0.68f).toInt(),
            )
            webView.settings.javaScriptEnabled = true
            webView.settings.domStorageEnabled = true
            android.webkit.CookieManager.getInstance().setAcceptCookie(true)
            Native.webAuthInitialCookies(request, android.webkit.CookieManager.getInstance().getCookie(callbackURL).orEmpty())
            val completed = AtomicBoolean(false)
            lateinit var dialog: AlertDialog
            val finish: (String?, String?, String?) -> Unit = { resultURL, cookies, error ->
                if (completed.compareAndSet(false, true)) {
                    pendingWebAuth.remove(request)
                    webView.stopLoading()
                    dialog.dismiss()
                    webView.destroy()
                    Native.completeWebAuth(request, resultURL, cookies, error)
                }
            }
            dialog = AlertDialog.Builder(activity)
                .setTitle("Authorize")
                .setView(webView)
                .setNegativeButton("Cancel") { _, _ -> finish(null, null, "cancelled") }
                .create()
            webView.webViewClient = object : WebViewClient() {
                override fun shouldOverrideUrlLoading(view: WebView, navigation: WebResourceRequest): Boolean {
                    val address = navigation.url.toString()
                    if (navigation.isForMainFrame && Native.webAuthIsCallback(request, address)) {
                        val cookies = Native.webAuthNewCookies(request, android.webkit.CookieManager.getInstance().getCookie(address).orEmpty())
                        finish(address, cookies, null)
                        return true
                    }
                    return false
                }
            }
            dialog.setOnCancelListener { finish(null, null, "cancelled") }
            dialog.setOnDismissListener { finish(null, null, "cancelled") }
            dialog.show()
            pendingWebAuth[request] = PendingWebAuth(WeakReference(activity), finish)
            runCatching { webView.loadUrl(url) }.onFailure { finish(null, null, "unavailable") }
        }
    }

    @JvmStatic
    fun cancelWebAuth(request: Long) {
        mainHandler.post { pendingWebAuth[request]?.finish?.invoke(null, null, "cancelled") }
    }

    @JvmStatic
    fun triggerHaptic(strength: String) {
        mainHandler.post {
            val activity = activeActivity() ?: return@post
            if (activity.isFinishing || activity.isDestroyed) return@post
            val feedback = when (strength) {
                "light" -> if (android.os.Build.VERSION.SDK_INT >= 27) HapticFeedbackConstants.KEYBOARD_TAP else HapticFeedbackConstants.VIRTUAL_KEY
                "medium" -> HapticFeedbackConstants.VIRTUAL_KEY
                "heavy" -> HapticFeedbackConstants.LONG_PRESS
                else -> return@post
            }
            activity.window.decorView.performHapticFeedback(feedback)
        }
    }

    @JvmStatic
    fun copyText(value: String) {
        mainHandler.post {
            val activity = activeActivity() ?: return@post
            val clipboard = activity.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return@post
            clipboard.setPrimaryClip(ClipData.newPlainText("astral", value))
            Toast.makeText(activity, "Copied", Toast.LENGTH_SHORT).show()
        }
    }

    @JvmStatic
    fun pickFile(request: Long, extensions: String) {
        mainHandler.post {
            if (pendingFilePick != null) {
                Native.completeFilePick(request, null, null, "picker_busy")
                return@post
            }
            val activity = activeActivity()
            if (activity == null || activity.isFinishing || activity.isDestroyed) {
                Native.completeFilePick(request, null, null, "picker_unavailable")
                return@post
            }
            val mimeTypes = extensions.split('\n').mapNotNull { MimeTypeMap.getSingleton().getMimeTypeFromExtension(it) }.distinct()
            val intent = Intent(Intent.ACTION_OPEN_DOCUMENT)
                .addCategory(Intent.CATEGORY_OPENABLE)
                .setType(if (mimeTypes.size == 1) mimeTypes.first() else "*/*")
            if (mimeTypes.isNotEmpty()) intent.putExtra(Intent.EXTRA_MIME_TYPES, mimeTypes.toTypedArray())
            pendingFilePick = request
            runCatching { activity.startActivityForResult(intent, PICK_FILE_REQUEST) }
                .onFailure {
                    pendingFilePick = null
                    Native.completeFilePick(request, null, null, "picker_unavailable")
                }
        }
    }

    @JvmStatic
    fun cancelFilePick(request: Long) {
        mainHandler.post {
            if (pendingFilePick != request) return@post
            pendingFilePick = null
            activeActivity()?.let { activity -> runCatching { activity.finishActivity(PICK_FILE_REQUEST) } }
        }
    }

    @JvmStatic
    fun editText(request: Long, title: String, value: String, maximumLength: Int) {
        mainHandler.post {
            val activity = activeActivity()
            if (activity == null || activity.isFinishing || activity.isDestroyed) {
                Native.completeTextEdit(request, null)
                return@post
            }
            val editor = EditText(activity)
            editor.setText(value)
            editor.setSelection(editor.text.length)
            editor.inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES
            editor.minLines = 5
            editor.maxLines = 12
            editor.filters = arrayOf(InputFilter { source, start, end, destination, replaceStart, replaceEnd ->
                val next = buildString {
                    append(destination, 0, replaceStart)
                    append(source, start, end)
                    append(destination, replaceEnd, destination.length)
                }
                if (next.toByteArray(Charsets.UTF_8).size <= maximumLength) null else ""
            })
            AlertDialog.Builder(activity)
                .setTitle(title)
                .setView(editor)
                .setNegativeButton("Cancel") { _, _ -> Native.completeTextEdit(request, null) }
                .setPositiveButton("Save") { _, _ -> Native.completeTextEdit(request, editor.text.toString()) }
                .setOnCancelListener { Native.completeTextEdit(request, null) }
                .show()
        }
    }

    @JvmStatic
    fun exportBackup() {
        mainHandler.post {
            val activity = activeActivity()
            if (activity == null) return@post
            val contents = runCatching { Native.exportBackup() }.getOrNull()
            if (contents.isNullOrEmpty()) return@post showBackupStatus(activity, "Could not create the backup")
            pendingExport = contents
            val intent = Intent(Intent.ACTION_CREATE_DOCUMENT)
                .addCategory(Intent.CATEGORY_OPENABLE)
                .setType("application/toml")
                .putExtra(Intent.EXTRA_TITLE, "astral-backup.toml")
            runCatching { activity.startActivityForResult(intent, EXPORT_BACKUP_REQUEST) }
                .onFailure {
                    pendingExport = null
                    showBackupStatus(activity, "Could not open the document picker")
                }
        }
    }

    @JvmStatic
    fun syncFolderName(): String? = SyncFolder.name()

    @JvmStatic
    fun syncChooseFolder() {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
            .addFlags(Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION or Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
        startDocumentPicker(intent, SYNC_FOLDER_REQUEST)
    }

    @JvmStatic
    fun syncForgetFolder() = SyncFolder.forget()

    @JvmStatic
    fun syncRead(): String? = SyncFolder.read()

    @JvmStatic
    fun syncWrite(contents: String): Boolean = SyncFolder.write(contents)

    @JvmStatic
    fun importBackup() {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT)
            .addCategory(Intent.CATEGORY_OPENABLE)
            .setType("*/*")
            .putExtra(Intent.EXTRA_MIME_TYPES, arrayOf("application/toml", "text/plain"))
        startDocumentPicker(intent, IMPORT_BACKUP_REQUEST)
    }

    @JvmStatic
    fun openReport(report: String) {
        mainHandler.post {
            val activity = activeActivity() ?: return@post
            val clipboard = activity.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return@post
            clipboard.setPrimaryClip(ClipData.newPlainText("astral report", report))
            Toast.makeText(activity, "Report copied", Toast.LENGTH_LONG).show()
        }
    }

    @JvmStatic
    fun refreshEmoji() {
        EmojiComposer.refresh()
        handler.post { if (guestHost != null) Native.pendingUpdates()?.let(::apply) }
    }

    @JvmStatic
    fun fetchPickedMedia(limit: Long) = PickedMedia.fetch(limit.toInt())

    @JvmStatic
    fun pasteText(): String? {
        val activity = activeActivity() ?: return null
        val clipboard = activity.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return null
        return clipboard.primaryClip?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.coerceToText(activity)?.toString()
    }

    @JvmStatic
    fun notify(delayMs: Long, title: String, payload: String) {
        val options = runCatching { org.json.JSONObject(payload) }.getOrNull() ?: return
        val body = options.optString("body")
        val link = options.optString("link")
        val post = Runnable {
            val context = activeActivity()?.applicationContext ?: return@Runnable
            val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as? NotificationManager ?: return@Runnable
            manager.createNotificationChannel(NotificationChannel(NOTIFICATION_CHANNEL, "astral", NotificationManager.IMPORTANCE_DEFAULT))
            val id = notificationIds.incrementAndGet()
            val builder = Notification.Builder(context, NOTIFICATION_CHANNEL)
                .setSmallIcon(context.applicationInfo.icon)
                .setContentTitle(title)
                .setContentText(body)
                .setStyle(Notification.BigTextStyle().bigText(body))
                .setAutoCancel(true)
            if (link.isNotEmpty()) {
                val open = Intent(Intent.ACTION_VIEW, Uri.parse(link)).setPackage(context.packageName)
                val flags = android.app.PendingIntent.FLAG_IMMUTABLE or android.app.PendingIntent.FLAG_UPDATE_CURRENT
                builder.setContentIntent(android.app.PendingIntent.getActivity(context, id, open, flags))
            }
            val notification = builder.build()
            runCatching { manager.notify(id, notification) }
                .onFailure { Log.warning("Failed to post notification $title: ${it.javaClass.simpleName}") }
        }
        if (delayMs > 0) mainHandler.postDelayed(post, delayMs) else mainHandler.post(post)
    }

    @JvmStatic
    fun speak(rateMillis: Long, text: String, language: String) {
        mainHandler.post {
            val context = activeActivity()?.applicationContext ?: return@post
            val pending: (TextToSpeech) -> Unit = { engine ->
                if (language.isNotEmpty()) engine.language = Locale.forLanguageTag(language)
                engine.setSpeechRate(0.5f + rateMillis / 1000f * 1.5f)
                engine.speak(text, TextToSpeech.QUEUE_ADD, null, "astral.${speechIds.incrementAndGet()}")
            }
            val ready = speech
            if (ready != null && speechReady) {
                pending(ready)
                return@post
            }
            speechQueue.add(pending)
            if (ready == null) {
                speech = TextToSpeech(context) { status ->
                    mainHandler.post {
                        speechReady = status == TextToSpeech.SUCCESS
                        val engine = speech
                        if (!speechReady || engine == null) {
                            Log.warning("Failed to start text to speech: status $status")
                            speechQueue.clear()
                            return@post
                        }
                        speechQueue.forEach { it(engine) }
                        speechQueue.clear()
                    }
                }
            }
        }
    }

    @JvmStatic
    fun playSound(volumeMillis: Long, path: String, unused: String) {
        mainHandler.post {
            player?.release()
            player = runCatching {
                MediaPlayer().apply {
                    setDataSource(path)
                    setVolume(volumeMillis / 1000f, volumeMillis / 1000f)
                    setOnCompletionListener { it.release(); if (player === it) player = null }
                    prepare()
                    start()
                }
            }.onFailure { Log.warning("Failed to play $path: ${it.javaClass.simpleName}: ${it.message}") }.getOrNull()
        }
    }

    @JvmStatic
    fun stopAudio() {
        mainHandler.post {
            speech?.stop()
            player?.release()
            player = null
        }
    }

    @JvmStatic
    fun openURL(url: String) {
        mainHandler.post {
            val activity = activeActivity() ?: return@post
            val intent = Intent(Intent.ACTION_VIEW, Uri.parse(url))
            if (isDiscordLink(url)) intent.setPackage(activity.packageName)
            runCatching { activity.startActivity(intent) }
                .onFailure { Log.warning("Failed to open $url: ${it.javaClass.simpleName}") }
        }
    }

    private fun isDiscordLink(url: String) = url.startsWith("https://discord.com/channels/")

    @JvmStatic
    fun insertComposerText(text: String, replace: Boolean): Boolean {
        mainHandler.post {
            runCatching {
                val activity = activeActivity() ?: return@runCatching
                val field = (activity.currentFocus as? EditText) ?: findComposer(activity.window.decorView) ?: return@runCatching
                field.requestFocus()
                if (replace) field.text.replace(0, field.text.length, text) else field.text.insert(field.selectionStart.coerceAtLeast(0), text)
            }.onFailure { Log.error("Failed to type into the chat input", it) }
        }
        return true
    }

    private fun findComposer(view: android.view.View): EditText? {
        if (view is EditText && view.isShown) return view
        val group = view as? android.view.ViewGroup ?: return null
        for (index in 0 until group.childCount) findComposer(group.getChildAt(index))?.let { return it }
        return null
    }

    @JvmStatic
    fun pressNode(identifier: String, prefix: Boolean): Boolean {
        val pressed = java.util.concurrent.CompletableFuture<Boolean>()
        mainHandler.post {
            pressed.complete(runCatching {
                val root = activeActivity()?.window?.decorView ?: return@runCatching false
                val view = identifiedView(root, identifier, prefix) ?: return@runCatching false
                view.performAccessibilityAction(android.view.accessibility.AccessibilityNodeInfo.ACTION_CLICK, null) || view.performClick()
            }.getOrDefault(false))
        }
        return runCatching { pressed.get(2, java.util.concurrent.TimeUnit.SECONDS) }.getOrDefault(false)
    }

    private fun identifiedView(view: android.view.View, identifier: String, prefix: Boolean): android.view.View? {
        if (!view.isShown) return null
        val own = runCatching { view.createAccessibilityNodeInfo().viewIdResourceName }.getOrNull()
        if (own != null && (if (prefix) own.startsWith(identifier) else own == identifier)) return view
        val group = view as? android.view.ViewGroup ?: return null
        for (index in group.childCount - 1 downTo 0) identifiedView(group.getChildAt(index), identifier, prefix)?.let { return it }
        return null
    }

    @JvmStatic
    fun dispatchGateway(message: String) {
        handler.post { GatewaySession.dispatchLocal(message) }
    }

    @JvmStatic
    fun restartGatewaySession() = NetworkHooks.restartGatewaySession()

    @JvmStatic
    fun discordRequest(id: Long, method: String, path: String, body: String) = PhoenixOkHttp.sendDiscordRequest(id, method, path, body)

    @JvmStatic
    fun deliver(ops: ByteArray) {
        handler.post { apply(ops) }
    }

    private fun apply(ops: ByteArray) {
        val host = guestHost ?: return
        val method = applyOps ?: return
        delivering.set(true)
        try {
            method.invoke(host, ops, ops.size)
            NavigationBack.refresh()
        } catch (error: Throwable) {
            Log.error("Failed to deliver astral's ViewWire update", error)
        } finally {
            delivering.set(false)
        }
    }

    private fun scheduleTween() {
        if (!tweening.compareAndSet(false, true)) return
        handler.postDelayed(::tweenFrame, TWEEN_FRAME_MS)
    }

    private fun tweenFrame() {
        try {
            val frame = Native.tweenFrame(SystemClock.uptimeMillis().toDouble())
            if (guestHost != null) frame?.let(::apply)
        } finally {
            tweening.set(false)
            if (runCatching { Native.tweensActive() }.getOrDefault(false)) scheduleTween()
        }
    }

    private fun retry() {
        try {
            if (guestHost != null) Native.pendingUpdates()?.let(::apply)
            scheduleTween()
        } finally {
            handler.postDelayed(::retry, RETRY_INTERVAL_MS)
        }
    }
}
