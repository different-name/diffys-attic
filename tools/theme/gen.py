import json, re, sys
from pathlib import Path

FLAVOR = sys.argv[1] if len(sys.argv) > 1 else "mocha"
ACCENT = sys.argv[2] if len(sys.argv) > 2 else "mauve"

PALETTES = {
    "mocha": dict(
        rosewater="#f5e0dc", flamingo="#f2cdcd", pink="#f5c2e7", mauve="#cba6f7", red="#f38ba8",
        maroon="#eba0ac", peach="#fab387", yellow="#f9e2af", green="#a6e3a1", teal="#94e2d5",
        sky="#89dceb", sapphire="#74c7ec", blue="#89b4fa", lavender="#b4befe", text="#cdd6f4",
        subtext1="#bac2de", subtext0="#a6adc8", overlay2="#9399b2", overlay1="#7f849c",
        overlay0="#6c7086", surface2="#585b70", surface1="#45475a", surface0="#313244",
        base="#1e1e2e", mantle="#181825", crust="#11111b",
    ),
}
if FLAVOR not in PALETTES:
    sys.exit(f"no palette for {FLAVOR}, known: {', '.join(PALETTES)}")
P = PALETTES[FLAVOR]
if ACCENT not in P:
    sys.exit(f"{ACCENT} is not a {FLAVOR} color")
accent = P[ACCENT]

HERE = Path(__file__).parent
tokens = json.loads((HERE / "tokens.json").read_text())
dump, raw = tokens["semantic"], tokens["raw"]


def to_rgb(h):
    h = h.lstrip("#")
    return [int(h[i:i + 2], 16) / 255 for i in (0, 2, 4)], (int(h[6:8], 16) / 255 if len(h) == 8 else 1)


def lin(c):
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def delin(c):
    c = max(0.0, min(1.0, c))
    return 12.92 * c if c <= 0.0031308 else 1.055 * c ** (1 / 2.4) - 0.055


def oklab(h):
    r, g, b = (lin(c) for c in to_rgb(h)[0])
    l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b) ** (1 / 3)
    m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b) ** (1 / 3)
    s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b) ** (1 / 3)
    return [0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
            1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
            0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s]


def from_oklab(L, a, b):
    l, m, s = ((L + 0.3963377774 * a + 0.2158037573 * b) ** 3,
               (L - 0.1055613458 * a - 0.0638541728 * b) ** 3,
               (L - 0.0894841775 * a - 1.2914855480 * b) ** 3)
    rgb = (4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
           -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
           -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s)
    return "#" + "".join(f"{round(delin(c) * 255):02x}" for c in rgb)


def mix(h1, h2, t):
    a, b = oklab(h1), oklab(h2)
    return from_oklab(*(x + (y - x) * t for x, y in zip(a, b)))


def with_alpha(h, alpha):
    return h if alpha >= 1 else h + f"{round(alpha * 255):02x}"


NEUTRAL_ANCHORS = [
    (1, "text"), (5, "text"), (8, "subtext1"), (23, "subtext1"), (29, "subtext0"),
    (35, "overlay2"), (43, "overlay1"), (50, "overlay0"), (55, "surface2"), (71, "surface1"),
    (73, "surface0"), (79, "mantle"), (82, "mantle"), (86, "base"), (92, "crust"), (100, "crust"),
]


def neutral(i):
    for (i0, c0), (i1, c1) in zip(NEUTRAL_ANCHORS, NEUTRAL_ANCHORS[1:]):
        if i0 <= i <= i1:
            return P[c0] if i1 == i0 else mix(P[c0], P[c1], (i - i0) / (i1 - i0))
    return P["crust"]


neutral_L = sorted((oklab(raw[f"NEUTRAL_{i}"])[0], i) for i in range(1, 101))


def gray(h):
    L = oklab(h)[0]
    for (L0, i0), (L1, i1) in zip(neutral_L, neutral_L[1:]):
        if L0 <= L <= L1:
            return neutral(i0 + (i1 - i0) * ((L - L0) / (L1 - L0) if L1 > L0 else 0))
    return neutral(100 if L < neutral_L[0][0] else 1)


# discord uses mid steps for both buttons and text, catppuccin uses the flat accent for both
def ramp(color, step):
    if step < 25:
        return mix(color, P["text"], (25 - step) / 25 * 0.6)
    if step > 55:
        return mix(color, P["crust"], (step - 55) / 45 * 0.9)
    return color


HUES = {
    "BLURPLE": accent, "BRAND": accent, "BLUE": P["blue"], "RED": P["red"], "GREEN": P["green"],
    "YELLOW": P["yellow"], "ORANGE": P["peach"], "TEAL": P["teal"], "PINK": P["pink"],
}

out_raw = {}
for key, value in raw.items():
    alpha = to_rgb(value)[1] if value.startswith("#") else 1
    if m := re.fullmatch(r"(NEUTRAL|PLUM|PRIMARY)_\d+", key):
        out_raw[key] = gray(value)
    elif m := re.fullmatch(r"(BLURPLE|BRAND|BLUE|RED|GREEN|YELLOW|ORANGE|TEAL|PINK)(_NEW)?_(\d+)", key):
        n = int(m[3])
        step = n / 10 if (not m[2] and m[1] not in ("BLURPLE", "PINK")) else n
        out_raw[key] = ramp(HUES[m[1]], step)
    elif m := re.fullmatch(r"OPACITY_(BLURPLE|BLUE|RED|GREEN|YELLOW|ORANGE|TEAL|PINK|WHITE|BLACK)_\d+", key):
        base = {"WHITE": P["text"], "BLACK": P["crust"]}.get(m[1]) or HUES[m[1]]
        out_raw[key] = with_alpha(base, alpha)
    elif re.fullmatch(r"OPACITY_\d+", key):
        out_raw[key] = with_alpha(P["overlay2"], alpha)
    # revenge xposed also maps raw keys onto android color resources, and a dark native white blanks the emoji grid
    elif key in ("WHITE", "WHITE_500"):
        out_raw[key] = P["text"]
    elif key in ("BLACK", "BLACK_500"):
        out_raw[key] = P["crust"]

out_raw.update({
    "GUILD_BOOSTING_BLUE": P["blue"], "GUILD_BOOSTING_BLUE_FOR_GRADIENTS": P["blue"],
    "GUILD_BOOSTING_PINK": P["pink"], "GUILD_BOOSTING_PINK_REFRESH": P["pink"],
    "GUILD_BOOSTING_PURPLE": P["mauve"], "GUILD_BOOSTING_PURPLE_FOR_GRADIENTS": P["mauve"],
})

ON_ACCENT = [
    "BADGE_TEXT_BRAND", "CHAT_INPUT_SEND_BUTTON_ICON_ACTIVE_TINT", "CHECKBOX_ICON_ACTIVE",
    "CONTROL_CONNECTED_TEXT_DEFAULT", "CONTROL_CRITICAL_PRIMARY_TEXT_DEFAULT",
    "CONTROL_PRIMARY_TEXT_DEFAULT", "MOBILE_VOICE_MESSAGE_RECORDING_LOCK_ICON_ACTIVE",
    "STATUS_POSITIVE_TEXT", "STATUS_WARNING_TEXT", "SWITCH_THUMB_BACKGROUND_SELECTED_DEFAULT",
    "WHITE",
]
semantic = {k: P["base"] for k in ON_ACCENT}
semantic["TEXT_LINK"] = accent

missing = [k for k in semantic if k not in dump]
if missing:
    sys.exit(f"unknown semantic tokens: {missing}")

theme = {
    "name": f"Catppuccin {FLAVOR.title()} {ACCENT.title()}",
    "description": f"Catppuccin {FLAVOR.title()} with a {ACCENT.title()} accent",
    "authors": [{"name": "diffy"}],
    "spec": 2,
    "semanticColors": {k: [v, v] for k, v in sorted(semantic.items())},
    "rawColors": dict(sorted(out_raw.items())),
}
path = HERE.parent.parent / "themes" / f"catppuccin-{FLAVOR}-{ACCENT}.json"
path.write_text(json.dumps(theme, indent=2) + "\n")
print(path, len(semantic), "semantic", len(out_raw), "raw")
