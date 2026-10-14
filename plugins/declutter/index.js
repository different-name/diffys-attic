(() => {
    const { patcher, metro } = vendetta;
    const runtime = metro.findByProps("jsx", "jsxs", "Fragment");
    const UserStore = metro.findByStoreName("UserStore");
    const UserProfileStore = metro.findByStoreName("UserProfileStore");
    const ApexExperimentStore = metro.findByStoreName("ApexExperimentStore");
    const { View } = metro.common.ReactNative;

    const navButtons = new Set(["Quests", "Shop", "Nitro", "Send a gift"]);
    const settingsRows = new Set(["Get Nitro", "Shop", "Quests", "Server Boost", "Gift Inventory"]);
    const strippedUserFields = ["primaryGuild", "displayNameStyles"];
    const strippedUserGetters = ["avatarDecoration", "nameplate"];

    function isClutter(props) {
        if (!props) return false;
        const a11y = props.accessibilityLabel;
        if (typeof a11y === "string") {
            if (props.IconComponent && navButtons.has(a11y)) return true;
            if (a11y.startsWith("Orbs balance")) return true;
            if (a11y === "Server Boosts" && "channelInfo" in props) return true;
            if (a11y === "Apps" && props.accessibilityRole === "button") return true;
        }
        if (typeof props.label === "string" && "arrow" in props && "subLabel" in props) {
            return settingsRows.has(props.label);
        }
        if (props.title === "Billing Settings") return true;
        // the profile tab bar is left with only main once the wishlist is removed
        const tabs = props.state?.items;
        return "variant" in props && props.state?.activeIndex != null && tabs?.length === 1 && tabs[0]?.id === "main";
    }

    const hiddenBadgePrefixes = ["gifting", "premium", "guild_booster", "quest_"];
    const isHiddenBadge = badge => typeof badge?.id === "string"
        && hiddenBadgePrefixes.some(prefix => badge.id.startsWith(prefix));

    function cleanProps(props) {
        let cleaned = props;
        if (Array.isArray(props.badges) && props.badges.some(isHiddenBadge)) {
            cleaned = { ...cleaned, badges: props.badges.filter(badge => !isHiddenBadge(badge)) };
        }
        // the channel list header is also the banner, so render it as if the server had none
        if (props.guild?.banner && "bannerHeight" in props) {
            cleaned = { ...cleaned, guild: { ...props.guild, banner: null } };
        }
        if (props.displayNameStylesEnabled) {
            cleaned = { ...cleaned, displayNameStylesEnabled: false, currentUserDisplayNameStyles: null };
        }
        return cleaned;
    }

    const isBoostRow = row => row === "guild-boosts";

    // hiding the boosts row still leaves its section divider, so drop it from the list model
    function stripBoostRow(channelList) {
        const actions = channelList.guildActionSection;
        if (!Array.isArray(actions?.guildActionRows) || !actions.guildActionRows.some(isBoostRow)) return;
        if (!Array.isArray(channelList.rows) || !Array.isArray(channelList.sections)) return;
        actions.guildActionRows = actions.guildActionRows.filter(row => !isBoostRow(row));
        channelList.rows.forEach((rows, section) => {
            if (!Array.isArray(rows) || !rows.some(isBoostRow)) return;
            channelList.rows[section] = rows.filter(row => !isBoostRow(row));
            channelList.sections[section] = channelList.rows[section].length;
        });
    }

    // some parents read .type on their children, so null crashes them
    const hidden = { style: { display: "none" } };

    function filter(args, orig) {
        const props = args[1];
        if (!props) return orig(...args);
        if (props.guildChannels) stripBoostRow(props.guildChannels);
        if (isClutter(props)) return orig(View, hidden, args[2]);
        args[1] = cleanProps(props);
        return orig(...args);
    }

    function withoutWishlist(args) {
        const options = args[0];
        if (Array.isArray(options?.items) && options.items.some(item => item?.id === "wishlist")) {
            args[0] = { ...options, items: options.items.filter(item => item?.id !== "wishlist") };
        }
    }

    function stripUserFields() {
        const UserRecord = metro.find(m => typeof m === "function"
            && typeof m.prototype?.hasHadPremium === "function"
            && typeof m.prototype?.getAvatarURL === "function");
        if (!UserRecord) return () => {};
        const proto = UserRecord.prototype;
        const originalGetters = strippedUserGetters
            .map(name => [name, Object.getOwnPropertyDescriptor(proto, name)])
            .filter(([, descriptor]) => descriptor?.get);

        // the constructor assigns these, so a prototype setter swallows them
        for (const field of strippedUserFields) {
            Object.defineProperty(proto, field, { configurable: true, get: () => null, set: () => {} });
        }
        for (const [name] of originalGetters) {
            Object.defineProperty(proto, name, { configurable: true, get: () => null });
        }
        for (const user of Object.values(UserStore.getUsers())) {
            for (const field of strippedUserFields) delete user[field];
        }
        UserStore.emitChange();

        return () => {
            for (const field of strippedUserFields) delete proto[field];
            for (const [name, descriptor] of originalGetters) Object.defineProperty(proto, name, descriptor);
        };
    }

    // profiles are plain objects shared by every screen, so return a cleaned copy with a stable identity
    const cleanedProfiles = new WeakMap();
    function cleanProfile(_args, profile) {
        if (!profile || typeof profile !== "object") return profile;
        let cleaned = cleanedProfiles.get(profile);
        if (!cleaned) {
            cleaned = {
                ...profile,
                profileEffect: null,
                themeColors: null,
                badges: Array.isArray(profile.badges) ? profile.badges.filter(badge => !isHiddenBadge(badge)) : profile.badges,
            };
            cleanedProfiles.set(profile, cleaned);
        }
        return cleaned;
    }

    const disabledExperiments = new Set([
        "2025-10-mobile-home-drawer",
        "2025-12-nitro-s-rewards",
        "2026-01-mobile-go-live-entrypoint",
        "2026-03-mobile-quest-home-red-dot-notification",
        "2026-04-gifting-badge",
        "2026-04-http-request-sample",
        "2026-04-mobile-boost-progress-bar",
        "2026-04-mobile-go-live-upsell",
        "2026-05-bounty-stale-refresh-quest-home",
        "2026-05-server-theme-user",
        "2026-06-android-two-week-trials",
        "2026-06-gummy-bears",
        "2026-06-gummy-viewers",
        "2026-06-mobile-friendship-anniversary",
        "2026-06-mobile-server-tag",
        "2026-07-ad-recheck-interval-experiment",
        "2026-07-custom-typing-indicator",
        "2026-07-mobile-boosting-optimizations",
        "2026-07-nitro-floating-subscribe",
        "2026-07-shop-this-look-mobile",
        "2026-08-bounties-mobile-quest-bar",
        "2026-08-nitro-tenure-badge-withheld-state",
        "2026-09-bounties-android-quest-bar-smoke-animation",
        "2026-09-gifting-badge-coachmark-audience",
        "2026-09-mobile-nitro-upsell-in-shop-feed",
    ]);

    // no assignment means getConfig and useConfig fall back to the default config and skip exposure tracking
    function unassignExperiments(args, result) {
        return disabledExperiments.has(args[2]) ? [result?.[0] ?? null, null] : result;
    }

    let unpatches = [];

    return {
        onLoad() {
            unpatches.push(patcher.instead("jsx", runtime, filter));
            unpatches.push(patcher.instead("jsxs", runtime, filter));
            for (const mod of metro.findByPropsAll("useSegmentedControlState")) {
                unpatches.push(patcher.before("useSegmentedControlState", mod, withoutWishlist));
            }
            unpatches.push(patcher.after("getUserProfile", UserProfileStore, cleanProfile));
            unpatches.push(patcher.after("getGuildMemberProfile", UserProfileStore, cleanProfile));
            unpatches.push(stripUserFields());
            unpatches.push(patcher.after("getEvaluationAndAssignmentInner", ApexExperimentStore, unassignExperiments));
            ApexExperimentStore.emitChange();
            UserProfileStore.emitChange();
        },
        onUnload() {
            for (const unpatch of unpatches) unpatch();
            unpatches = [];
            UserStore.emitChange();
            UserProfileStore.emitChange();
        },
    };
})()
