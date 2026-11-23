package app.astral.android

import android.app.Activity
import android.app.AlertDialog
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.view.HapticFeedbackConstants
import android.text.InputFilter
import android.text.InputType
import android.widget.EditText
import android.widget.Toast
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import io.github.libxposed.api.XposedInterface.ExceptionMode
import io.github.libxposed.api.XposedModule
import java.lang.reflect.Method
import java.lang.ref.WeakReference

object PhoenixBridge {
    private const val GUEST_HOST = "com.discord.nativeapp.bridge.GuestHost"
    private const val RETRY_INTERVAL_MS = 1000L

    private const val EVENT_FRAME = 1
    private const val NODE_OFFSET = 2
    private const val EVENT_OFFSET = 6
    private const val FUNCTION0 = "kotlin.jvm.functions.Function0"
    private const val MAIN_ACTIVITY = "com.discord.nativeapp.MainActivity"
    private const val EXPORT_BACKUP_REQUEST = 0xA571
    private const val IMPORT_BACKUP_REQUEST = 0xA572
    private const val PICK_FILE_REQUEST = 0xA573
    private const val MAXIMUM_BACKUP_BYTES = 1 shl 20

    @Volatile var pressEvent = -1
    @Volatile var changeEvent = -1
    @Volatile private var guestHost: Any? = null
    private var applyOps: Method? = null
    private val delivering = ThreadLocal.withInitial { false }
    private val worker = HandlerThread("astral-wire").apply { start() }
    private val handler = Handler(worker.looper)
    private val mainHandler = Handler(Looper.getMainLooper())
    @Volatile private var currentActivity: WeakReference<Activity>? = null
    @Volatile private var pendingExport: String? = null
    @Volatile private var pendingFilePick: Long? = null

    fun install(module: XposedModule, loader: ClassLoader) {
        val type = loader.loadClass(GUEST_HOST)
        val method = type.getDeclaredMethod("applyOps", ByteArray::class.java, Int::class.javaPrimitiveType)
        method.isAccessible = true
        applyOps = method
        module.hook(method)
            .setExceptionMode(ExceptionMode.PROTECTIVE)
            .intercept { chain ->
                guestHost = chain.thisObject
                if (delivering.get() == true) return@intercept chain.proceed()
                val ops = chain.getArg(0) as ByteArray
                val length = chain.getArg(1) as Int
                Capture.record(ops, length)
                val rewritten = Native.rewrite(ops, length)
                NavigationBack.refresh()
                if (rewritten != null) Capture.record(rewritten, rewritten.size)
                if (rewritten == null) chain.proceed() else chain.proceed(arrayOf<Any>(rewritten, rewritten.size))
            }
        hookEvents(module, type, loader)
        installBackupActions(module, loader)
        handler.postDelayed(::retry, RETRY_INTERVAL_MS)
        Log.info("Hooked GuestHost.applyOps")
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
                if (event != null && handled(event)) {
                    chain.getArg(1)?.let { done -> handler.post { runCatching { done.javaClass.getMethod("invoke").invoke(done) } } }
                    null
                } else {
                    chain.proceed()
                }
            }
        Log.info("Hooked GuestHost.${enqueue.name} for events")
    }

    private fun handled(event: ByteArray): Boolean {
        if (event.size < EVENT_OFFSET + 2 || event[0].toInt() != EVENT_FRAME || event[1].toInt() != EVENT_FRAME) return false
        val buffer = java.nio.ByteBuffer.wrap(event).order(java.nio.ByteOrder.LITTLE_ENDIAN)
        val kind = buffer.getShort(EVENT_OFFSET).toInt() and 0xFFFF
        return when (kind) {
            pressEvent -> Native.press(buffer.getInt(NODE_OFFSET))
            changeEvent -> Native.slide(buffer.getInt(NODE_OFFSET), event.copyOfRange(EVENT_OFFSET + 2, event.size))
            else -> false
        }
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
        val resultMethod = activityMethod(
            mainActivity,
            "onActivityResult",
            arrayOf(Int::class.javaPrimitiveType!!, Int::class.javaPrimitiveType!!, Intent::class.java),
        ) ?: return Log.warning("Phoenix activity has no document result callback; backups stay unavailable")
        module.hook(resultMethod)
            .setExceptionMode(ExceptionMode.PROTECTIVE)
            .intercept { chain ->
                val request = chain.getArg(0) as? Int ?: return@intercept chain.proceed()
                if (request != EXPORT_BACKUP_REQUEST && request != IMPORT_BACKUP_REQUEST && request != PICK_FILE_REQUEST) {
                    return@intercept chain.proceed()
                }
                val resultValue = chain.proceed()
                val activity = chain.thisObject as? Activity ?: return@intercept resultValue
                val result = chain.getArg(1) as? Int ?: Activity.RESULT_CANCELED
                val data = chain.getArg(2) as? Intent
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
            Toast.makeText(activity, "Plugin report copied", Toast.LENGTH_LONG).show()
        }
    }

    @JvmStatic
    fun refreshEmoji() = EmojiComposer.refresh()

    @JvmStatic
    fun fetch(url: String, token: String): ByteArray = Http.get(url, token)

    @JvmStatic
    fun request(id: Long, options: String) = Http.request(id, options)

    @JvmStatic
    fun cancelRequest(id: Long) = Http.cancel(id)

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

    private fun retry() {
        try {
            if (guestHost != null) Native.pendingUpdates()?.let(::apply)
        } finally {
            handler.postDelayed(::retry, RETRY_INTERVAL_MS)
        }
    }
}
