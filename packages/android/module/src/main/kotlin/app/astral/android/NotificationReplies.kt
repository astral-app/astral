package app.astral.android

import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.RemoteInput
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.graphics.drawable.Icon
import org.json.JSONObject
import java.util.concurrent.atomic.AtomicBoolean

object NotificationReplies {
    private const val ACTION = "app.astral.android.NOTIFICATION_REPLY"
    private const val TEXT_KEY = "astral_reply_text"
    private const val EXTRA_ID = "astral_notification"
    private const val EXTRA_PLUGIN = "astral_plugin"
    private const val EXTRA_CHANNEL = "astral_channel"
    private const val EXTRA_MESSAGE = "astral_message"
    private val registered = AtomicBoolean(false)

    private val receiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            val text = RemoteInput.getResultsFromIntent(intent)?.getCharSequence(TEXT_KEY)?.toString().orEmpty()
            val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as? NotificationManager
            manager?.cancel(intent.getIntExtra(EXTRA_ID, 0))
            if (text.isBlank()) return
            runCatching {
                Native.notificationReply(
                    intent.getStringExtra(EXTRA_PLUGIN).orEmpty(),
                    intent.getStringExtra(EXTRA_CHANNEL).orEmpty(),
                    intent.getStringExtra(EXTRA_MESSAGE).orEmpty(),
                    text,
                )
            }.onFailure { Log.warning("Failed to deliver notification reply ${intent.getIntExtra(EXTRA_ID, 0)}: ${it.javaClass.simpleName}") }
        }
    }

    fun attach(context: Context, builder: Notification.Builder, id: Int, reply: JSONObject) {
        val plugin = reply.optString("plugin")
        val channel = reply.optString("channel_id")
        if (plugin.isEmpty() || channel.isEmpty()) return
        register(context)
        val intent = Intent(ACTION)
            .setPackage(context.packageName)
            .putExtra(EXTRA_ID, id)
            .putExtra(EXTRA_PLUGIN, plugin)
            .putExtra(EXTRA_CHANNEL, channel)
            .putExtra(EXTRA_MESSAGE, reply.optString("message_id"))
        val flags = PendingIntent.FLAG_MUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        val pending = PendingIntent.getBroadcast(context, id, intent, flags)
        val input = RemoteInput.Builder(TEXT_KEY).setLabel(reply.optString("placeholder").ifEmpty { null }).build()
        val title = reply.optString("title").ifEmpty { reply.optString("send") }
        val action = Notification.Action.Builder(Icon.createWithResource(context, context.applicationInfo.icon), title, pending)
            .addRemoteInput(input)
            .setAllowGeneratedReplies(true)
            .build()
        builder.addAction(action)
    }

    private fun register(context: Context) {
        if (!registered.compareAndSet(false, true)) return
        runCatching {
            context.applicationContext.registerReceiver(receiver, IntentFilter(ACTION), Context.RECEIVER_NOT_EXPORTED)
        }.onFailure {
            registered.set(false)
            Log.warning("Failed to register notification replies: ${it.javaClass.simpleName}")
        }
    }
}
