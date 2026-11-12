(() => {
    const { patcher, metro } = vendetta;
    const byPath = path => bunny.metro.findByFilePath(path, false);
    const UserStore = metro.findByStoreName("UserStore");
    const UserProfileStore = metro.findByStoreName("UserProfileStore");
    const ApexExperimentStore = metro.findByStoreName("ApexExperimentStore");
    const ChannelListStore = metro.findByStoreName("ChannelListStore");
    const AccessibilityStore = metro.findByStoreName("AccessibilityStore");

    const hiddenSettings = new Set([
        "PREMIUM",
        "COLLECTIBLES_SHOP",
        "QUEST_HOME",
        "PREMIUM_MANAGE_SUBSCRIPTIONS",
        "PREMIUM_GUILD_BOOSTING",
        "PREMIUM_GIFTING",
        "GUILD_ROLE_SUBSCRIPTIONS",
        "PREMIUM_RESTORE_SUBSCRIPTION",
    ]);
    const hiddenNavIcons = ["QuestsIcon", "ShopIcon", "NitroWheelIcon"];
    const boostRows = new Set(["guild-boosts", "guild-premium-progress-bar"]);
    const strippedUserFields = ["primaryGuild", "displayNameStyles"];
    const strippedUserGetters = ["avatarDecoration", "nameplate"];

    const hiddenBadgePrefixes = ["gifting", "premium", "guild_booster", "quest_"];
    const isHiddenBadge = badge => typeof badge?.id === "string"
        && hiddenBadgePrefixes.some(prefix => badge.id.startsWith(prefix));

    function withoutSettings(args) {
        const sections = args[0]?.sections;
        if (!Array.isArray(sections)) return;
        args[0] = {
            ...args[0],
            sections: sections
                .map(section => (Array.isArray(section?.settings)
                    ? { ...section, settings: section.settings.filter(key => !hiddenSettings.has(key)) }
                    : section))
                .filter(section => !Array.isArray(section?.settings) || section.settings.length),
        };
    }

    function withoutBoostRows(args) {
        const rows = args[1]?.guildActionRows;
        if (Array.isArray(rows) && rows.some(row => boostRows.has(row))) {
            args[1] = { ...args[1], guildActionRows: rows.filter(row => !boostRows.has(row)) };
        }
    }

    // only the channel list sees the copy, server settings would save a null banner back
    const cleanedGuilds = new WeakMap();
    function withoutBanner(args) {
        const guild = args[0]?.guild;
        if (!guild?.banner || typeof guild !== "object") return;
        let cleaned = cleanedGuilds.get(guild);
        if (!cleaned) {
            cleaned = { ...guild, banner: null };
            cleanedGuilds.set(guild, cleaned);
        }
        args[0] = { ...args[0], guild: cleaned };
    }

    // the profile tab bar is left with only main once the wishlist is removed
    const isMainOnly = props => props?.state?.items?.length === 1 && props.state.items[0]?.id === "main";

    // components bind jsx at module init, so patch what they read at render time instead
    function patchRenderGates(unpatches) {
        const navIcon = byPath("modules/main_tabs_v2/native/tabs/you/YouScreenNavIcon.tsx")?.default;
        const icons = new Set(hiddenNavIcons.map(name => metro.findByProps(name)?.[name]).filter(Boolean));
        if (typeof navIcon?.type === "function") {
            unpatches.push(patcher.instead("type", navIcon, (args, orig) =>
                (icons.has(args[0]?.IconComponent) ? null : orig(...args))));
        }

        const createList = metro.findByProps("createList");
        if (createList) unpatches.push(patcher.before("createList", createList, withoutSettings));

        const orbs = metro.findByProps("useVirtualCurrencyMobileEnabled");
        if (orbs) {
            unpatches.push(patcher.after("useVirtualCurrencyMobileEnabled", orbs, (_args, result) =>
                ({ ...result, enabled: false })));
        }

        const billing = metro.findByProps("isPremiumGiftingSupported");
        if (billing) unpatches.push(patcher.instead("isPremiumGiftingSupported", billing, () => false));

        const appLauncher = metro.findByProps("getIsAppLauncherEnabled");
        if (appLauncher) unpatches.push(patcher.instead("getIsAppLauncherEnabled", appLauncher, () => false));

        const channelList = byPath("modules/channel_list_v2/native/RedesignChannelList.tsx")?.ChannelList;
        if (typeof channelList?.type === "function") unpatches.push(patcher.before("type", channelList, withoutBanner));

        for (const mod of new Set(metro.findByPropsAll("Tabs"))) {
            if (typeof mod.Tabs !== "function") continue;
            unpatches.push(patcher.instead("Tabs", mod, (args, orig) => (isMainOnly(args[0]) ? null : orig(...args))));
        }

        unpatches.push(patcher.before("getGuild", ChannelListStore, withoutBoostRows));

        Object.defineProperty(AccessibilityStore, "displayNameStylesEnabled", { configurable: true, get: () => false });
        unpatches.push(() => delete AccessibilityStore.displayNameStylesEnabled);
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
        "2026-01-mobile-go-live-entrypoint",
        "2026-04-gifting-badge",
        "2026-04-http-request-sample",
        "2026-04-mobile-go-live-upsell",
        "2026-05-bounty-stale-refresh-quest-home",
        "2026-05-server-theme-user",
        "2026-06-android-two-week-trials",
        "2026-07-custom-typing-indicator",
        "2026-07-nitro-floating-subscribe",
        "2026-07-shop-this-look-mobile",
        "2026-08-badge-management",
        "2026-08-bounties-mobile-quest-bar",
        "2026-08-nitro-tenure-badge-withheld-state",
        "2026-08-no-fill-logging",
        "2026-09-bounties-android-quest-bar-smoke-animation",
        "2026-09-gifting-badge-coachmark-audience",
        "2026-09-mobile-nitro-orbs-delivered-modal",
        "2026-09-mobile-nitro-upsell-in-shop-feed",
        "2026-09-mobile-sticker-picker-upsell-restyle",
        "2026-09-profile-frame-layer-preload-mobile",
        "2026-09-quest-mobile-bar-secondary-cta",
        "2026-10-recurring-referral-reminder",
        "2026-10-server-tag-upsell-on-profile",
    ]);

    // no assignment means getConfig and useConfig fall back to the default config and skip exposure tracking
    function unassignExperiments(args, result) {
        return disabledExperiments.has(args[2]) ? [result?.[0] ?? null, null] : result;
    }

    let unpatches = [];

    return {
        onLoad() {
            for (const mod of metro.findByPropsAll("useSegmentedControlState")) {
                unpatches.push(patcher.before("useSegmentedControlState", mod, withoutWishlist));
            }
            unpatches.push(patcher.after("getUserProfile", UserProfileStore, cleanProfile));
            unpatches.push(patcher.after("getGuildMemberProfile", UserProfileStore, cleanProfile));
            patchRenderGates(unpatches);
            unpatches.push(stripUserFields());
            unpatches.push(patcher.after("getEvaluationAndAssignmentInner", ApexExperimentStore, unassignExperiments));
            ApexExperimentStore.emitChange();
            UserProfileStore.emitChange();
            ChannelListStore.emitChange();
            AccessibilityStore.emitChange();
        },
        onUnload() {
            for (const unpatch of unpatches) unpatch();
            unpatches = [];
            UserStore.emitChange();
            UserProfileStore.emitChange();
            ChannelListStore.emitChange();
            AccessibilityStore.emitChange();
        },
    };
})()
