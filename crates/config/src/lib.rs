use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};

use config_macros::ConfigField;
use jiff::SignedDuration;
use parking_lot::RwLock;
use thiserror::Error;

/// The defaults configuration. Forms the root of every [`ConfigLayer`] chain.
const DEFAULT_CONFIG_KDL: &str = include_str!("../../../default_configuration.kdl");

/// The name looked for in the directory of the file being edited, and at the
/// root of the project.
const LOCAL_CONFIG_FILE_NAME: &str = ".editor/config.kdl";

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
}

impl ConfigLayer {
    pub fn parse(input: &str) -> Result<Self, ConfigParseError> {
        let document: kdl::KdlDocument = input.parse()?;
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

        Ok(config)
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
    /// Falls back to an empty layer if the file doesn't exist.
    pub fn load(&self, path: &Path) -> Result<Config, ConfigError> {
        let config = match fs::read_to_string(path) {
            Ok(content) => ConfigLayer::parse(&content)?,
            // We still need to initialize the conf in case it get set at runtime.
            Err(err) if err.kind() == io::ErrorKind::NotFound => ConfigLayer::default(),
            Err(err) => return Err(err.into()),
        };
        Ok(self.load_layer(config))
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
            ConfigLayer::parse(DEFAULT_CONFIG_KDL)
                .expect("bundled default_configuration.kdl must be valid KDL")
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
                .expect("default_configuration.kdl must set every field");
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

    if let Some(global_path) = global_config_path() {
        chain = load_and_log(chain, &global_path);
    }

    if let Some(buffer_path) = buffer_path {
        if let Some(repo_root) = find_git_root(buffer_path) {
            chain = load_and_log(chain, &repo_root.join(LOCAL_CONFIG_FILE_NAME));
        }
        if let Some(dir) = buffer_path.parent() {
            chain = load_and_log(chain, &dir.join(LOCAL_CONFIG_FILE_NAME));
        }
    }

    chain
}

fn load_and_log(parent: Config, path: &Path) -> Config {
    match parent.load(path) {
        Ok(layer) => layer,
        Err(err) => {
            log::error!("failed to load config file {}: {err}", path.display());
            parent
        }
    }
}

fn global_config_path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var("HOME").ok()?).join(".config/editor/config.kdl"))
}

#[cfg(test)]
mod test {
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
        let layer = ConfigLayer::parse(
            r#"
            status_bar {
                animation_speed 100
            }
            "#,
        )
        .unwrap();
        assert_eq!(
            layer.status_bar.animation_speed,
            Some(SignedDuration::from_millis(100))
        );
    }

    #[test]
    fn empty_input_sets_nothing() {
        let layer = ConfigLayer::parse("").unwrap();
        assert_eq!(layer.status_bar.animation_speed, None);
    }

    #[test]
    fn rejects_non_integer_animation_speed() {
        let err = ConfigLayer::parse(
            r#"
            status_bar {
                animation_speed "fast"
            }
            "#,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            ConfigParseError::InvalidAnimationSpeed { .. }
        ));
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
