package app.astral.android

import android.content.Context
import android.os.SystemClock
import android.provider.MediaStore

object PickedMedia {
    private const val FETCH_BATCH = 64
    private const val FRESH_MS = 20_000L

    private class Entry(val size: Long, val durationMs: Long, val type: String)

    private val lock = Any()
    private var entries: List<Entry> = emptyList()
    private var fetchedAt = 0L
    private var fetching = false

    fun describe(context: Context?, slot: Int, refresh: () -> Unit): String? {
        val (entry, fetch, limit) = synchronized(lock) {
            val count = entries.size
            val stale = SystemClock.uptimeMillis() - fetchedAt > FRESH_MS || fetchedAt == 0L ||
                (slot >= count && count % FETCH_BATCH == 0)
            val start = stale && !fetching && context != null
            if (start) fetching = true
            Triple(entries.getOrNull(slot), start, maxOf(count, (slot / FETCH_BATCH + 1) * FETCH_BATCH))
        }
        if (fetch && context != null) startFetch(context.applicationContext, limit, refresh)
        return entry?.let { "${it.size}\n${it.durationMs / 1000.0}\n${it.type}" }
    }

    private fun startFetch(context: Context, limit: Int, refresh: () -> Unit) {
        Thread {
            val fetched = runCatching { query(context, limit) }
                .onFailure { Log.warning("Failed to read picker media $limit: ${it.javaClass.simpleName}") }
                .getOrDefault(emptyList())
            val changed = synchronized(lock) {
                val differs = fetched.map { listOf(it.size, it.durationMs, it.type) } != entries.map { listOf(it.size, it.durationMs, it.type) }
                entries = fetched
                fetchedAt = SystemClock.uptimeMillis()
                fetching = false
                differs
            }
            if (changed) refresh()
        }.apply { name = "astral-picked-media" }.start()
    }

    private fun query(context: Context, limit: Int): List<Entry> {
        val columns = arrayOf(
            MediaStore.Files.FileColumns.SIZE,
            MediaStore.Files.FileColumns.MEDIA_TYPE,
            MediaStore.Video.VideoColumns.DURATION,
        )
        val selection = "${MediaStore.Files.FileColumns.MEDIA_TYPE} IN (?, ?)"
        val arguments = arrayOf(
            MediaStore.Files.FileColumns.MEDIA_TYPE_IMAGE.toString(),
            MediaStore.Files.FileColumns.MEDIA_TYPE_VIDEO.toString(),
        )
        val order = "${MediaStore.Files.FileColumns.DATE_ADDED} DESC"
        val uri = MediaStore.Files.getContentUri(MediaStore.VOLUME_EXTERNAL)
        val result = ArrayList<Entry>(limit)
        context.contentResolver.query(uri, columns, selection, arguments, order)?.use { cursor ->
            while (result.size < limit && cursor.moveToNext()) {
                val video = cursor.getInt(1) == MediaStore.Files.FileColumns.MEDIA_TYPE_VIDEO
                val size = if (cursor.isNull(0)) -1L else cursor.getLong(0)
                val duration = if (video && !cursor.isNull(2)) cursor.getLong(2) else 0L
                result.add(Entry(size, duration, if (video) "video" else "image"))
            }
        }
        return result
    }
}
