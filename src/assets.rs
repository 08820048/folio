//! Asset source for the icons gpui-component does not ship.
//!
//! `gpui-component-assets` embeds a fixed directory of SVGs and `IconName` is
//! generated from that directory at build time by `icon_named!`, so extra icons
//! cannot be added to the enum. This wraps the bundled source instead: the
//! vendored icons live in `assets/icons` and are served under the same
//! `icons/<name>.svg` paths, everything else falls through untouched.
//!
//! The line icons are Lucide (ISC, `assets/icons/LICENSE.txt`). The main
//! surfaces use the glass sets in `assets/icons/glass` (Black Glass) and
//! `assets/icons/glass-blue` (Glass) instead, see [`GlassIcon`] and the rule
//! in AGENTS.md. The file-type logos in
//! `crate::file_icons` are served here too.

use crate::file_icons::FILE_TYPE_ICONS;
use gpui::{AssetSource, Result, SharedString};
use gpui_component::IconNamed;
use std::borrow::Cow;

/// Icons vendored in `assets/icons`.
///
/// Implements `IconNamed`, so it is accepted anywhere an `IconName` is, via the
/// blanket `impl From<T: IconNamed> for Icon` in gpui-component.
#[derive(Clone, Copy)]
pub enum FolioIcon {
    GapHorizontal,
}

impl IconNamed for FolioIcon {
    fn path(self) -> SharedString {
        match self {
            Self::GapHorizontal => "icons/gap-horizontal.svg".into(),
        }
    }
}

/// Path and bytes of every vendored icon, keyed by asset path.
const ICONS: &[(&str, &[u8])] = &[(
    "icons/gap-horizontal.svg",
    include_bytes!("../assets/icons/gap-horizontal.svg"),
)];

/// A glass icon: a full-colour raster drawn with `gpui::img` at its own
/// colours, never tinted. Each comes in two materials from the same set of
/// shapes, see [`Tone`]. The PNGs are 96px so they stay sharp up to 48pt on
/// a Retina display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlassIcon {
    Branch,
    Changes,
    Files,
    Layout,
    Open,
    Preview,
    QuickOpen,
    Recent,
    Run,
    Search,
    Settings,
    Terminal,
}

/// The two glass materials. Black Glass is the resting state on the light
/// theme; Glass (blue) marks what is active or primary there, and is the
/// only material on the dark theme, where black glass disappears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Black,
    Blue,
}

impl GlassIcon {
    #[cfg(test)]
    pub const ALL: &[GlassIcon] = &[
        Self::Branch,
        Self::Changes,
        Self::Files,
        Self::Layout,
        Self::Open,
        Self::Preview,
        Self::QuickOpen,
        Self::Recent,
        Self::Run,
        Self::Search,
        Self::Settings,
        Self::Terminal,
    ];

    fn stem(self) -> &'static str {
        match self {
            Self::Branch => "branch",
            Self::Changes => "changes",
            Self::Files => "files",
            Self::Layout => "layout",
            Self::Open => "open",
            Self::Preview => "preview",
            Self::QuickOpen => "quickopen",
            Self::Recent => "recent",
            Self::Run => "run",
            Self::Search => "search",
            Self::Settings => "settings",
            Self::Terminal => "terminal",
        }
    }

    pub fn path(self, tone: Tone) -> SharedString {
        let dir = match tone {
            Tone::Black => "glass",
            Tone::Blue => "glass-blue",
        };
        format!("icons/{dir}/{}.png", self.stem()).into()
    }
}

const GLASS: &[(&str, &[u8])] = &[
    (
        "icons/glass/branch.png",
        include_bytes!("../assets/icons/glass/branch.png"),
    ),
    (
        "icons/glass/changes.png",
        include_bytes!("../assets/icons/glass/changes.png"),
    ),
    (
        "icons/glass/files.png",
        include_bytes!("../assets/icons/glass/files.png"),
    ),
    (
        "icons/glass/layout.png",
        include_bytes!("../assets/icons/glass/layout.png"),
    ),
    (
        "icons/glass/open.png",
        include_bytes!("../assets/icons/glass/open.png"),
    ),
    (
        "icons/glass/preview.png",
        include_bytes!("../assets/icons/glass/preview.png"),
    ),
    (
        "icons/glass/quickopen.png",
        include_bytes!("../assets/icons/glass/quickopen.png"),
    ),
    (
        "icons/glass/recent.png",
        include_bytes!("../assets/icons/glass/recent.png"),
    ),
    (
        "icons/glass/run.png",
        include_bytes!("../assets/icons/glass/run.png"),
    ),
    (
        "icons/glass/search.png",
        include_bytes!("../assets/icons/glass/search.png"),
    ),
    (
        "icons/glass/settings.png",
        include_bytes!("../assets/icons/glass/settings.png"),
    ),
    (
        "icons/glass/terminal.png",
        include_bytes!("../assets/icons/glass/terminal.png"),
    ),
    (
        "icons/glass-blue/branch.png",
        include_bytes!("../assets/icons/glass-blue/branch.png"),
    ),
    (
        "icons/glass-blue/changes.png",
        include_bytes!("../assets/icons/glass-blue/changes.png"),
    ),
    (
        "icons/glass-blue/files.png",
        include_bytes!("../assets/icons/glass-blue/files.png"),
    ),
    (
        "icons/glass-blue/layout.png",
        include_bytes!("../assets/icons/glass-blue/layout.png"),
    ),
    (
        "icons/glass-blue/open.png",
        include_bytes!("../assets/icons/glass-blue/open.png"),
    ),
    (
        "icons/glass-blue/preview.png",
        include_bytes!("../assets/icons/glass-blue/preview.png"),
    ),
    (
        "icons/glass-blue/quickopen.png",
        include_bytes!("../assets/icons/glass-blue/quickopen.png"),
    ),
    (
        "icons/glass-blue/recent.png",
        include_bytes!("../assets/icons/glass-blue/recent.png"),
    ),
    (
        "icons/glass-blue/run.png",
        include_bytes!("../assets/icons/glass-blue/run.png"),
    ),
    (
        "icons/glass-blue/search.png",
        include_bytes!("../assets/icons/glass-blue/search.png"),
    ),
    (
        "icons/glass-blue/settings.png",
        include_bytes!("../assets/icons/glass-blue/settings.png"),
    ),
    (
        "icons/glass-blue/terminal.png",
        include_bytes!("../assets/icons/glass-blue/terminal.png"),
    ),
];

/// The application's asset source.
pub struct Assets;

fn vendored() -> impl Iterator<Item = &'static (&'static str, &'static [u8])> {
    ICONS
        .iter()
        .chain(GLASS.iter())
        .chain(FILE_TYPE_ICONS.iter())
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = vendored().find(|(name, _)| *name == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut items = gpui_component_assets::Assets.list(path)?;
        items.extend(
            vendored()
                .filter(|(name, _)| name.starts_with(path))
                .map(|(name, _)| SharedString::from(*name)),
        );
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendored_icons_resolve_and_the_bundled_set_still_loads() {
        let assets = Assets;
        for (path, _) in vendored() {
            let loaded = assets.load(path).unwrap();
            assert!(loaded.is_some(), "`{path}` is declared but did not load");
        }
        let path = FolioIcon::GapHorizontal.path();
        assert!(
            assets.load(&path).unwrap().is_some(),
            "`{path}` is not in the asset source"
        );
        for icon in GlassIcon::ALL {
            for tone in [Tone::Black, Tone::Blue] {
                let path = icon.path(tone);
                assert!(
                    assets.load(&path).unwrap().is_some(),
                    "`{path}` is not in the asset source"
                );
            }
        }
        // Anything the component library ships must keep working.
        let bundled = assets.load("icons/close.svg").unwrap();
        assert!(bundled.is_some());
        // So must the ones we deliberately do not serve.
        assert!(assets.load("icons/not-a-real-icon.svg").is_err());
    }

    /// The glass icons are drawn by `gpui::img`, which sniffs the format;
    /// a file that is not really a PNG would draw nothing.
    #[test]
    fn glass_icons_are_pngs() {
        for (path, bytes) in GLASS {
            assert!(
                bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
                "`{path}` is not a PNG"
            );
        }
    }

    /// A malformed SVG shows up as a blank icon rather than an error, so check
    /// the vendored bytes really rasterize.
    #[test]
    fn vendored_icons_are_renderable_svgs() {
        for (path, bytes) in vendored().filter(|(path, _)| path.ends_with(".svg")) {
            let rendered =
                gpui::SvgRenderer::new(std::sync::Arc::new(())).render_single_frame(bytes, 1.);
            assert!(rendered.is_ok(), "`{path}` did not rasterize: {rendered:?}");
        }
    }
}
