// ESLint for koje's GNOME Shell extension: GJS (ESM) inside gnome-shell.
import js from '@eslint/js';

export default [
    js.configs.recommended,
    {
        files: ['extension/**/*.js'],
        languageOptions: {
            ecmaVersion: 2024,
            sourceType: 'module',
            globals: {
                // gnome-shell's and GJS's globals
                global: 'readonly',
                log: 'readonly',
                logError: 'readonly',
                console: 'readonly',
                TextDecoder: 'readonly',
                TextEncoder: 'readonly',
            },
        },
        rules: {
            'no-unused-vars': ['error', {argsIgnorePattern: '^_'}],
            'prefer-const': 'error',
            'eqeqeq': 'error',
            'indent': ['error', 4, {SwitchCase: 0}],
            'quotes': ['error', 'single', {avoidEscape: true}],
            'semi': ['error', 'always'],
        },
    },
];
