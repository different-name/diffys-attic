(() => {
    const { patcher, metro } = vendetta;

    const buttonPath = "modules/media_viewer/native/components/overlay/MediaViewerOverlayButtonFavoriteGIF.tsx";
    const videoExtensions = [".mp4", ".webm", ".mov", ".avi", ".mkv", ".flv", ".wmv", ".m4v", ".gifv"];
    const withThumbnails = new WeakMap();

    function isVideo(url) {
        if (!url) return false;
        const path = url.split("?")[0].toLowerCase();
        return videoExtensions.some(ext => path.endsWith(ext));
    }

    // cdn.discordapp.com ignores ?format=, media.discordapp.net honours it
    function videoThumbnail(url) {
        if (!url) return url;
        const media = url.replace("cdn.discordapp.com", "media.discordapp.net");
        if (!media.includes("media.discordapp.net") && !media.includes("images-ext")) return url;
        return media + (media.includes("?") ? "&" : "?") + "format=jpeg";
    }

    // the button renders nothing unless the media looks like a gif
    function asGif(source) {
        if (!source || source.isGIFV) return source;
        return {
            ...source,
            isGIFV: true,
            embedURI: source.embedURI || source.sourceURI || source.uri,
            videoURI: source.videoURI || source.uri,
            embedProviderName: source.embedProviderName || "",
        };
    }

    function fixFormat(args) {
        const gif = args[0];
        if (!gif || typeof gif !== "object") return;
        if (isVideo(gif.url) || isVideo(gif.src)) gif.format = 2;
        else if (gif.format === 2) gif.format = 1;
    }

    // the mobile picker can't render video favourites, so show a jpeg frame without touching the synced records
    function thumbnailVideos(_args, result) {
        if (!Array.isArray(result?.favorites)) return result;
        let cached = withThumbnails.get(result);
        if (!cached) {
            const favorites = result.favorites.map(favorite =>
                favorite && (isVideo(favorite.url) || isVideo(favorite.src))
                    ? { ...favorite, src: videoThumbnail(favorite.src || favorite.url) }
                    : favorite);
            cached = { ...result, favorites };
            withThumbnails.set(result, cached);
        }
        return cached;
    }

    let unpatches = [];

    return {
        onLoad() {
            const button = bunny.metro.findByFilePath(buttonPath, false)?.default;
            if (typeof button?.type === "function") {
                unpatches.push(patcher.before("type", button, args => {
                    args[0] = { ...args[0], source: asGif(args[0]?.source) };
                }));
            }

            const favorites = metro.findByProps("addFavoriteGIF");
            if (favorites) unpatches.push(patcher.before("addFavoriteGIF", favorites, fixFormat));

            const picker = metro.findByProps("useFavoriteGIFsMobile");
            if (picker) unpatches.push(patcher.after("useFavoriteGIFsMobile", picker, thumbnailVideos));
        },
        onUnload() {
            for (const unpatch of unpatches) unpatch();
            unpatches = [];
        },
    };
})()
