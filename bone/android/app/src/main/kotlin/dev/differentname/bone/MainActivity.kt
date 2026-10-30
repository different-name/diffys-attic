package dev.differentname.bone

import android.Manifest
import android.app.Activity
import android.app.DownloadManager
import android.content.ActivityNotFoundException
import android.content.Intent
import android.content.pm.PackageManager
import android.content.res.Configuration
import android.net.MailTo
import android.net.Uri
import android.os.Bundle
import android.os.Environment
import android.os.Message
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.webkit.JavascriptInterface
import android.webkit.URLUtil
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import android.window.OnBackInvokedDispatcher
import org.json.JSONObject

class MainActivity : Activity() {
    private lateinit var web: WebView
    private val server = Uri.parse(BuildConfig.SERVER_URL)
    private var loaded = false

    override fun onCreate(state: Bundle?) {
        super.onCreate(state)
        WebView.setWebContentsDebuggingEnabled(BuildConfig.DEBUG)

        web = WebView(this).apply { setBackgroundColor(getColor(R.color.base)) }
        val root = FrameLayout(this).apply {
            setBackgroundColor(getColor(R.color.base))
            addView(web)
            setOnApplyWindowInsetsListener { view, insets ->
                val bars = insets.getInsets(
                    WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout() or WindowInsets.Type.ime(),
                )
                view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
                WindowInsets.CONSUMED
            }
        }
        window.setDecorFitsSystemWindows(false)
        setContentView(root)
        matchSystemBars()

        web.settings.apply {
            javaScriptEnabled = true
            domStorageEnabled = true
            allowFileAccess = false
            allowContentAccess = false
            setSupportMultipleWindows(true)
        }
        web.addJavascriptInterface(Bridge(), "BoneAndroid")
        web.webViewClient = object : WebViewClient() {
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest) = leave(request.url)

            override fun onPageFinished(view: WebView, url: String) {
                loaded = true
            }
        }
        web.webChromeClient = object : WebChromeClient() {
            // mail links open a new window, grab its url and hand it to the browser
            override fun onCreateWindow(view: WebView, dialog: Boolean, gesture: Boolean, result: Message): Boolean {
                val popup = WebView(this@MainActivity)
                popup.webViewClient = object : WebViewClient() {
                    override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                        leave(request.url)
                        popup.destroy()
                        return true
                    }
                }
                (result.obj as WebView.WebViewTransport).webView = popup
                result.sendToTarget()
                return true
            }
        }
        web.setDownloadListener { url, _, disposition, mime, _ ->
            val request = DownloadManager.Request(Uri.parse(url))
                .setMimeType(mime)
                .setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE_NOTIFY_COMPLETED)
                .setDestinationInExternalPublicDir(Environment.DIRECTORY_DOWNLOADS, URLUtil.guessFileName(url, disposition, mime))
            getSystemService(DownloadManager::class.java).enqueue(request)
        }

        onBackInvokedDispatcher.registerOnBackInvokedCallback(OnBackInvokedDispatcher.PRIORITY_DEFAULT) {
            web.evaluateJavascript("window.boneBack ? window.boneBack() : false") { handled ->
                if (handled == "true") return@evaluateJavascript
                if (web.canGoBack()) web.goBack() else moveTaskToBack(true)
            }
        }

        val prefs = getSharedPreferences("bone", MODE_PRIVATE)
        if (prefs.getInt(CACHE_VERSION, 0) != BuildConfig.VERSION_CODE) {
            web.clearCache(true)
            prefs.edit().putInt(CACHE_VERSION, BuildConfig.VERSION_CODE).apply()
        }

        val start = state?.getString(STATE_URL) ?: (BuildConfig.SERVER_URL + (pathFor(intent) ?: "/"))
        web.loadUrl(start)

        if (checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 0)
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        pathFor(intent)?.let(::open)
    }

    override fun onSaveInstanceState(state: Bundle) {
        super.onSaveInstanceState(state)
        web.url?.let { state.putString(STATE_URL, it) }
    }

    override fun onConfigurationChanged(config: Configuration) {
        super.onConfigurationChanged(config)
        matchSystemBars()
    }

    override fun onDestroy() {
        web.destroy()
        super.onDestroy()
    }

    private fun open(path: String) {
        if (!loaded) {
            web.loadUrl(BuildConfig.SERVER_URL + path)
            return
        }
        val target = JSONObject.quote(path)
        web.evaluateJavascript("history.pushState(null, '', $target); dispatchEvent(new PopStateEvent('popstate'))", null)
    }

    private fun leave(url: Uri): Boolean {
        if (url.scheme == server.scheme && url.authority == server.authority) return false
        if (url.scheme == "mailto") {
            open(composePath(MailTo.parse(url.toString())))
            return true
        }
        try {
            startActivity(Intent(Intent.ACTION_VIEW, url).addCategory(Intent.CATEGORY_BROWSABLE))
        } catch (_: ActivityNotFoundException) {
        }
        return true
    }

    private fun pathFor(intent: Intent?): String? = when (intent?.action) {
        Intent.ACTION_VIEW, Intent.ACTION_SENDTO ->
            intent.data?.takeIf { it.scheme == "mailto" }?.let { composePath(MailTo.parse(it.toString())) }
        Intent.ACTION_SEND -> compose(
            subject = intent.getStringExtra(Intent.EXTRA_SUBJECT),
            body = intent.getStringExtra(Intent.EXTRA_TEXT),
        )
        else -> intent?.getStringExtra(EXTRA_PATH)
    }

    private fun composePath(mail: MailTo) = compose(mail.to, mail.cc, mail.subject, mail.body)

    private fun compose(to: String? = null, cc: String? = null, subject: String? = null, body: String? = null): String {
        val query = Uri.Builder()
        listOf("to" to to, "cc" to cc, "subject" to subject, "body" to body).forEach { (key, value) ->
            if (!value.isNullOrEmpty()) query.appendQueryParameter(key, value)
        }
        return "/compose" + (query.build().encodedQuery?.let { "?$it" } ?: "")
    }

    private fun matchSystemBars() {
        val light = resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK != Configuration.UI_MODE_NIGHT_YES
        val flags = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS or WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS
        window.insetsController?.setSystemBarsAppearance(if (light) flags else 0, flags)
    }

    inner class Bridge {
        @JavascriptInterface
        fun setToken(token: String) {
            val changed = token != Api.token(this@MainActivity)
            Api.setToken(this@MainActivity, token)
            if (token.isNotEmpty() && (changed || !registered)) {
                registered = true
                runOnUiThread { Push.register(this@MainActivity) }
            }
        }
    }

    private var registered = false

    companion object {
        const val EXTRA_PATH = "path"
        private const val STATE_URL = "url"
        private const val CACHE_VERSION = "cache_version"
    }
}
