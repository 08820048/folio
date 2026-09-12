//! Asset source for the icons gpui-component does not ship.
//!
//! `gpui-component-assets` embeds a fixed directory of SVGs and `IconName` is
//! generated from that directory at build time by `icon_named!`, so extra icons
//! cannot be added to the enum. This wraps the bundled source instead: the
//! vendored icons live in `assets/icons` and are served under the same
//! `icons/<name>.svg` paths, everything else falls through untouched.
//!
//! Keep these in Lucide, matching the rule in AGENTS.md. The vendored files are
//! ISC licensed; see `assets/icons/LICENSE.txt`.

use gpui::{AssetSource, Result, SharedString};
use gpui_component::IconNamed;
use std::borrow::Cow;

/// Icons vendored in `assets/icons`.
///
/// Implements `IconNamed`, so it is accepted anywhere an `IconName` is, via the
/// blanket `impl From<T: IconNamed> for Icon` in gpui-component.
#[derive(Clone, Copy)]
pub enum FolioIcon {
    PanelLeftDashed,
    PanelRightDashed,
    SquareTerminal,
    GitCompare,
    GapHorizontal,
}

impl IconNamed for FolioIcon {
    fn path(self) -> SharedString {
        match self {
            Self::PanelLeftDashed => "icons/panel-left-dashed.svg".into(),
            Self::PanelRightDashed => "icons/panel-right-dashed.svg".into(),
            Self::SquareTerminal => "icons/square-terminal.svg".into(),
            Self::GitCompare => "icons/git-compare.svg".into(),
            Self::GapHorizontal => "icons/gap-horizontal.svg".into(),
        }
    }
}

/// Path and bytes of every vendored icon, keyed by asset path.
const ICONS: &[(&str, &[u8])] = &[
    (
        "icons/panel-left-dashed.svg",
        include_bytes!("../assets/icons/panel-left-dashed.svg"),
    ),
    (
        "icons/panel-right-dashed.svg",
        include_bytes!("../assets/icons/panel-right-dashed.svg"),
    ),
    (
        "icons/square-terminal.svg",
        include_bytes!("../assets/icons/square-terminal.svg"),
    ),
    (
        "icons/git-compare.svg",
        include_bytes!("../assets/icons/git-compare.svg"),
    ),
    (
        "icons/gap-horizontal.svg",
        include_bytes!("../assets/icons/gap-horizontal.svg"),
    ),
];

/// The application's asset source.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = ICONS.iter().find(|(name, _)| *name == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut items = gpui_component_assets::Assets.list(path)?;
        items.extend(
            ICONS
                .iter()
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
        for (path, _) in ICONS {
            let loaded = assets.load(path).unwrap();
            assert!(loaded.is_some(), "`{path}` is declared but did not load");
        }
        for icon in [
            FolioIcon::PanelLeftDashed,
            FolioIcon::PanelRightDashed,
            FolioIcon::SquareTerminal,
            FolioIcon::GitCompare,
            FolioIcon::GapHorizontal,
        ] {
            let path = icon.path();
            assert!(
                assets.load(&path).unwrap().is_some(),
                "`{path}` is not in the asset source"
            );
        }
        // Anything the component library ships must keep working.
        let bundled = assets.load("icons/close.svg").unwrap();
        assert!(bundled.is_some());
        // So must the ones we deliberately do not serve.
        assert!(assets.load("icons/not-a-real-icon.svg").is_err());
    }

    /// A malformed SVG shows up as a blank icon rather than an error, so check
    /// the vendored bytes really rasterize.
    #[test]
    fn vendored_icons_are_renderable_svgs() {
        for (path, bytes) in ICONS {
            let rendered =
                gpui::SvgRenderer::new(std::sync::Arc::new(())).render_single_frame(bytes, 1.);
            assert!(rendered.is_ok(), "`{path}` did not rasterize: {rendered:?}");
        }
    }
}
