package app.astral.android

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Paint
import android.icu.text.BreakIterator
import android.os.Handler
import android.os.Looper
import android.text.Spannable
import android.text.Spanned
import android.text.style.ReplacementSpan
import android.widget.EditText
import android.widget.TextView
import android.util.LruCache
import android.graphics.RectF
import io.github.libxposed.api.XposedInterface.ExceptionMode
import io.github.libxposed.api.XposedModule
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.net.URI
import java.util.Collections
import java.util.Locale
import java.util.WeakHashMap
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit
import okhttp3.Call
import okhttp3.Callback
import okhttp3.CookieJar
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response

object EmojiComposer {
    private const val PHOENIX_PACKAGE = "com.discord.nativeapp."
    private const val MAXIMUM_EMOJI = 64
    private const val MAXIMUM_DOWNLOAD_BYTES = 1 shl 20
    private const val MAXIMUM_DIMENSION = 384
    private val main = Handler(Looper.getMainLooper())
    private val composers = Collections.newSetFromMap(WeakHashMap<EditText, Boolean>())
    private val downloading = Collections.newSetFromMap(ConcurrentHashMap<String, Boolean>())
    private val images = object : LruCache<String, Bitmap>(8 * 1024 * 1024) {
        override fun sizeOf(key: String, value: Bitmap): Int = value.byteCount
    }
    private val client = OkHttpClient.Builder()
        .callTimeout(15, TimeUnit.SECONDS)
        .connectTimeout(8, TimeUnit.SECONDS)
        .readTimeout(8, TimeUnit.SECONDS)
        .followRedirects(false)
        .followSslRedirects(false)
        .cookieJar(CookieJar.NO_COOKIES)
        .cache(null)
        .build()

    private fun isPhoenixInput(view: EditText): Boolean = view.javaClass.name.startsWith(PHOENIX_PACKAGE)

    fun install(module: XposedModule) {
        val changed = TextView::class.java.getDeclaredMethod(
            "onTextChanged",
            CharSequence::class.java,
            Int::class.javaPrimitiveType,
            Int::class.javaPrimitiveType,
            Int::class.javaPrimitiveType,
        )
        module.hook(changed).setExceptionMode(ExceptionMode.PROTECTIVE).intercept { chain ->
            val result = chain.proceed()
            val view = chain.thisObject as? EditText
            if (view != null && isPhoenixInput(view)) {
                synchronized(composers) { composers.add(view) }
                main.post {
                    apply(view)
                    MarkdownComposer.apply(view)
                }
            }
            result
        }
        Log.info("Hooked Phoenix's native message composer for emoji packs")
    }

    fun refresh() {
        val views = synchronized(composers) { composers.toList() }
        main.post {
            views.forEach { view ->
                apply(view)
                MarkdownComposer.apply(view)
            }
        }
    }

    private fun apply(view: EditText) {
        val content = view.text as? Spannable ?: return
        content.getSpans(0, content.length, EmojiSpan::class.java).forEach(content::removeSpan)
        if (content.isEmpty()) return
        val iterator = BreakIterator.getCharacterInstance(Locale.ROOT)
        iterator.setText(content.toString())
        var start = iterator.first()
        var count = 0
        while (start != BreakIterator.DONE && count < MAXIMUM_EMOJI) {
            val end = iterator.next()
            if (end == BreakIterator.DONE) break
            val sequence = content.subSequence(start, end).toString()
            if (sequence.codePointAt(0) >= 0x2000) {
                val url = Native.emojiURL(codepoints(sequence))
                if (!url.isNullOrEmpty()) {
                    val span = EmojiSpan(sequence, images.get(url))
                    content.setSpan(span, start, end, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                    if (span.image == null) download(url)
                    count++
                }
            }
            start = end
        }
    }

    private fun codepoints(value: String): String {
        val result = ArrayList<String>()
        var index = 0
        while (index < value.length) {
            val codepoint = value.codePointAt(index)
            result.add(codepoint.toString(16))
            index += Character.charCount(codepoint)
        }
        return result.joinToString("-")
    }

    private fun download(url: String) {
        val uri = runCatching { URI(url) }.getOrNull() ?: return
        if (uri.scheme != "https" || uri.host.isNullOrEmpty() || uri.userInfo != null || !downloading.add(url)) return
        val request = runCatching { Request.Builder().url(url).get().build() }.getOrNull() ?: run {
            downloading.remove(url)
            return
        }
        client.newCall(request).enqueue(object : Callback {
            override fun onFailure(call: Call, e: IOException) {
                downloading.remove(url)
            }

            override fun onResponse(call: Call, response: Response) {
                val bitmap = response.use { result ->
                    if (!result.isSuccessful || result.request.url.scheme != "https") return@use null
                    val body = result.body ?: return@use null
                    if (body.contentLength() > MAXIMUM_DOWNLOAD_BYTES) return@use null
                    val bytes = body.byteStream().use(::readBounded)
                    if (bytes.size > MAXIMUM_DOWNLOAD_BYTES) return@use null
                    decode(bytes)
                }
                downloading.remove(url)
                if (bitmap != null) {
                    images.put(url, bitmap)
                    refresh()
                }
            }
        })
    }

    private fun readBounded(input: java.io.InputStream): ByteArray {
        val output = ByteArrayOutputStream()
        val buffer = ByteArray(16 * 1024)
        while (output.size() <= MAXIMUM_DOWNLOAD_BYTES) {
            val count = input.read(buffer, 0, minOf(buffer.size, MAXIMUM_DOWNLOAD_BYTES + 1 - output.size()))
            if (count < 0) break
            output.write(buffer, 0, count)
        }
        return output.toByteArray()
    }

    private fun decode(bytes: ByteArray): Bitmap? {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
        if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null
        var sample = 1
        while (maxOf(bounds.outWidth, bounds.outHeight) / sample > MAXIMUM_DIMENSION) sample *= 2
        return BitmapFactory.decodeByteArray(bytes, 0, bytes.size, BitmapFactory.Options().apply { inSampleSize = sample })
    }

    private class EmojiSpan(private val fallback: String, val image: Bitmap?) : ReplacementSpan() {
        override fun getSize(paint: Paint, text: CharSequence, start: Int, end: Int, fontMetrics: Paint.FontMetricsInt?): Int {
            val metrics = paint.fontMetricsInt
            if (fontMetrics != null) {
                fontMetrics.ascent = metrics.ascent
                fontMetrics.descent = metrics.descent
                fontMetrics.top = metrics.top
                fontMetrics.bottom = metrics.bottom
            }
            return (metrics.descent - metrics.ascent).coerceAtLeast(paint.measureText(fallback).toInt())
        }

        override fun draw(canvas: Canvas, text: CharSequence, start: Int, end: Int, x: Float, top: Int, y: Int, bottom: Int, paint: Paint) {
            val bitmap = image
            if (bitmap == null || bitmap.isRecycled) {
                canvas.drawText(fallback, x, y.toFloat(), paint)
                return
            }
            val metrics = paint.fontMetrics
            val side = minOf((bottom - top).toFloat(), (metrics.descent - metrics.ascent).coerceAtLeast(1f))
            val targetTop = y + metrics.ascent + ((metrics.descent - metrics.ascent) - side) / 2f
            val target = RectF(x, targetTop, x + side, targetTop + side)
            canvas.drawBitmap(bitmap, null, target, paint)
        }
    }
}
