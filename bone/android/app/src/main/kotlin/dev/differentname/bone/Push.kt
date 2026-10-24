package dev.differentname.bone

import android.app.Activity
import org.json.JSONObject
import org.unifiedpush.android.connector.UnifiedPush
import kotlin.concurrent.thread

object Push {
    fun register(activity: Activity) {
        if (Api.token(activity).isEmpty()) return
        thread {
            val key = Api.call(activity, "GET", "/api/push/key")?.let { JSONObject(it).optString("key") } ?: return@thread
            activity.runOnUiThread {
                UnifiedPush.tryUseCurrentOrDefaultDistributor(activity) { found ->
                    if (found) UnifiedPush.register(activity, messageForDistributor = "bone", vapid = key)
                }
            }
        }
    }
}
