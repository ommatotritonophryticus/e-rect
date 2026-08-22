package org.vpngen.erect;

import android.annotation.SuppressLint;
import android.app.Activity;
import android.os.Build;
import android.os.Bundle;
import android.view.View;
import android.view.WindowManager;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;

import java.io.IOException;
import java.io.InputStream;
import java.util.HashMap;
import java.util.Map;

/**
 * The browser build in a window of its own.
 *
 * <p>The whole of this class exists to solve one problem: a page loaded from
 * {@code file:///android_asset/} has an opaque origin. Opaque origins have no
 * localStorage to save settings into, and {@code fetch} from one is refused as
 * cross-origin - which would take the soundtrack with it. Serving the same
 * files from an intercepted {@code https://} host gives the page a real,
 * stable, secure origin, and both work as they do in a browser.
 *
 * <p>The host is never resolved. Every request for it is answered out of the
 * APK's assets, so the app needs no network permission and works with the radio
 * off.
 */
public class MainActivity extends Activity {

    /**
     * The host the game appears to be served from.
     *
     * <p>Android reserves this name for exactly this purpose, so it can never
     * collide with a real site - which matters, because localStorage is keyed
     * by origin and a collision would mean sharing saves with a stranger.
     */
    private static final String HOST = "appassets.androidplatform.net";

    private static final String ORIGIN = "https://" + HOST + "/";

    /**
     * Content types the page depends on being right.
     *
     * <p>Not decoration: the wasm is instantiated with
     * {@code WebAssembly.compileStreaming}, which refuses anything that is not
     * {@code application/wasm}, and the audio is decoded by a browser that
     * decides what a file is from this and not from its name.
     */
    private static final Map<String, String> TYPES = new HashMap<>();

    static {
        TYPES.put("html", "text/html");
        TYPES.put("js", "text/javascript");
        TYPES.put("wasm", "application/wasm");
        TYPES.put("json", "application/json");
        TYPES.put("flac", "audio/flac");
        TYPES.put("png", "image/png");
        TYPES.put("css", "text/css");
    }

    private WebView web;

    @SuppressLint("SetJavaScriptEnabled")
    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);

        web = new WebView(this);
        WebSettings settings = web.getSettings();
        settings.setJavaScriptEnabled(true);
        // Where the settings and the high score live.
        settings.setDomStorageEnabled(true);
        // The game asks for a tap before it loads the soundtrack anyway - a
        // browser will not decode audio before one - so this only spares the
        // player a second tap, not a first.
        settings.setMediaPlaybackRequiresUserGesture(false);
        settings.setCacheMode(WebSettings.LOAD_NO_CACHE);

        web.setWebViewClient(new AssetClient());
        // Nothing here scrolls or bounces; the game fills the window.
        web.setVerticalScrollBarEnabled(false);
        web.setHorizontalScrollBarEnabled(false);
        web.setOverScrollMode(View.OVER_SCROLL_NEVER);
        web.setBackgroundColor(0xFF000000);

        setContentView(web);
        web.loadUrl(ORIGIN + "index.html");
    }

    @Override
    public void onWindowFocusChanged(boolean focused) {
        super.onWindowFocusChanged(focused);
        if (focused) {
            goFullscreen();
        }
    }

    /** Bars away, and back away again after a swipe brings them in. */
    private void goFullscreen() {
        View decor = getWindow().getDecorView();
        decor.setSystemUiVisibility(
                View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                        | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                        | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                        | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                        | View.SYSTEM_UI_FLAG_FULLSCREEN
                        | View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY);
    }

    /** Answers every request for {@link #HOST} out of the packaged assets. */
    private final class AssetClient extends WebViewClient {

        @Override
        public WebResourceResponse shouldInterceptRequest(WebView view, WebResourceRequest request) {
            if (!HOST.equals(request.getUrl().getHost())) {
                // Nothing else should ever be asked for. Refusing rather than
                // letting it through keeps the "no network" promise true even
                // if a future page grows a stray reference.
                return new WebResourceResponse("text/plain", "utf-8", null);
            }
            String path = request.getUrl().getPath();
            if (path == null || path.equals("/")) {
                path = "/index.html";
            }
            String name = path.substring(1);
            try {
                InputStream stream = getAssets().open(name);
                WebResourceResponse response =
                        new WebResourceResponse(typeOf(name), null, stream);
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP) {
                    response.setStatusCodeAndReasonPhrase(200, "OK");
                }
                return response;
            } catch (IOException missing) {
                return new WebResourceResponse("text/plain", "utf-8", null);
            }
        }
    }

    private static String typeOf(String name) {
        int dot = name.lastIndexOf('.');
        if (dot < 0) {
            return "application/octet-stream";
        }
        String type = TYPES.get(name.substring(dot + 1).toLowerCase());
        return type != null ? type : "application/octet-stream";
    }

    @Override
    protected void onDestroy() {
        if (web != null) {
            web.destroy();
            web = null;
        }
        super.onDestroy();
    }
}
