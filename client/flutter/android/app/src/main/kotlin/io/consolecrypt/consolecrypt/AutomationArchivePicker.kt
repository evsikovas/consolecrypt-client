package io.consolecrypt.consolecrypt

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import io.flutter.plugin.common.MethodChannel
import java.io.File
import java.io.IOException
import java.util.UUID

/** Android providers can assign a generic MIME type and an arbitrary display name to a project archive. */
internal class AutomationArchivePicker(private val activity: Activity) {
    private var pending: MethodChannel.Result? = null

    fun open(result: MethodChannel.Result) {
        if (pending != null) {
            result.error("archive_busy", "Another archive is being opened", null)
            return
        }
        pending = result
        try {
            @Suppress("DEPRECATION")
            activity.startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                addCategory(Intent.CATEGORY_OPENABLE)
                type = "*/*"
            }, REQUEST)
        } catch (_: Exception) {
            pending = null
            result.error("archive_read", "Document picker unavailable", null)
        }
    }

    fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?): Boolean {
        if (requestCode != REQUEST) return false
        val result = pending ?: return true
        val uri = data?.data
        if (resultCode != Activity.RESULT_OK || uri == null) {
            pending = null
            result.success(null)
            return true
        }
        Thread {
            val archive = runCatching { copy(uri) }
            activity.runOnUiThread {
                if (pending !== result) {
                    archive.getOrNull()?.file?.let { discard(it) }
                    return@runOnUiThread
                }
                pending = null
                val picked = archive.getOrNull()
                if (picked != null) {
                    result.success(mapOf("path" to picked.file.path, "name" to picked.name))
                } else {
                    result.error(
                        (archive.exceptionOrNull() as? ArchiveError)?.code ?: "archive_read",
                        "Could not open the project archive", null,
                    )
                }
            }
        }.start()
        return true
    }

    fun dispose() {
        pending?.success(null)
        pending = null
    }

    private fun copy(uri: Uri): Picked {
        if (uri.scheme != "content") throw ArchiveError("archive_read")
        val resolver = activity.contentResolver
        val name = runCatching {
            resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                if (it.moveToFirst()) it.getString(0) else null
            }
        }.getOrNull()?.take(240)
        val directory = File(activity.cacheDir.canonicalFile, "ccproject-" + UUID.randomUUID())
        if (!directory.mkdir()) throw ArchiveError("archive_read")
        val file = File(directory, "archive.ccproject")
        try {
            val input = resolver.openInputStream(uri)?.buffered() ?: throw ArchiveError("archive_read")
            input.use {
                val prefix = ByteArray(MAGIC.size)
                var offset = 0
                while (offset < prefix.size) {
                    val length = input.read(prefix, offset, prefix.size - offset)
                    if (length <= 0) throw ArchiveError("archive_format")
                    offset += length
                }
                if (!prefix.contentEquals(MAGIC)) throw ArchiveError("archive_format")
                file.outputStream().buffered().use { output ->
                    output.write(prefix)
                    var total = prefix.size
                    val buffer = ByteArray(64 * 1024)
                    while (true) {
                        val count = input.read(buffer)
                        if (count == -1) break
                        total += count
                        if (total > MAX_BYTES) throw ArchiveError("archive_size")
                        output.write(buffer, 0, count)
                    }
                }
            }
            return Picked(file, name)
        } catch (error: Exception) {
            discard(file)
            throw error
        }
    }

    private fun discard(file: File) {
        file.delete()
        file.parentFile?.delete()
    }

    private data class Picked(val file: File, val name: String?)
    private class ArchiveError(val code: String) : IOException()

    private companion object {
        const val REQUEST = 7402
        // crypto-core portable_project.rs, v1 envelope including header and tag.
        const val MAX_BYTES = 17_301_600
        val MAGIC = "consolecrypt/project-package/v1\u0000".toByteArray(Charsets.US_ASCII)
    }
}
