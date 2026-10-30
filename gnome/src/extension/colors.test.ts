import {describe, expect, it} from 'vitest';

import {dark, levelColor, rgb, textOn} from './colors.js';

describe('colors', () => {
    it('reads #RRGGBB', () => {
        expect(rgb('#2E7D5B')).toEqual([0x2e, 0x7d, 0x5b]);
        expect(rgb('#ffffff')).toEqual([255, 255, 255]);
    });

    it('puts white text on dark colors and black on light ones', () => {
        expect(dark('#000000')).toBe(true);
        expect(dark('#1c71d8')).toBe(true);
        expect(dark('#f6d32d')).toBe(false);
        expect(textOn('#1c71d8')).toBe('#ffffff');
        expect(textOn('#f6d32d')).toBe('rgba(0, 0, 0, 0.87)');
    });

    it('colors the meter by level', () => {
        const green = [0x2e, 0xc2, 0x7e];
        expect(levelColor(1, 0, 0)).toEqual(green);
        expect(levelColor(80, 100, 0)).toEqual(green);
        expect(levelColor(90, 100, 0)).toEqual([0xe5, 0xa5, 0x0a]);
        expect(levelColor(100, 100, 200)).toEqual([0xe6, 0x61, 0x00]);
        expect(levelColor(200, 100, 200)).toEqual([0xe0, 0x1b, 0x24]);
        expect(levelColor(200, 0, 200)).toEqual([0xe0, 0x1b, 0x24]);
    });
});
