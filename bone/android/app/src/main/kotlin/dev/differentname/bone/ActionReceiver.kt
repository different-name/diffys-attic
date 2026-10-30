package dev.differentname.bone

import android.app.NotificationManager
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import org.json.JSONArray
import org.json.JSONObject
import kotlin.concurrent.thread

class ActionReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val op = intent.action?.takeIf { it == ARCHIVE || it == READ } ?: return
        val id = intent.getLongExtra(EXTRA_ID, -1).takeIf { it >= 0 } ?: return
        context.getSystemService(NotificationManager::class.java).cancel(intent.getIntExtra(EXTRA_NOTIFICATION, 0))
        val body = JSONObject().put("ids", JSONArray().put(id)).put("op", op).toString()
        val pending = goAsync()
        thread {
            try {
                Api.call(context, "POST", "/api/actions", body)
            } finally {
                pending.finish()
            }
        }
    }

    companion object {
        const val ARCHIVE = "archive"
        const val READ = "read"
        const val EXTRA_ID = "id"
        const val EXTRA_NOTIFICATION = "notification"
    }
}
