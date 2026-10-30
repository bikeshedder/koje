// Colors for what the extension draws: kojed's `#RRGGBB`, text on them, the
// memory meter's levels. Pure (vitest).

export type Rgb = [number, number, number];

export function rgb(hex: string): Rgb {
    const n = parseInt(hex.slice(1), 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** Whether white text reads better than black on `hex` (WCAG luminance). */
export function dark(hex: string): boolean {
    const [r, g, b] = rgb(hex).map(c => {
        const s = c / 255;
        return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * b < 0.4;
}

/** Text color on `hex` for the workspace pills (perceived brightness). */
export function textOn(hex: string): string {
    const [r, g, b] = rgb(hex);
    return 0.299 * r + 0.587 * g + 0.114 * b < 150 ? '#ffffff' : 'rgba(0, 0, 0, 0.87)';
}

// Amber from this share of the soft limit (koje_core::memory::WARN_PERCENT).
const WARN = 0.9;

/** The memory meter's used part by level (GNOME's palette, as koje-shell's). */
export function levelColor(used: number, soft: number, hard: number): Rgb {
    if (hard > 0 && used >= hard)
        return [0xe0, 0x1b, 0x24];
    if (soft > 0 && used >= soft)
        return [0xe6, 0x61, 0x00];
    if (soft > 0 && used >= soft * WARN)
        return [0xe5, 0xa5, 0x0a];
    return [0x2e, 0xc2, 0x7e];
}
