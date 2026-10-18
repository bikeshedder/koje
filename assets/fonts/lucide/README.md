# Lucide icon font

Icon font for koje's bar configuration and UI ([ADR 0020](../../../docs/decisions/0020-lucide-icon-font.md)).

| | |
|---|---|
| Source | npm package [`lucide-static`](https://www.npmjs.com/package/lucide-static) 1.59.0, `font/` directory |
| Integrity | npm `sha512-MuHZYaXbHgzN2QO6D3OM19bj09A+8GJiFSdXIMlbnLLEGQQ9+NRH2KaCxuivevD3Y23vM74kV17XBifgtqhfEg==` (checked) |
| `lucide.ttf` | sha256 `387dd5a1c10fc24701551f1f19d9384f4b871a799d78284d46f6bdc4205a54bd`, font family `lucide` |
| `codepoints.json` | icon name → code point (Private Use Area, e.g. `wifi` = U+E1AE) |
| License | `LICENSE`: ISC (Lucide), MIT for icons derived from Feather |

Installed by `./install.sh` to `$XDG_DATA_HOME/fonts/koje/` (default
`~/.local/share/fonts/koje/`).

## Updating

Code points have been stable: between 1.49.0 and 1.59.0 no shared icon changed
its code point and none was removed. Still, check before bumping:

```sh
v=<new version>
dir=$(mktemp -d) && cd "$dir"
curl -fsSL -o pkg.tgz "$(npm view lucide-static@$v dist.tarball)"
want=$(npm view lucide-static@$v dist.integrity | sed 's/^sha512-//')
[ "$want" = "$(openssl dgst -sha512 -binary pkg.tgz | base64 -w0)" ] && tar -xzf pkg.tgz
python3 -c "import json;a=json.load(open('$OLDPWD/codepoints.json'));b=json.load(open('package/font/codepoints.json'));print([k for k in a if b.get(k)!=a[k]])"
```

The last line must print `[]` (no icon changed or removed). Then copy
`font/lucide.ttf`, `font/codepoints.json` and `LICENSE`, and update this file.
