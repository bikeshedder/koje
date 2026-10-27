//! The desktop's background (M8.82), as GNOME draws it, from GNOME's
//! settings (`org.gnome.desktop.background`), so GNOME Settings' Appearance
//! panel changes it. Without GTK: what to draw and where. `background_ui`
//! draws it on a layer-shell surface per monitor.

/// GNOME's `picture-options`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fit {
    /// Only the colors.
    None,
    /// Tiled at its size from the top left.
    Wallpaper,
    /// At its size in the middle.
    Centered,
    /// As large as fits whole, the colors around it.
    Scaled,
    /// Stretched to the screen.
    Stretched,
    /// Covers the screen, cut at the sides.
    #[default]
    Zoom,
    /// Across all monitors; drawn per monitor as `Zoom`.
    Spanned,
}

impl Fit {
    pub fn parse(s: &str) -> Self {
        match s {
            "none" => Self::None,
            "wallpaper" => Self::Wallpaper,
            "centered" => Self::Centered,
            "scaled" => Self::Scaled,
            "stretched" => Self::Stretched,
            "spanned" => Self::Spanned,
            _ => Self::Zoom,
        }
    }
}

/// GNOME's `color-shading-type`: from the primary to the secondary color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shading {
    #[default]
    Solid,
    /// Top to bottom.
    Vertical,
    /// Left to right.
    Horizontal,
}

impl Shading {
    pub fn parse(s: &str) -> Self {
        match s {
            "vertical" => Self::Vertical,
            "horizontal" => Self::Horizontal,
            _ => Self::Solid,
        }
    }
}

/// What the background shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Spec {
    /// A local image file; `None`: the colors alone.
    pub picture: Option<String>,
    pub fit: Fit,
    pub primary: [u8; 3],
    pub secondary: [u8; 3],
    pub shading: Shading,
    /// 0–100.
    pub opacity: u8,
}

/// GNOME's settings as read: `picture-uri`, `picture-uri-dark` in the dark
/// style, the rest as their keys say.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    pub uri: String,
    pub uri_dark: String,
    pub dark: bool,
    pub options: String,
    pub primary: String,
    pub secondary: String,
    pub shading: String,
    pub opacity: i32,
}

pub fn spec(s: &Settings) -> Spec {
    let uri = if s.dark && !s.uri_dark.is_empty() {
        &s.uri_dark
    } else {
        &s.uri
    };
    let fit = Fit::parse(&s.options);
    Spec {
        picture: (fit != Fit::None).then(|| file_path(uri)).flatten(),
        fit,
        primary: parse_color(&s.primary).unwrap_or([0x02, 0x3c, 0x88]),
        secondary: parse_color(&s.secondary).unwrap_or([0, 0, 0]),
        shading: Shading::parse(&s.shading),
        opacity: u8::try_from(s.opacity.clamp(0, 100)).unwrap_or(100),
    }
}

/// `file:///usr/share/backgrounds/a%20b.png` → `/usr/share/backgrounds/a b.png`;
/// other schemes are not loaded.
pub fn file_path(uri: &str) -> Option<String> {
    let path = if let Some(rest) = uri.strip_prefix("file://") {
        rest
    } else if uri.starts_with('/') {
        uri
    } else {
        return None;
    };
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = path.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).ok().filter(|p| !p.is_empty())
}

/// `#023c88` (GNOME also writes `#rrrrggggbbbb` rarely; the first two
/// digits of each are taken then).
pub fn parse_color(s: &str) -> Option<[u8; 3]> {
    let hex = s.strip_prefix('#')?;
    let n = match hex.len() {
        6 => 2,
        12 => 4,
        _ => return None,
    };
    let part = |i: usize| u8::from_str_radix(hex.get(i * n..i * n + 2)?, 16).ok();
    Some([part(0)?, part(1)?, part(2)?])
}

/// The first picture of a GNOME slideshow (`<background>` XML, as Ubuntu's
/// and GNOME's own backgrounds are): `<static><file>…</file>` or a `<file>`
/// with `<size>` variants (the first).
pub fn slideshow_first(xml: &str) -> Option<String> {
    let file = xml.split("<file").nth(1)?;
    let body = file.split_once('>')?.1;
    let body = body.split("</file>").next()?;
    let text = match body.split("<size").nth(1) {
        Some(size) => size.split_once('>')?.1.split("</size>").next()?,
        None => body,
    };
    Some(text.trim().to_owned()).filter(|t| !t.is_empty())
}

/// A rectangle in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Where the picture of size `image` goes on a screen of size `screen`:
/// one rectangle, several for tiles, none without a picture.
pub fn place(fit: Fit, image: (f64, f64), screen: (f64, f64)) -> Vec<Rect> {
    let (iw, ih) = image;
    let (sw, sh) = screen;
    if iw <= 0.0 || ih <= 0.0 || sw <= 0.0 || sh <= 0.0 {
        return Vec::new();
    }
    let centered = |w: f64, h: f64| Rect {
        x: ((sw - w) / 2.0).round(),
        y: ((sh - h) / 2.0).round(),
        w,
        h,
    };
    match fit {
        Fit::None => Vec::new(),
        Fit::Stretched => vec![Rect {
            x: 0.0,
            y: 0.0,
            w: sw,
            h: sh,
        }],
        Fit::Centered => vec![centered(iw, ih)],
        Fit::Scaled => {
            let s = (sw / iw).min(sh / ih);
            vec![centered((iw * s).round(), (ih * s).round())]
        }
        Fit::Zoom | Fit::Spanned => {
            let s = (sw / iw).max(sh / ih);
            vec![centered((iw * s).round(), (ih * s).round())]
        }
        Fit::Wallpaper => {
            let mut tiles = Vec::new();
            let mut y = 0.0;
            while y < sh {
                let mut x = 0.0;
                while x < sw {
                    tiles.push(Rect { x, y, w: iw, h: ih });
                    x += iw;
                }
                y += ih;
            }
            tiles
        }
    }
}

/// The colors behind the picture as RGB rows, `width` × `height`.
pub fn fill(spec: &Spec, width: usize, height: usize) -> Vec<u8> {
    let mut out = vec![0; width * height * 3];
    let mix = |t: f64| -> [u8; 3] {
        let mut c = [0; 3];
        for (i, v) in c.iter_mut().enumerate() {
            let a = f64::from(spec.primary[i]);
            let b = f64::from(spec.secondary[i]);
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "0–255"
            )]
            {
                *v = (a + (b - a) * t).round().clamp(0.0, 255.0) as u8;
            }
        }
        c
    };
    #[allow(clippy::cast_precision_loss, reason = "screen sizes")]
    let along = |i: usize, n: usize| {
        if n > 1 {
            i as f64 / (n - 1) as f64
        } else {
            0.0
        }
    };
    for y in 0..height {
        for x in 0..width {
            let c = match spec.shading {
                Shading::Solid => spec.primary,
                Shading::Vertical => mix(along(y, height)),
                Shading::Horizontal => mix(along(x, width)),
            };
            let at = (y * width + x) * 3;
            out[at..at + 3].copy_from_slice(&c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_as_gnome_reads_them() {
        let s = Settings {
            uri: "file:///usr/share/backgrounds/light%20one.png".into(),
            uri_dark: "file:///usr/share/backgrounds/dark.png".into(),
            dark: false,
            options: "zoom".into(),
            primary: "#023c88".into(),
            secondary: "#5789ca".into(),
            shading: "solid".into(),
            opacity: 100,
        };
        let light = spec(&s);
        assert_eq!(
            light.picture.as_deref(),
            Some("/usr/share/backgrounds/light one.png")
        );
        assert_eq!(light.primary, [0x02, 0x3c, 0x88]);
        let dark = spec(&Settings {
            dark: true,
            ..s.clone()
        });
        assert_eq!(
            dark.picture.as_deref(),
            Some("/usr/share/backgrounds/dark.png")
        );
        // `none`: the colors only.
        let none = spec(&Settings {
            options: "none".into(),
            ..s
        });
        assert_eq!((none.picture, none.fit), (None, Fit::None));
    }

    #[test]
    fn uris_and_colors() {
        assert_eq!(file_path("https://x/y.png"), None);
        assert_eq!(file_path("/a/b.jpg").as_deref(), Some("/a/b.jpg"));
        assert_eq!(parse_color("#ff0080"), Some([255, 0, 128]));
        assert_eq!(parse_color("#ffff00008080"), Some([255, 0, 128]));
        assert_eq!(parse_color("red"), None);
    }

    #[test]
    fn slideshows_show_their_first_picture() {
        let xml = "<background><starttime/><static><duration>1</duration>\
                   <file>/usr/share/backgrounds/a.png</file></static></background>";
        assert_eq!(
            slideshow_first(xml).as_deref(),
            Some("/usr/share/backgrounds/a.png")
        );
        let sizes = "<background><static><file>\n<size width=\"1024\" height=\"768\">/b-1.jpg</size>\
                     <size width=\"2048\" height=\"1536\">/b-2.jpg</size></file></static></background>";
        assert_eq!(slideshow_first(sizes).as_deref(), Some("/b-1.jpg"));
        assert_eq!(slideshow_first("<background/>"), None);
    }

    #[test]
    fn pictures_placed_as_gnome() {
        let screen = (1920.0, 1080.0);
        let img = (1000.0, 1000.0);
        let r = place(Fit::Zoom, img, screen);
        assert_eq!((r[0].w, r[0].h, r[0].y), (1920.0, 1920.0, -420.0));
        let r = place(Fit::Scaled, img, screen);
        assert_eq!((r[0].x, r[0].w, r[0].h), (420.0, 1080.0, 1080.0));
        let r = place(Fit::Centered, img, screen);
        assert_eq!((r[0].x, r[0].y), (460.0, 40.0));
        assert_eq!(place(Fit::Wallpaper, img, screen).len(), 2 * 2);
        assert_eq!(place(Fit::None, img, screen), []);
        assert_eq!(place(Fit::Stretched, img, screen)[0].w, 1920.0);
    }

    #[test]
    fn color_shading() {
        let s = Spec {
            primary: [0, 0, 0],
            secondary: [255, 255, 255],
            shading: Shading::Vertical,
            ..Spec::default()
        };
        let px = fill(&s, 1, 3);
        assert_eq!(px, [0, 0, 0, 128, 128, 128, 255, 255, 255]);
        let solid = fill(
            &Spec {
                primary: [1, 2, 3],
                ..Spec::default()
            },
            2,
            1,
        );
        assert_eq!(solid, [1, 2, 3, 1, 2, 3]);
    }
}
