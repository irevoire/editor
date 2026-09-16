use std::{
    borrow::Cow,
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};

use config_macros::ConfigField;
use include_dir::{include_dir, Dir};
use jiff::SignedDuration;
use parking_lot::RwLock;
use thiserror::Error;

mod theme;
pub use theme::{Style, Theme, ThemeParseError};

/// The bundled default configuration.
static DEFAULT_CONFIG_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../default_config");

/// The bundled `default_config/config.kdl`, forming the root of every
/// [`ConfigLayer`] chain.
fn default_config_kdl() -> &'static str {
    DEFAULT_CONFIG_DIR
        .get_file("config.kdl")
        .expect("default_config/config.kdl must be bundled")
        .contents_utf8()
        .expect("default_config/config.kdl must be UTF-8")
}

/// The directory looked for in the directory of the file being edited, and
/// at the root of the project.
const LOCAL_CONFIG_DIR_NAME: &str = ".editor";

/// The global config of the editor. Contains every options available.
/// Can be extended by pushing layers to it.
/// When setting a value it's only set for the layer you own.
/// When retrieving a value, the whole chain of configs is iterated on until
/// we reach the default configuration where all values are defined.
#[derive(Debug, Clone)]
pub struct Config(Arc<InnerConfig>);

/// One layer of configuration, optionally chained to a parent layer.
/// It is GUARANTEED that all values are defined at some point.
#[derive(Debug)]
struct InnerConfig {
    parent: Option<Arc<InnerConfig>>,

    // The config at a specific layer
    layer: RwLock<ConfigLayer>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, ConfigField)]
struct ConfigLayer {
    status_bar: StatusBarConfig,
    theme: Option<Theme>,
    soft_wrap: Option<bool>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ConfigField)]
#[config(path = "status_bar")]
struct StatusBarConfig {
    animation_speed: Option<SignedDuration>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not read the config file: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Parse(#[from] ConfigParseError),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConfigParseError {
    #[error("failed to parse the config file: {0}")]
    Kdl(#[from] kdl::KdlError),
    #[error(
        "`status_bar > animation_speed` must be an integer number of milliseconds, found `{found}`"
    )]
    InvalidAnimationSpeed { found: String },
    #[error("`soft_wrap` must be a boolean, found `{found}`")]
    InvalidSoftWrap { found: String },
    #[error(transparent)]
    Theme(#[from] ThemeParseError),
    #[error("could not read theme file `{path}`: {message}")]
    ThemeFileIo { path: String, message: String },
}

/// Where a config layer's `config.kdl` (and its `theme/` directory) come
/// from.
enum ConfigSource<'a> {
    /// The default configuration bundled into the binary.
    Bundled,
    /// A whole directory for complex / complete configurations.
    Directory(&'a Path),
    /// Raw KDL text, mostly used for temporary / inline modification in the editor directly
    Raw(&'a str),
}

impl ConfigSource<'_> {
    fn load(&self) -> Result<(Cow<'_, str>, Option<&Path>), ConfigError> {
        match self {
            ConfigSource::Bundled => Ok((Cow::Borrowed(default_config_kdl()), None)),
            ConfigSource::Raw(text) => Ok((Cow::Borrowed(text), None)),
            ConfigSource::Directory(dir) => {
                let content = match fs::read_to_string(dir.join("config.kdl")) {
                    Ok(content) => content,
                    Err(err) if err.kind() == io::ErrorKind::NotFound => String::new(),
                    Err(err) => return Err(err.into()),
                };
                Ok((Cow::Owned(content), Some(*dir)))
            }
        }
    }
}

impl ConfigLayer {
    /// Parses one layer of config from `source`.
    pub fn parse(source: ConfigSource) -> Result<Self, ConfigError> {
        let (input, base_dir) = source.load()?;
        let document: kdl::KdlDocument = input.parse().map_err(ConfigParseError::Kdl)?;
        let mut config = ConfigLayer::default();

        if let Some(status_bar) = document.get("status_bar") {
            if let Some(children) = status_bar.children() {
                if let Some(value) = children.get_arg("animation_speed") {
                    let ms = value.as_integer().ok_or_else(|| {
                        ConfigParseError::InvalidAnimationSpeed {
                            found: value.to_string(),
                        }
                    })?;
                    config.status_bar.animation_speed =
                        Some(SignedDuration::from_millis(ms as i64));
                }
            }
        }

        if let Some(theme_node) = document.get("theme") {
            config.theme = Some(parse_theme_node(theme_node, base_dir)?);
        }

        if let Some(value) = document.get_arg("soft_wrap") {
            let enabled = value
                .as_bool()
                .ok_or_else(|| ConfigParseError::InvalidSoftWrap {
                    found: value.to_string(),
                })?;
            config.soft_wrap = Some(enabled);
        }

        Ok(config)
    }
}

/// Parses a top-level `theme` node. It's either:
/// - The name of a pre-defined theme
/// - The path to a local theme, resolved as `{base_dir}/theme/{path}`
/// - Undefined, aka the default theme
fn parse_theme_node(
    node: &kdl::KdlNode,
    base_dir: Option<&Path>,
) -> Result<Theme, ConfigParseError> {
    if let Some(path) = node.entries().first().and_then(|e| e.value().as_string()) {
        if let Some(theme) = Theme::built_in(path) {
            return Ok(theme);
        }

        let Some(base_dir) = base_dir else {
            return Err(ConfigParseError::ThemeFileIo {
                path: format!("{}/theme/{path}", display_base_dir(None)),
                message: "not a built-in theme name".to_string(),
            });
        };

        let theme_path = base_dir.join("theme").join(path);
        let content =
            fs::read_to_string(&theme_path).map_err(|err| ConfigParseError::ThemeFileIo {
                path: theme_path.display().to_string(),
                message: err.to_string(),
            })?;
        let document: kdl::KdlDocument =
            content
                .parse()
                .map_err(|err: kdl::KdlError| ConfigParseError::ThemeFileIo {
                    path: theme_path.display().to_string(),
                    message: err.to_string(),
                })?;
        Ok(Theme::parse(&document)?)
    } else if let Some(children) = node.children() {
        Ok(Theme::parse(children)?)
    } else {
        Ok(Theme::default())
    }
}

/// Mostly used for error messages
fn display_base_dir(base_dir: Option<&Path>) -> String {
    match base_dir {
        Some(dir) => dir.display().to_string(),
        None => "*default*".to_string(),
    }
}

impl Config {
    // Retrieve the default configuration defined at compile time
    // It's parsed only once at startup and can be called again for free.
    // You're guanranteed that every field is set.
    pub fn default() -> Config {
        Config(InnerConfig::default())
    }

    /// Fork the current config and load a new config layer from a given path.
    /// Falls back to an empty layer if the directory doesn't exist.
    pub fn load(&self, dir: &Path) -> Result<Config, ConfigError> {
        let layer = ConfigLayer::parse(ConfigSource::Directory(dir))?;
        Ok(self.load_layer(layer))
    }

    /// Fork the current config and load a new layer parsed directly from
    /// raw `text`.
    pub fn load_raw(&self, text: &str) -> Result<Config, ConfigError> {
        let layer = ConfigLayer::parse(ConfigSource::Raw(text))?;
        Ok(self.load_layer(layer))
    }

    fn load_layer(&self, layer: ConfigLayer) -> Config {
        Config(Arc::new(InnerConfig {
            parent: Some(self.0.clone()),
            layer: RwLock::new(layer),
        }))
    }

    /// Create a new empty config that uses the current config as a base.
    pub fn fork(&self) -> Config {
        self.load_layer(ConfigLayer::default())
    }
}

impl InnerConfig {
    fn default() -> Arc<InnerConfig> {
        let lazy = LazyLock::new(|| {
            ConfigLayer::parse(ConfigSource::Bundled)
                .expect("bundled default_config/config.kdl must be valid KDL")
        });
        Arc::new(Self {
            parent: None,
            layer: RwLock::new(lazy.clone()),
        })
    }

    fn resolve<T: Clone>(&self, get: impl Fn(&ConfigLayer) -> Option<T>) -> T {
        let mut current = self;
        loop {
            if let Some(value) = get(&current.layer.read()) {
                return value;
            }
            current = current
                .parent
                .as_deref()
                // Every config should starts with the base as its default configuration,
                // which means every field should be set.
                .expect("default_config/config.kdl must set every field");
        }
    }
}

/// Walk up from `start` looking for a directory containing a `.git` entry
/// (either a real directory, or the `.git` file used by git worktrees).
pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf)
}

/// Build the full configuration chain for a buffer at `buffer_path` (or with
/// From lowest to highest priority:
/// 1. default configuration
/// 2. global user config
/// 3. git repo root
/// 4. containing directory
pub fn build_chain(buffer_path: Option<&Path>) -> Config {
    let mut chain = Config::default();

    if let Some(global_dir) = global_config_dir() {
        chain = load_and_log(chain, &global_dir);
    }

    if let Some(buffer_path) = buffer_path {
        if let Some(repo_root) = find_git_root(buffer_path) {
            chain = load_and_log(chain, &repo_root.join(LOCAL_CONFIG_DIR_NAME));
        }
        if let Some(dir) = buffer_path.parent() {
            chain = load_and_log(chain, &dir.join(LOCAL_CONFIG_DIR_NAME));
        }
    }

    chain
}

fn load_and_log(parent: Config, dir: &Path) -> Config {
    match parent.load(dir) {
        Ok(layer) => layer,
        Err(err) => {
            log::error!("failed to load config directory {}: {err}", dir.display());
            parent
        }
    }
}

fn global_config_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var("HOME").ok()?).join(".config/editor"))
}

#[cfg(test)]
mod test {
    use insta::assert_debug_snapshot;

    use super::*;

    #[test]
    fn default_layer_parses() {
        // Sanity check: the bundled file stays valid and sets every field,
        // which is the invariant every `.expect(...)` accessor relies on.

        // TODO: Use facet to make sure that's actually the case.
        let _ = Config::default();
    }

    #[test]
    fn cloning_the_public_wrapper_shares_the_same_layer() {
        // This is the whole point of `Config` wrapping an `Arc`: any clone
        // handed to a child component still points at the same layer, so a
        // mutation through one clone is visible through every other one.
        let parent = Config::default();
        let child = parent.clone();

        child.set_status_bar_animation_speed(SignedDuration::from_millis(42));

        assert_eq!(
            parent.get_status_bar_animation_speed(),
            SignedDuration::from_millis(42)
        );
    }

    #[test]
    fn empty_layer_falls_back_to_bundled_default() {
        let layer = Config::default().fork();
        assert_eq!(
            layer.get_status_bar_animation_speed(),
            SignedDuration::from_millis(500)
        );
    }

    #[test]
    fn child_layer_overrides_parent() {
        let child = Config::default().fork();

        child.set_status_bar_animation_speed(SignedDuration::from_millis(100));
        assert_eq!(
            child.get_status_bar_animation_speed(),
            SignedDuration::from_millis(100)
        );
    }

    #[test]
    fn child_layer_falls_through_when_unset() {
        let parent = Config::default();
        parent.set_status_bar_animation_speed(SignedDuration::from_millis(250));
        let child = parent.fork();

        assert_eq!(
            child.get_status_bar_animation_speed(),
            SignedDuration::from_millis(250)
        );
    }

    #[test]
    fn sibling_layers_sharing_a_parent_dont_affect_each_other() {
        let parent = Config::default();
        parent.set_status_bar_animation_speed(SignedDuration::from_millis(250));

        let view_a = parent.fork();
        let view_b = parent.fork();

        view_a.set_status_bar_animation_speed(SignedDuration::from_millis(10));

        assert_eq!(
            view_a.get_status_bar_animation_speed(),
            SignedDuration::from_millis(10)
        );
        assert_eq!(
            view_b.get_status_bar_animation_speed(),
            SignedDuration::from_millis(250)
        );
    }

    #[test]
    fn parses_status_bar_animation_speed() {
        let layer = ConfigLayer::parse(ConfigSource::Raw(
            r#"
            status_bar {
                animation_speed 100
            }
            "#,
        ))
        .unwrap();
        assert_eq!(
            layer.status_bar.animation_speed,
            Some(SignedDuration::from_millis(100))
        );
    }

    #[test]
    fn empty_input_sets_nothing() {
        let layer = ConfigLayer::parse(ConfigSource::Raw("")).unwrap();
        assert_debug_snapshot!(layer, @"
        ConfigLayer {
            status_bar: StatusBarConfig {
                animation_speed: None,
            },
            theme: None,
            soft_wrap: None,
        }
        ");
    }

    #[test]
    fn rejects_non_integer_animation_speed() {
        let err = ConfigLayer::parse(ConfigSource::Raw(
            r#"
            status_bar {
                animation_speed "fast"
            }
            "#,
        ))
        .unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Parse(ConfigParseError::InvalidAnimationSpeed { .. })
        ));
    }

    #[test]
    fn parses_soft_wrap() {
        let layer = ConfigLayer::parse(ConfigSource::Raw("soft_wrap #true")).unwrap();
        assert_eq!(layer.soft_wrap, Some(true));
    }

    #[test]
    fn rejects_non_boolean_soft_wrap() {
        let err = ConfigLayer::parse(ConfigSource::Raw(r#"soft_wrap "true""#)).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Parse(ConfigParseError::InvalidSoftWrap { .. })
        ));
    }

    #[test]
    fn soft_wrap_defaults_to_false() {
        assert!(!Config::default().get_soft_wrap());
    }

    #[test]
    fn parses_an_inline_theme() {
        let layer = ConfigLayer::parse(ConfigSource::Raw(
            r#"
            theme {
                ui {
                    status_bar {
                        fg "white"
                        bg "dark_grey"
                    }
                }
            }
            "#,
        ))
        .unwrap();
        let theme = layer.theme.expect("theme was set");
        assert_eq!(
            theme.ui.status_bar,
            Style {
                fg: Some(crossterm::style::Color::White),
                bg: Some(crossterm::style::Color::DarkGrey),
                ..Default::default()
            }
        );
    }

    #[test]
    fn loads_a_theme_from_the_theme_directory_next_to_the_config_file() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("config.kdl"), r#"theme "custom.kdl""#).unwrap();
        std::fs::create_dir(tmp.path().join("theme")).unwrap();
        std::fs::write(
            tmp.path().join("theme/custom.kdl"),
            r#"
            ui {
                popup {
                    fg "white"
                }
            }
            "#,
        )
        .unwrap();

        let layer = ConfigLayer::parse(ConfigSource::Directory(tmp.path())).unwrap();
        let theme = layer.theme.expect("theme was set");
        assert_eq!(theme.ui.popup.fg, Some(crossterm::style::Color::White));
    }

    #[test]
    fn a_bare_name_resolves_to_a_built_in_theme_without_touching_disk() {
        let tmp = tempfile::tempdir().unwrap();
        // No `theme/` subdirectory here: if the built-in lookup were skipped,
        // falling through to path resolution would fail to find the file.
        std::fs::write(tmp.path().join("config.kdl"), r#"theme "monokai""#).unwrap();

        let layer = ConfigLayer::parse(ConfigSource::Directory(tmp.path())).unwrap();
        let theme = layer.theme.expect("theme was set");
        assert_eq!(theme, Theme::built_in("monokai").unwrap());
    }

    #[test]
    fn reports_a_missing_theme_file() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("config.kdl"),
            r#"theme "does_not_exist.kdl""#,
        )
        .unwrap();

        let err = ConfigLayer::parse(ConfigSource::Directory(tmp.path())).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Parse(ConfigParseError::ThemeFileIo { .. })
        ));
    }

    #[test]
    fn a_bare_theme_node_with_no_path_and_no_body_defaults_to_an_empty_theme() {
        let layer = ConfigLayer::parse(ConfigSource::Raw("theme")).unwrap();
        assert_eq!(layer.theme, Some(Theme::default()));
    }

    #[test]
    fn a_path_like_theme_from_the_default_config_reports_a_default_placeholder() {
        // The bundled default configuration has no real directory to
        // resolve a `theme "path"` against, so this must fail clearly
        // instead of silently resolving against the process's cwd.
        let err =
            ConfigLayer::parse(ConfigSource::Raw(r#"theme "not_a_built_in.kdl""#)).unwrap_err();
        let ConfigError::Parse(ConfigParseError::ThemeFileIo { path, .. }) = err else {
            panic!("expected a ThemeFileIo error, got {err:?}");
        };
        assert_eq!(path, "*default*/theme/not_a_built_in.kdl");
    }

    #[test]
    fn loading_a_directory_with_no_config_kdl_is_an_empty_layer_not_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let layer = ConfigLayer::parse(ConfigSource::Directory(tmp.path())).unwrap();
        assert_eq!(layer, ConfigLayer::default());
    }

    #[test]
    fn load_raw_never_touches_disk() {
        let config = Config::default();
        let forked = config
            .load_raw(r#"status_bar { animation_speed 42 }"#)
            .unwrap();
        assert_eq!(
            forked.get_status_bar_animation_speed(),
            SignedDuration::from_millis(42)
        );
    }

    #[test]
    fn find_git_root_locates_enclosing_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path().join("repo");
        let nested = repo_root.join("src").join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir(repo_root.join(".git")).unwrap();

        assert_eq!(
            find_git_root(&nested.join("file.rs")),
            Some(repo_root.clone())
        );
        assert_eq!(find_git_root(&repo_root), Some(repo_root));
    }

    #[test]
    fn find_git_root_returns_none_without_a_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("a").join("b");
        fs::create_dir_all(&nested).unwrap();

        assert_eq!(find_git_root(&nested), None);
    }
}
