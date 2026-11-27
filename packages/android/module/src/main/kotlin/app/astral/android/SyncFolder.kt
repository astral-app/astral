package app.astral.android

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.DocumentsContract

object SyncFolder {
    private const val PREFERENCES = "astral.sync"
    private const val TREE = "tree"
    private const val FILE_NAME = "astral-sync.toml"
    private const val MIME = "application/toml"

    private fun context(): Context? = runCatching {
        Class.forName("android.app.ActivityThread").getMethod("currentApplication").invoke(null) as? Context
    }.getOrNull()

    private fun tree(context: Context): Uri? =
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).getString(TREE, null)?.let(Uri::parse)

    fun chosen(context: Context, tree: Uri) {
        context.contentResolver.takePersistableUriPermission(
            tree,
            Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION,
        )
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).edit().putString(TREE, tree.toString()).apply()
        Native.syncChosen()
    }

    fun name(): String? {
        val context = context() ?: return null
        val tree = tree(context) ?: return null
        val document = DocumentsContract.buildDocumentUriUsingTree(tree, DocumentsContract.getTreeDocumentId(tree))
        return runCatching {
            context.contentResolver.query(document, arrayOf(DocumentsContract.Document.COLUMN_DISPLAY_NAME), null, null, null)?.use {
                if (it.moveToFirst()) it.getString(0) else null
            }
        }.getOrNull() ?: tree.lastPathSegment
    }

    fun forget() {
        val context = context() ?: return
        val tree = tree(context) ?: return
        runCatching {
            context.contentResolver.releasePersistableUriPermission(
                tree,
                Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION,
            )
        }
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).edit().remove(TREE).apply()
    }

    private fun file(context: Context, tree: Uri): Uri? {
        val children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, DocumentsContract.getTreeDocumentId(tree))
        val columns = arrayOf(DocumentsContract.Document.COLUMN_DOCUMENT_ID, DocumentsContract.Document.COLUMN_DISPLAY_NAME)
        return context.contentResolver.query(children, columns, null, null, null)?.use { cursor ->
            while (cursor.moveToNext()) {
                if (cursor.getString(1) == FILE_NAME) return@use DocumentsContract.buildDocumentUriUsingTree(tree, cursor.getString(0))
            }
            null
        }
    }

    fun read(): String? {
        val context = context() ?: return null
        val tree = tree(context) ?: return null
        return runCatching {
            val uri = file(context, tree) ?: return null
            context.contentResolver.openInputStream(uri)?.use { it.readBytes().toString(Charsets.UTF_8) }
        }.onFailure { Log.error("Failed to read the sync file", it) }.getOrNull()
    }

    fun write(contents: String): Boolean {
        val context = context() ?: return false
        val tree = tree(context) ?: return false
        return runCatching {
            val parent = DocumentsContract.buildDocumentUriUsingTree(tree, DocumentsContract.getTreeDocumentId(tree))
            val uri = file(context, tree)
                ?: DocumentsContract.createDocument(context.contentResolver, parent, MIME, FILE_NAME)
                ?: return false
            context.contentResolver.openOutputStream(uri, "wt")?.use { it.write(contents.toByteArray(Charsets.UTF_8)) } != null
        }.onFailure { Log.error("Failed to write the sync file", it) }.getOrDefault(false)
    }
}
