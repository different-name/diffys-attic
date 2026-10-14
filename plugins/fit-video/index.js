(() => {
    const { patcher } = vendetta;
    const byPath = path => bunny.metro.findByFilePath(path, false);

    const VideoUploadUtils = byPath("modules/media_uploads/native/VideoUploadUtils.tsx");
    const UploadLimits = byPath("modules/media_uploads/UploadLimits.tsx");
    const FileUtils = byPath("utils/FileUtils.tsx");
    const PreCompression = byPath("lib/uploader/native/getPreCompressionFileSize.tsx");

    // room for encoder overshoot and mp4 overhead
    const SIZE_MARGIN = 0.94;
    const MIN_VIDEO_BITRATE = 100_000;
    const MAX_AUDIO_BITRATE = 512_000;
    const FALLBACK_AUDIO_BITRATE = 320_000;
    // discord's encoder caps output at 30fps on android 15+
    const MAX_FRAME_RATE = 30;
    const MIN_BITS_PER_PIXEL = 0.05;
    const HEIGHTS = [1080, 720, 540, 480, 360, 240];
    const skippableFormat = /(avc1|hvc1|video\/(avc|hevc))/i;
    const videoExtension = /\.(mp4|m4v|mov|webm|mkv|3gp|avi)$/i;

    let sourceSizes = new Map();
    let unpatches = [];

    const uploadLimit = () => UploadLimits.getEffectiveUploadLimit(FileUtils.maxFileSize());
    const budget = durationMs => uploadLimit() * 8000 * SIZE_MARGIN / durationMs;

    // the uploader only hands the encoder metadata, so the source size is matched up by shape
    const shapeKey = (width, height, seconds) =>
        `${Math.min(width, height)}x${Math.max(width, height)}@${Math.round(seconds)}`;

    function sourceSize(meta) {
        const known = sourceSizes.get(shapeKey(meta.width, meta.height, meta.durationMs / 1000));
        if (known) return known;
        if (!(meta.bitRate > 0)) return null;
        return (meta.bitRate + FALLBACK_AUDIO_BITRATE) * meta.durationMs / 8000;
    }

    // the encoder copies the audio track
    function audioBitrate(meta, size) {
        const total = size * 8000 / meta.durationMs;
        if (!(meta.bitRate > 0) || total <= meta.bitRate) return FALLBACK_AUDIO_BITRATE;
        return Math.min(total - meta.bitRate, MAX_AUDIO_BITRATE);
    }

    function plan(meta) {
        if (!(meta?.durationMs > 0 && meta.width > 0 && meta.height > 0)) return null;
        const size = sourceSize(meta);
        if (size == null || size <= uploadLimit() * SIZE_MARGIN) return null;

        const fit = budget(meta.durationMs) - audioBitrate(meta, size);
        const bitrate = Math.round(Math.max(MIN_VIDEO_BITRATE, Math.min(meta.bitRate || Infinity, fit)));

        const short = Math.min(meta.width, meta.height);
        const aspect = Math.max(meta.width, meta.height) / short;
        const frameRate = Math.min(meta.frameRate || MAX_FRAME_RATE, MAX_FRAME_RATE);
        const heights = [short, ...HEIGHTS.filter(height => height < short)];
        const resolution = heights.find(height => bitrate / (height * height * aspect * frameRate) >= MIN_BITS_PER_PIXEL)
            ?? heights[heights.length - 1];

        return { bitrate, resolution };
    }

    function isVideoItem(item) {
        if (item?.isImage) return false;
        const type = item?.mimeType ?? item?.type;
        if (typeof type === "string") return type.startsWith("video");
        return videoExtension.test(item?.filename ?? item?.uri ?? "");
    }

    function canFit(item) {
        const seconds = item?.playableDuration;
        if (!(seconds > 0)) return false;
        return budget(seconds * 1000) - FALLBACK_AUDIO_BITRATE >= MIN_VIDEO_BITRATE;
    }

    function recordSource(item, size) {
        if (!(item?.width > 0 && item?.height > 0 && item?.playableDuration > 0)) return;
        sourceSizes.set(shapeKey(item.width, item.height, item.playableDuration), size);
    }

    return {
        onLoad() {
            if (!VideoUploadUtils || !UploadLimits || !FileUtils || !PreCompression) return;

            unpatches.push(patcher.instead("canSkipVideoTranscode", VideoUploadUtils, (args, orig) => {
                const meta = args[1];
                const size = meta?.durationMs > 0 ? sourceSize(meta) : null;
                if (size == null) return orig(...args);
                if (size > uploadLimit() * SIZE_MARGIN) return false;
                return meta.format != null && skippableFormat.test(meta.format);
            }));

            unpatches.push(patcher.instead("calculateOptimalBitrate", VideoUploadUtils, (args, orig) =>
                plan(args[0])?.bitrate ?? orig(...args)));

            unpatches.push(patcher.instead("calculateTargetDimensions", VideoUploadUtils, (args, orig) => {
                const target = plan(args[0]);
                return target ? orig(args[0], target.resolution) : orig(...args);
            }));

            // the uploader rejects a source over the limit before compressing it
            unpatches.push(patcher.instead("getPreCompressionFileSize", PreCompression, (args, orig) =>
                Promise.resolve(orig(...args)).then(size => {
                    const item = args[0];
                    if (typeof size !== "number" || !isVideoItem(item)) return size;
                    recordSource(item, size);
                    return size > uploadLimit() && canFit(item) ? uploadLimit() : size;
                })));
        },
        onUnload() {
            for (const unpatch of unpatches) unpatch();
            unpatches = [];
            sourceSizes = new Map();
        },
    };
})()
