package dev.differentname.bone

import android.content.Context
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL

object Api {
    private const val PREFS = "bone"

    fun token(context: Context): String =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString("token", null).orEmpty()

    fun setToken(context: Context, token: String) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putString("token", token).apply()
    }

    fun call(context: Context, method: String, path: String, body: String? = null): String? {
        val token = token(context)
        if (token.isEmpty()) return null
        val connection = URL(BuildConfig.SERVER_URL + path).openConnection() as HttpURLConnection
        return try {
            connection.requestMethod = method
            connection.connectTimeout = 10_000
            connection.readTimeout = 15_000
            connection.setRequestProperty("Authorization", "Bearer $token")
            if (body != null) {
                connection.doOutput = true
                connection.setRequestProperty("Content-Type", "application/json")
                connection.outputStream.use { it.write(body.toByteArray()) }
            }
            if (connection.responseCode in 200..299) connection.inputStream.bufferedReader().use { it.readText() } else null
        } catch (_: IOException) {
            null
        } finally {
            connection.disconnect()
        }
    }
}
