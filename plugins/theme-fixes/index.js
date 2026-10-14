(() => {
    const { patcher, metro, ui } = vendetta;
    const { React, ReactNative, FluxDispatcher } = metro.common;

    const tokens = metro.findByProps("SemanticColor");
    const resolver = tokens.default.meta ?? tokens.default.internal;
    const ThemeStore = metro.findByStoreName("ThemeStore");
    const runtime = metro.findByProps("jsx", "jsxs", "Fragment");
    const themeKind = metro.findByProps("isThemeDark", "isThemeLight");
    const { NativeModules, AppState, StatusBar } = ReactNative;
    const nativeTheme = NativeModules.NativeThemeModule ?? NativeModules.RTNThemeManager ?? NativeModules.DCDTheme;

    const builtInThemes = new Set(["dark", "darker", "midnight", "light"]);

    const statusTokens = {
        StatusOnline: "ICON_STATUS_ONLINE",
        StatusMobileOnline: "ICON_STATUS_ONLINE",
        StatusVROnline: "ICON_STATUS_ONLINE",
        StatusIdle: "ICON_STATUS_IDLE",
        StatusDND: "ICON_STATUS_DND",
        StatusOffline: "ICON_STATUS_OFFLINE",
        StatusStreaming: "ICON_BRAND",
    };

    const tokenByAsset = new Map();
    for (const [asset, token] of Object.entries(statusTokens)) {
        const id = ui.assets.getAssetIDByName(asset);
        if (id != null) tokenByAsset.set(id, ui.semanticColors[token]);
    }

    // the dots are bundled pngs
    function tintStatus(_args, rendered) {
        const image = rendered?.props?.children;
        const token = image?.props && tokenByAsset.get(image.props.source);
        if (!token) return rendered;
        const tintColor = resolver.resolveSemanticColor(ThemeStore.theme, token);
        return React.cloneElement(rendered, undefined,
            React.cloneElement(image, { style: [image.props.style, { tintColor }] }));
    }

    const nativeThemeFor = theme => (themeKind.isThemeLight(theme) ? "light" : "darker");

    // revenge only rewrites its current theme key, a stale one reaches native and flips status bar icons to dark
    function mapNativeTheme(args) {
        if (typeof args[0] === "string" && !builtInThemes.has(args[0])) args[0] = nativeThemeFor(ThemeStore.theme);
    }

    const isCustomTheme = () => !builtInThemes.has(ThemeStore.theme);
    const barStyleFor = () => (themeKind.isThemeLight(ThemeStore.theme) ? "dark-content" : "light-content");

    function syncNativeTheme() {
        if (!isCustomTheme()) return;
        nativeTheme?.updateTheme?.(nativeThemeFor(ThemeStore.theme));
        StatusBar.setBarStyle(barStyleFor());
    }

    // discord picks the default bar style from the theme name, which it doesn't know for custom themes
    function mapBarStyle(args) {
        if (isCustomTheme() && (args[0] === "dark-content" || args[0] === "default")) args[0] = barStyleFor();
    }

    // themes darken text on colored fills, but this label shares its token with text over media
    function recolorJoinVoice(args) {
        const props = args[1];
        if (props?.color === "text-overlay-light" && props.children === "Join Voice") {
            args[1] = { ...props, color: "control-connected-text-default" };
        }
    }

    let unpatches = [];
    const timers = new Set();

    return {
        onLoad() {
            for (const mod of new Set(metro.findByPropsAll("StatusWithTyping"))) {
                const key = typeof mod.default === "function" ? "default" : "Status";
                if (typeof mod[key] === "function") unpatches.push(patcher.after(key, mod, tintStatus));
            }

            if (nativeTheme?.updateTheme) unpatches.push(patcher.before("updateTheme", nativeTheme, mapNativeTheme));
            unpatches.push(patcher.before("setBarStyle", StatusBar, mapBarStyle));
            unpatches.push(patcher.before("jsx", runtime, recolorJoinVoice));
            unpatches.push(patcher.before("jsxs", runtime, recolorJoinVoice));

            // native picks its status bar style during startup without going through updateTheme, so resync once things settle
            const syncSoon = () => {
                const timer = setTimeout(() => {
                    timers.delete(timer);
                    syncNativeTheme();
                }, 500);
                timers.add(timer);
            };
            syncSoon();
            ThemeStore.addChangeListener(syncSoon);
            FluxDispatcher.subscribe("CONNECTION_OPEN", syncSoon);
            const appState = AppState.addEventListener("change", state => state === "active" && syncSoon());
            unpatches.push(
                () => ThemeStore.removeChangeListener(syncSoon),
                () => FluxDispatcher.unsubscribe("CONNECTION_OPEN", syncSoon),
                () => appState.remove(),
            );
        },
        onUnload() {
            for (const unpatch of unpatches) unpatch();
            unpatches = [];
            for (const timer of timers) clearTimeout(timer);
            timers.clear();
        },
    };
})()
