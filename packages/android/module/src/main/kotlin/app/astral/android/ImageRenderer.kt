package app.astral.android

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.Typeface
import android.text.TextPaint
import android.text.TextUtils
import org.json.JSONObject
import java.io.ByteArrayOutputStream

object ImageRenderer {
    fun render(json: String): ByteArray? {
        val spec = JSONObject(json)
        val scale = spec.optDouble("scale", 2.0).toFloat()
        val width = (spec.getDouble("width") * scale).toInt()
        val height = (spec.getDouble("height") * scale).toInt()
        val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        canvas.scale(scale, scale)
        if (spec.has("background")) canvas.drawColor(color(spec, "background"))
        val items = spec.optJSONArray("items") ?: return encode(bitmap)
        for (index in 0 until items.length()) {
            val item = items.optJSONObject(index) ?: continue
            when (item.optString("type")) {
                "rect" -> drawRect(canvas, item)
                "circle" -> drawCircle(canvas, item)
                "text" -> drawText(canvas, item)
            }
        }
        return encode(bitmap)
    }

    private fun color(item: JSONObject, key: String): Int = item.optLong(key, 0).toInt()

    private fun drawRect(canvas: Canvas, item: JSONObject) {
        val frame = RectF(item.optDouble("x").toFloat(), item.optDouble("y").toFloat(),
            (item.optDouble("x") + item.optDouble("w")).toFloat(), (item.optDouble("y") + item.optDouble("h")).toFloat())
        val radius = item.optDouble("radius", 0.0).toFloat()
        canvas.drawRoundRect(frame, radius, radius, Paint(Paint.ANTI_ALIAS_FLAG).apply { color = color(item, "color") })
        if (item.has("border")) {
            val border = Paint(Paint.ANTI_ALIAS_FLAG).apply {
                style = Paint.Style.STROKE
                color = color(item, "border")
                strokeWidth = item.optDouble("border_width", 1.0).toFloat()
            }
            canvas.drawRoundRect(frame, radius, radius, border)
        }
    }

    private fun drawCircle(canvas: Canvas, item: JSONObject) {
        val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = color(item, "color") }
        canvas.drawCircle(item.optDouble("x").toFloat(), item.optDouble("y").toFloat(), item.optDouble("r", 4.0).toFloat(), paint)
    }

    private fun drawText(canvas: Canvas, item: JSONObject) {
        val weight = item.optString("weight")
        val family = if (item.optBoolean("mono")) Typeface.MONOSPACE else Typeface.DEFAULT
        val style = if (weight == "bold" || weight == "semibold") Typeface.BOLD else Typeface.NORMAL
        val paint = TextPaint(Paint.ANTI_ALIAS_FLAG).apply {
            color = if (item.has("color")) color(item, "color") else 0xFFFFFFFF.toInt()
            textSize = item.optDouble("size", 16.0).toFloat()
            typeface = Typeface.create(family, style)
        }
        val width = item.optDouble("width", 4096.0).toFloat()
        val text = TextUtils.ellipsize(item.optString("text"), paint, width, TextUtils.TruncateAt.END).toString()
        val measured = paint.measureText(text)
        val x = item.optDouble("x").toFloat() + when (item.optString("align")) {
            "right" -> width - measured
            "center" -> (width - measured) / 2
            else -> 0f
        }
        canvas.drawText(text, x, item.optDouble("y").toFloat() - paint.fontMetrics.ascent, paint)
    }

    private fun encode(bitmap: Bitmap): ByteArray {
        val output = ByteArrayOutputStream()
        bitmap.compress(Bitmap.CompressFormat.PNG, 100, output)
        bitmap.recycle()
        return output.toByteArray()
    }
}
