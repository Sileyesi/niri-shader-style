use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Arc;

use knuffel::errors::DecodeError;
use miette::miette;

use crate::appearance::{Color, CornerRadius};
use crate::utils::{FloatOrInt, MergeWith};
use crate::{BasePath, Includes};

/// Per-window shader style, as written in the `style` block of a window rule.
#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct StyleRule {
    #[knuffel(child, default)]
    pub palette: PaletteRule,
    #[knuffel(child)]
    pub shader: Option<ShaderFile>,
    #[knuffel(child)]
    pub corner_radius: Option<CornerRadius>,
    #[knuffel(children(name = "param"))]
    pub params: Vec<StyleParam>,
}

#[derive(knuffel::Decode, Debug, Default, Clone, Copy, PartialEq)]
pub struct PaletteRule {
    #[knuffel(child)]
    pub primary: Option<Color>,
    #[knuffel(child)]
    pub secondary: Option<Color>,
    #[knuffel(child)]
    pub accent: Option<Color>,
    #[knuffel(child)]
    pub background: Option<Color>,
}

/// A user-defined shader parameter: `param "name" 1.5`.
#[derive(knuffel::Decode, Debug, Clone, PartialEq)]
pub struct StyleParam {
    #[knuffel(argument, str)]
    pub name: ParamName,
    #[knuffel(argument)]
    pub value: FloatOrInt<-1_000_000, 1_000_000>,
}

/// Name of a user-defined shader parameter, which becomes a `uniform float` in the shader.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ParamName(pub String);

/// Shader file referenced by a style.
///
/// A failure to read the file is not a config error: the error is kept here and the window is
/// drawn with the default style instead.
#[derive(Debug, Clone, PartialEq)]
pub struct ShaderFile {
    /// Absolute or config-relative path, with `~` expanded.
    pub path: PathBuf,
    /// Shader source, or a message about why it couldn't be read.
    pub source: Result<Arc<str>, String>,
    /// Hash of the source; together with the path it identifies a compiled program.
    pub hash: u64,
}

/// Style with all matching window rules applied.
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    pub palette: Palette,
    pub shader: Option<ShaderFile>,
    pub params: BTreeMap<String, f64>,
}

/// Resolved palette colors.
///
/// Unset colors fall back to the default niri colors: active, inactive and urgent for primary,
/// secondary and accent, and transparent black for the background.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub primary: Color,
    pub secondary: Color,
    pub accent: Color,
    pub background: Color,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            primary: Color::from_rgba8_unpremul(0x7f, 0xc8, 0xff, 0xff),
            secondary: Color::from_rgba8_unpremul(0x50, 0x50, 0x50, 0xff),
            accent: Color::from_rgba8_unpremul(0x9b, 0x00, 0x00, 0xff),
            background: Color::from_rgba8_unpremul(0, 0, 0, 0),
        }
    }
}

impl Default for Style {
    fn default() -> Self {
        Self {
            palette: Palette::default(),
            shader: None,
            params: BTreeMap::new(),
        }
    }
}

impl MergeWith<StyleRule> for Style {
    fn merge_with(&mut self, part: &StyleRule) {
        let palette = &part.palette;
        if let Some(x) = palette.primary {
            self.palette.primary = x;
        }
        if let Some(x) = palette.secondary {
            self.palette.secondary = x;
        }
        if let Some(x) = palette.accent {
            self.palette.accent = x;
        }
        if let Some(x) = palette.background {
            self.palette.background = x;
        }

        if let Some(x) = &part.shader {
            self.shader = Some(x.clone());
        }

        for param in &part.params {
            self.params.insert(param.name.0.clone(), param.value.0);
        }
    }
}

impl FromStr for ParamName {
    type Err = miette::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut chars = s.chars();
        let valid_start = chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
        if !valid_start || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(miette!(
                "parameter name must be a GLSL identifier: \
                 letters, digits and underscores, not starting with a digit"
            ));
        }

        // The `u_` prefix is taken by the built-in uniforms, and the rest is reserved by niri and
        // GLSL.
        if ["u_", "niri_", "gl_"].iter().any(|p| s.starts_with(p)) || s.contains("__") {
            return Err(miette!(
                "parameter name cannot start with `u_`, `niri_` or `gl_`, or contain `__`"
            ));
        }

        Ok(Self(s.to_owned()))
    }
}

impl ShaderFile {
    pub fn new(path: PathBuf, source: Result<Arc<str>, String>) -> Self {
        let mut hasher = DefaultHasher::new();
        match &source {
            Ok(text) => text.hash(&mut hasher),
            Err(err) => err.hash(&mut hasher),
        }

        Self {
            path,
            source,
            hash: hasher.finish(),
        }
    }

    /// Reads the shader from disk, logging a warning if that fails.
    pub fn load(path: PathBuf) -> Self {
        let source = fs::read_to_string(&path)
            .map(Arc::from)
            .map_err(|err| err.to_string());

        if let Err(err) = &source {
            warn!("error reading style shader {path:?}: {err}; windows will use the default style");
        }

        Self::new(path, source)
    }
}

impl<S> knuffel::Decode<S> for ShaderFile
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        // Check for unexpected type name.
        if let Some(type_name) = &node.type_name {
            ctx.emit_error(DecodeError::unexpected(
                type_name,
                "type name",
                "no type name expected for this node",
            ));
        }

        // Get the first argument.
        let mut iter_args = node.arguments.iter();
        let val = iter_args
            .next()
            .ok_or_else(|| DecodeError::missing(node, "additional argument is required"))?;
        let path: PathBuf = knuffel::traits::DecodeScalar::decode(val, ctx)?;

        // Check for unexpected following arguments, properties and children.
        if let Some(val) = iter_args.next() {
            ctx.emit_error(DecodeError::unexpected(
                &val.literal,
                "argument",
                "unexpected argument",
            ));
        }
        for name in node.properties.keys() {
            ctx.emit_error(DecodeError::unexpected(
                name,
                "property",
                format!("unexpected property `{}`", name.escape_default()),
            ));
        }
        for child in node.children() {
            ctx.emit_error(DecodeError::unexpected(
                child,
                "node",
                format!("unexpected node `{}`", child.node_name.escape_default()),
            ));
        }

        // Expand ~ into the home dir, like includes do.
        let path = if let Ok(rest) = path.strip_prefix("~") {
            let Some(home) = std::env::home_dir() else {
                return Err(DecodeError::missing(
                    node,
                    format!("error retrieving home directory to expand {path:?}"),
                ));
            };

            home.join(rest)
        } else if let Some(base) = ctx.get::<BasePath>() {
            // Relative paths are relative to the file that contains the style.
            base.0.join(path)
        } else {
            path
        };

        // Watch the shader even if it fails to read, so that creating or fixing it reloads the
        // config.
        if let Some(includes) = ctx.get::<Rc<RefCell<Includes>>>() {
            includes.borrow_mut().0.push(path.clone());
        }

        Ok(Self::load(path))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::{Config, WindowRule};

    fn parse_rule(text: &str) -> Result<WindowRule, String> {
        let mut config = Config::parse_mem(text).map_err(|err| format!("{err:?}"))?;
        Ok(config.window_rules.remove(0))
    }

    fn style_of(rule: &WindowRule) -> &StyleRule {
        rule.style.as_ref().unwrap()
    }

    #[test]
    fn parses_style() {
        let rule = parse_rule(
            r##"
            window-rule {
                match app-id="firefox"
                style {
                    palette {
                        primary "#89b4fa"
                        secondary "#cba6f7"
                        accent "#f38ba8"
                        background "#1e1e2e"
                    }
                    shader "/nonexistent/glass.frag"
                    corner-radius 16
                    param "blur_radius" 12.0
                    param "glow" 1
                }
            }
            "##,
        )
        .unwrap();

        let style = style_of(&rule);
        assert_eq!(
            style.palette.primary,
            Some(Color::from_rgba8_unpremul(0x89, 0xb4, 0xfa, 0xff)),
        );
        assert_eq!(
            style.palette.background,
            Some(Color::from_rgba8_unpremul(0x1e, 0x1e, 0x2e, 0xff)),
        );
        assert_eq!(
            style.corner_radius,
            Some(CornerRadius {
                top_left: 16.,
                top_right: 16.,
                bottom_right: 16.,
                bottom_left: 16.,
            })
        );
        assert_eq!(style.params.len(), 2);
        assert_eq!(style.params[0].name.0, "blur_radius");
        assert_eq!(style.params[0].value.0, 12.);
        assert_eq!(style.params[1].value.0, 1.);
    }

    #[test]
    fn rule_without_style_has_none() {
        let rule = parse_rule(r#"window-rule { match app-id="foo"; opacity 0.5; }"#).unwrap();
        assert!(rule.style.is_none());
    }

    #[test]
    fn empty_style_block() {
        let rule = parse_rule(r#"window-rule { style {}; }"#).unwrap();
        assert_eq!(style_of(&rule), &StyleRule::default());
    }

    #[test]
    fn bad_color_is_config_error() {
        let res = parse_rule(r#"window-rule { style { palette { primary "nonsense"; }; }; }"#);
        assert!(res.is_err());
    }

    #[test]
    fn bad_param_names_are_config_errors() {
        for name in ["u_time", "niri_x", "gl_x", "1abc", "a-b", "", "a__b"] {
            let text = format!(r#"window-rule {{ style {{ param "{name}" 1; }}; }}"#);
            assert!(parse_rule(&text).is_err(), "{name:?} should be rejected");
        }
        for name in ["glow", "_x", "blur_radius2", "A"] {
            let text = format!(r#"window-rule {{ style {{ param "{name}" 1; }}; }}"#);
            assert!(parse_rule(&text).is_ok(), "{name:?} should be accepted");
        }
    }

    #[test]
    fn param_requires_number() {
        let err = parse_rule(r#"window-rule { style { param "glow" "high"; }; }"#);
        assert!(err.is_err());
    }

    #[test]
    fn unexpected_nodes_are_errors() {
        assert!(parse_rule(r#"window-rule { style { bogus 1; }; }"#).is_err());
        assert!(parse_rule(r#"window-rule { style { shader "a" "b"; }; }"#).is_err());
    }

    #[test]
    fn missing_shader_file_is_not_a_config_error() {
        let res = Config::parse(
            Path::new("/nonexistent-dir/config.kdl"),
            r#"window-rule { style { shader "shaders/missing.frag"; }; }"#,
        );
        let config = res.config.unwrap();
        let shader = style_of(&config.window_rules[0]).shader.as_ref().unwrap();
        assert_eq!(
            shader.path,
            Path::new("/nonexistent-dir/shaders/missing.frag")
        );
        assert!(shader.source.is_err());

        // The missing file is still watched so that creating it reloads the config.
        assert_eq!(res.includes, [shader.path.clone()]);
    }

    #[test]
    fn loads_shader_relative_to_config() {
        let dir = std::env::temp_dir().join(format!("niri-style-test-{}", std::process::id()));
        fs::create_dir_all(dir.join("shaders")).unwrap();
        fs::write(dir.join("shaders/a.frag"), "// a").unwrap();
        fs::write(dir.join("shaders/b.frag"), "// b").unwrap();

        let text = r#"
            window-rule { style { shader "shaders/a.frag"; }; }
            window-rule { style { shader "shaders/b.frag"; }; }
        "#;
        let res = Config::parse(&dir.join("config.kdl"), text);
        let config = res.config.unwrap();
        let a = style_of(&config.window_rules[0]).shader.clone().unwrap();
        let b = style_of(&config.window_rules[1]).shader.clone().unwrap();
        fs::remove_dir_all(&dir).unwrap();

        assert_eq!(a.source.as_deref(), Ok("// a"));
        assert_eq!(b.source.as_deref(), Ok("// b"));
        assert_ne!(a.hash, b.hash);
        assert_eq!(res.includes, [a.path, b.path]);
    }

    #[test]
    fn merge_overrides_per_field() {
        let rule = |text: &str| parse_rule(text).unwrap();
        let first = rule(
            r##"window-rule { style {
                palette { primary "#ff0000"; accent "#00ff00"; }
                param "a" 1
                param "b" 2
            }; }"##,
        );
        let second = rule(
            r##"window-rule { style {
                palette { primary "#0000ff"; }
                param "b" 3
                param "c" 4
            }; }"##,
        );

        let mut style = Style::default();
        style.merge_with(style_of(&first));
        style.merge_with(style_of(&second));

        assert_eq!(
            style.palette.primary,
            Color::from_rgba8_unpremul(0, 0, 0xff, 0xff)
        );
        assert_eq!(
            style.palette.accent,
            Color::from_rgba8_unpremul(0, 0xff, 0, 0xff)
        );
        // Unset colors keep their defaults.
        assert_eq!(style.palette.secondary, Palette::default().secondary);
        assert_eq!(
            style.params,
            BTreeMap::from([
                ("a".to_owned(), 1.),
                ("b".to_owned(), 3.),
                ("c".to_owned(), 4.)
            ])
        );
    }
}
