(() => {
    const { patcher, metro } = vendetta;

    const blockedRequest = /\/api(\/v\d+)?\/(science|track|metrics)\b|\.ingest\.sentry\.io|client-analytics\.braintreegateway\.com|app\.adjust\./;
    const noop = () => {};

    let unpatches = [];

    function silence(target, method) {
        if (typeof target?.[method] !== "function") return false;
        unpatches.push(patcher.instead(method, target, noop));
        return true;
    }

    function silenceAll(targets, methods) {
        let hits = 0;
        for (const target of new Set(targets.filter(Boolean))) {
            for (const method of methods) if (silence(target, method)) hits++;
        }
        return hits;
    }

    function blockRequests() {
        const proto = XMLHttpRequest.prototype;
        unpatches.push(patcher.before("open", proto, function (args) {
            this.__noTrackUrl = String(args[1] ?? "");
        }));
        // react native keeps the url on _url, open is a fallback for anything that bypasses it
        unpatches.push(patcher.instead("send", proto, function (args, orig) {
            const url = this._url ?? this.__noTrackUrl ?? "";
            if (blockedRequest.test(url)) return undefined;
            return orig(...args);
        }));
    }

    return {
        onLoad() {
            silenceAll([metro.findByProps("AnalyticsActionHandlers")?.AnalyticsActionHandlers],
                ["handleTrack", "handleFingerprint"]);
            silenceAll(metro.findByPropsAll("track", "expandEventProperties"), ["track"]);
            silenceAll(metro.findByPropsAll("trackWithMetadata"), ["trackWithMetadata"]);
            // the metrics buffer is replaced on every flush, so the methods are patched instead of the array
            silenceAll(metro.findByPropsAll("_metrics"), ["increment", "distribution", "_flush"]);
            blockRequests();
        },
        onUnload() {
            for (const unpatch of unpatches) unpatch();
            unpatches = [];
        },
    };
})()
