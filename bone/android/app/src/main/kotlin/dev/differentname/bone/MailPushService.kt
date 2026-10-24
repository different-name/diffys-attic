package dev.differentname.bone

import org.json.JSONObject
import org.unifiedpush.android.connector.FailedReason
import org.unifiedpush.android.connector.PushService
import org.unifiedpush.android.connector.data.PushEndpoint
import org.unifiedpush.android.connector.data.PushMessage
import kotlin.concurrent.thread

class MailPushService : PushService() {
    override fun onNewEndpoint(endpoint: PushEndpoint, instance: String) {
        val keys = endpoint.pubKeySet ?: return
        val body = JSONObject()
            .put("endpoint", endpoint.url)
            .put("p256dh", keys.pubKey)
            .put("auth", keys.auth)
            .toString()
        thread { Api.call(this, "POST", "/api/push", body) }
    }

    override fun onMessage(message: PushMessage, instance: String) {
        if (!message.decrypted) return
        val notice = runCatching { JSONObject(String(message.content)) }.getOrNull() ?: return
        if (notice.optString("type") == "new") Notifications.newMail(this, notice)
    }

    override fun onRegistrationFailed(reason: FailedReason, instance: String) {}

    // the server drops a subscription once its endpoint reports it gone
    override fun onUnregistered(instance: String) {}
}
