(() => {
    const { patcher, metro, plugin, ui } = vendetta;
    const { React, ReactNative } = metro.common;
    const { NativeModules, View, Text, Pressable } = ReactNative;

    const runtime = metro.findByProps("jsx", "jsxs", "Fragment");
    const files = NativeModules.NativeFileModule ?? NativeModules.RTNFileManager ?? NativeModules.DCDFileManager;
    const tokens = metro.findByProps("SemanticColor");
    const resolver = tokens.default.meta ?? tokens.default.internal;
    const ThemeStore = metro.findByStoreName("ThemeStore");

    const DIR = "pyoncord/gifcache";
    const WORKERS = 4;
    const MEGABYTE = 1024 * 1024;
    const SAVE_DELAY = 30_000;
    const SIZE_CHOICES = [256, 512, 1024, 2048, 4096];

    const storage = plugin.storage;
    storage.maxMegabytes ??= 512;
    // plugin storage persists on every nested write, so work on plain copies and save in batches
    let index = JSON.parse(JSON.stringify(storage.index ?? {}));
    let known = JSON.parse(JSON.stringify(storage.known ?? {}));

    const documents = files.getConstants().DocumentsDirPath;
    const downloading = new Set();
    let queue = [];
    let workers = 0;
    let saveTimer;

    // discord re-signs attachment links, so the signature params can't be part of the key
    function cacheKey(src) {
        try {
            const url = new URL(src);
            for (const param of ["ex", "is", "hm"]) url.searchParams.delete(param);
            return url.toString();
        } catch {
            return null;
        }
    }

    function fileName(key) {
        let hash = 0x811c9dc5;
        for (let i = 0; i < key.length; i++) hash = Math.imul(hash ^ key.charCodeAt(i), 0x01000193);
        const ext = (new URL(key).pathname.match(/\.(gif|webp|avif|png|jpe?g)$/i) ?? [])[1] ?? "img";
        return `${(hash >>> 0).toString(16)}-${key.length}.${ext}`;
    }

    function save() {
        if (saveTimer) clearTimeout(saveTimer);
        saveTimer = undefined;
        storage.index = JSON.parse(JSON.stringify(index));
        storage.known = JSON.parse(JSON.stringify(known));
    }

    function saveSoon() {
        saveTimer ??= setTimeout(save, SAVE_DELAY);
    }

    async function download(key, src) {
        const response = await fetch(src);
        if (!response.ok || !(response.headers.get("content-type") ?? "").startsWith("image/")) return;
        const bytes = await response.arrayBuffer();
        if (!bytes.byteLength) return;
        const name = fileName(key);
        await files.writeFile("documents", `${DIR}/${name}`, Buffer.from(bytes).toString("base64"), "base64");
        index[key] = { file: name, size: bytes.byteLength, lastUsed: Date.now() };
        saveSoon();
    }

    async function work() {
        workers++;
        try {
            for (let next = queue.shift(); next; next = queue.shift()) {
                // stop at the limit, or every launch would refetch what the last eviction dropped
                if (storedBytes() >= storage.maxMegabytes * MEGABYTE) break;
                const [key, src] = next;
                if (index[key] || downloading.has(key)) continue;
                downloading.add(key);
                try {
                    await download(key, src);
                } catch {
                } finally {
                    downloading.delete(key);
                }
            }
        } finally {
            workers--;
            if (!workers) await evict();
        }
    }

    function prefetch() {
        queue = Object.entries(known).filter(([key]) => !index[key]);
        for (let i = workers; i < WORKERS && queue.length; i++) void work();
    }

    const storedBytes = () => Object.values(index).reduce((sum, entry) => sum + entry.size, 0);

    async function evict() {
        const entries = Object.entries(index).sort(([, a], [, b]) => a.lastUsed - b.lastUsed);
        let total = storedBytes();
        const limit = storage.maxMegabytes * MEGABYTE;
        for (const [key, entry] of entries) {
            if (total <= limit) break;
            total -= entry.size;
            delete index[key];
            await files.removeFile("documents", `${DIR}/${entry.file}`).catch(() => {});
        }
        save();
    }

    function learnFavorites(_args, favorites) {
        if (!Array.isArray(favorites)) return favorites;
        let learned = false;
        for (const favorite of favorites) {
            const key = typeof favorite?.src === "string" && cacheKey(favorite.src);
            if (!key || known[key] === favorite.src) continue;
            known[key] = favorite.src;
            learned = true;
        }
        if (learned) {
            saveSoon();
            prefetch();
        }
        return favorites;
    }

    function serveFromDisk(args) {
        const props = args[1];
        const uri = props?.source?.uri;
        if (typeof uri !== "string" || !uri.startsWith("http")) return;
        const key = cacheKey(uri);
        const entry = key && index[key];
        if (!entry) return;
        entry.lastUsed = Date.now();
        saveSoon();
        args[1] = { ...props, source: { ...props.source, uri: `file://${documents}/${DIR}/${entry.file}` } };
    }

    async function clear() {
        queue = [];
        const entries = Object.values(index);
        index = {};
        save();
        for (const entry of entries) await files.removeFile("documents", `${DIR}/${entry.file}`).catch(() => {});
    }

    const color = token => resolver.resolveSemanticColor(ThemeStore.theme, ui.semanticColors[token]);
    const formatSize = megabytes => (megabytes >= 1024 ? `${megabytes / 1024} GB` : `${megabytes} MB`);

    async function setLimit(megabytes) {
        storage.maxMegabytes = megabytes;
        await evict();
        prefetch();
    }

    function SizeChoice({ megabytes, onPick }) {
        const selected = storage.maxMegabytes === megabytes;
        return React.createElement(Pressable, {
            onPress: () => onPick(megabytes),
            accessibilityRole: "radio",
            accessibilityState: { selected },
            style: {
                flexGrow: 1,
                paddingVertical: 10,
                borderRadius: 8,
                alignItems: "center",
                backgroundColor: color(selected ? "CONTROL_PRIMARY_BACKGROUND_DEFAULT" : "BACKGROUND_MOD_NORMAL"),
            },
        }, React.createElement(Text, {
            style: { color: color(selected ? "CONTROL_PRIMARY_TEXT_DEFAULT" : "TEXT_DEFAULT"), fontSize: 14 },
        }, formatSize(megabytes)));
    }

    function Settings() {
        const [, rerender] = React.useReducer(n => n + 1, 0);
        const stored = Object.keys(index).length;
        const megabytes = storedBytes() / MEGABYTE;
        const pick = async value => {
            await setLimit(value);
            rerender();
        };
        return React.createElement(View, { style: { padding: 16, gap: 12 } },
            React.createElement(Text, { style: { color: color("TEXT_DEFAULT"), fontSize: 16 } },
                `${stored} of ${Object.keys(known).length} favourites stored, ${megabytes.toFixed(1)} MB of ${formatSize(storage.maxMegabytes)}`),
            React.createElement(Text, { style: { color: color("TEXT_MUTED"), fontSize: 14 } }, "Storage limit"),
            React.createElement(View, { style: { flexDirection: "row", gap: 8 } },
                ...SIZE_CHOICES.map(value => React.createElement(SizeChoice, { key: value, megabytes: value, onPick: pick }))),
            React.createElement(Pressable, {
                onPress: async () => {
                    await clear();
                    rerender();
                },
                style: { backgroundColor: color("CONTROL_CRITICAL_PRIMARY_BACKGROUND_DEFAULT"), borderRadius: 8, padding: 12, alignItems: "center" },
            }, React.createElement(Text, { style: { color: color("CONTROL_CRITICAL_PRIMARY_TEXT_DEFAULT"), fontSize: 16 } }, "Clear stored GIFs")));
    }

    let unpatches = [];

    return {
        onLoad() {
            for (const mod of new Set(metro.findByPropsAll("useSortedFavoriteGIFs"))) {
                unpatches.push(patcher.after("useSortedFavoriteGIFs", mod, learnFavorites));
            }
            unpatches.push(patcher.before("jsx", runtime, serveFromDisk));
            unpatches.push(patcher.before("jsxs", runtime, serveFromDisk));
            prefetch();
        },
        onUnload() {
            for (const unpatch of unpatches) unpatch();
            unpatches = [];
            queue = [];
            save();
        },
        settings: Settings,
    };
})()
