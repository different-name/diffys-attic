package dev.differentname.bone

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import org.json.JSONObject

object Notifications {
    private const val CHANNEL = "mail"

    fun newMail(context: Context, notice: JSONObject) {
        val manager = context.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, context.getString(R.string.channel_mail), NotificationManager.IMPORTANCE_DEFAULT),
        )

        val id = notice.getLong("id")
        val thread = notice.optLong("thread", 0)
        val tag = id.toInt()
        val subject = notice.optString("subject")
        val flags = PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT

        val open = Intent(context, MainActivity::class.java)
            .putExtra(MainActivity.EXTRA_PATH, if (thread > 0) "/inbox/$thread" else "/")
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

        fun action(op: String, label: Int, code: Int): Notification.Action {
            val intent = Intent(context, ActionReceiver::class.java)
                .setAction(op)
                .putExtra(ActionReceiver.EXTRA_ID, id)
                .putExtra(ActionReceiver.EXTRA_NOTIFICATION, tag)
            val pending = PendingIntent.getBroadcast(context, tag * 3 + code, intent, flags)
            return Notification.Action.Builder(null, context.getString(label), pending).build()
        }

        val notification = Notification.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setColor(context.getColor(R.color.accent))
            .setCategory(Notification.CATEGORY_EMAIL)
            .setContentTitle(notice.optString("from"))
            .setContentText(subject)
            .setStyle(Notification.BigTextStyle().bigText(subject))
            .apply { notice.optString("recipient").takeIf { it.isNotEmpty() }?.let { setSubText(it) } }
            .setContentIntent(PendingIntent.getActivity(context, tag * 3, open, flags))
            .setAutoCancel(true)
            .addAction(action(ActionReceiver.ARCHIVE, R.string.archive, 1))
            .addAction(action(ActionReceiver.READ, R.string.mark_read, 2))
            .build()
        manager.notify(tag, notification)
    }
}
