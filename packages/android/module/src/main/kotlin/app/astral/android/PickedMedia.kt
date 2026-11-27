package app.astral.android

import android.content.ContentUris
import android.content.Context
import android.provider.MediaStore
import org.json.JSONArray
import org.json.JSONObject

object PickedMedia {
    private const val MILLISECONDS = 1000.0

    private fun application(): Context? = runCatching {
        Class.forName("android.app.ActivityThread").getMethod("currentApplication").invoke(null) as? Context
    }.getOrNull()

    fun fetch(limit: Int) {
        val application = application()
        Thread {
            val entries = runCatching { application?.let { query(it, limit) } ?: JSONArray() }
                .onFailure { Log.warning("Failed to read picker media $limit: ${it.javaClass.simpleName}") }
                .getOrDefault(JSONArray())
            Native.pickedMediaFetched(entries.toString())
        }.apply { name = "astral-picked-media" }.start()
    }

    private fun query(context: Context, limit: Int): JSONArray {
        val columns = arrayOf(
            MediaStore.Files.FileColumns.SIZE,
            MediaStore.Files.FileColumns.MEDIA_TYPE,
            MediaStore.Video.VideoColumns.DURATION,
            MediaStore.Files.FileColumns._ID,
        )
        val selection = "${MediaStore.Files.FileColumns.MEDIA_TYPE} IN (?, ?)"
        val arguments = arrayOf(
            MediaStore.Files.FileColumns.MEDIA_TYPE_IMAGE.toString(),
            MediaStore.Files.FileColumns.MEDIA_TYPE_VIDEO.toString(),
        )
        val order = "${MediaStore.Files.FileColumns.DATE_ADDED} DESC"
        val result = JSONArray()
        context.contentResolver.query(MediaStore.Files.getContentUri(MediaStore.VOLUME_EXTERNAL), columns, selection, arguments, order)?.use { cursor ->
            while (result.length() < limit && cursor.moveToNext()) {
                val video = cursor.getInt(1) == MediaStore.Files.FileColumns.MEDIA_TYPE_VIDEO
                val collection = if (video) MediaStore.Video.Media.EXTERNAL_CONTENT_URI else MediaStore.Images.Media.EXTERNAL_CONTENT_URI
                val uri = ContentUris.withAppendedId(collection, cursor.getLong(3)).toString()
                result.put(
                    JSONObject()
                        .put("size", if (cursor.isNull(0)) -1L else cursor.getLong(0))
                        .put("duration", if (video && !cursor.isNull(2)) cursor.getLong(2) / MILLISECONDS else 0.0)
                        .put("type", if (video) "video" else "image")
                        .put("source", uri)
                        .put("url", uri),
                )
            }
        }
        return result
    }
}
