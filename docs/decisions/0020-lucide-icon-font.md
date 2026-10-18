# 0020. Lucide as koje's icon font

- Status: Accepted
- Date: 2026-10-18
- Source: M2.1 (Waybar showed missing icons), discussion

## Context

koje will ship a bar configuration (Waybar module, M8.2) and later a panel. The
icons have to be present on every system. Ubuntu's Font Awesome is version 4.7,
which lacks icons that Waybar's default config already uses (`F796`, ethernet);
newer Font Awesome or Nerd Fonts are not packaged everywhere.

## Decision

- koje ships the **Lucide** icon font (`lucide-static`, ISC; Feather-derived
  icons MIT) in `assets/fonts/lucide/`, with its `LICENSE` and `codepoints.json`.
- Version **1.59.0**, pinned; updates are deliberate and checked against the
  previous `codepoints.json` (no changed or removed code points), see the README
  next to the font.
- Installed per user into `$XDG_DATA_HOME/fonts/koje/` by `install.sh`
  (ADR 0021); font family `lucide`.
- koje's bar configuration and UI refer to icons by name and look up code points
  in `codepoints.json` where they are generated, instead of hard-coding them.

## Consequences

+ Consistent line icons, the same on every distribution; one font for bar and
  panel.
+ Permissive license; the license file travels with the font.
- About 0.9 MB binary in the repository; each update adds another copy to the
  history.
- Lucide releases almost daily (new icons); koje only follows when it needs an
  icon.
