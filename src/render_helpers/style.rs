//! Uniforms of window style shaders.
//!
//! A style shader gets a fixed set of built-in uniforms (see `shaders/style_prelude.frag`) and
//! one `uniform float` for each parameter that it declares itself. A parameter is set from the
//! `param` nodes of the style, or to 0 if the style doesn't have it.

use niri_config::{CornerRadius, Style};
use smithay::backend::renderer::gles::{Uniform, UniformName, UniformType};
use smithay::utils::{Logical, Point, Size};

/// Declarations of the built-in uniforms to put in front of the user shader.
pub const PRELUDE: &str = include_str!("shaders/style_prelude.frag");

/// Names and types of the built-in uniforms, in the same order as in the prelude.
const BUILTIN: [(&str, UniformType); 9] = [
    ("u_time", UniformType::_1f),
    ("u_size", UniformType::_2f),
    ("u_pos", UniformType::_2f),
    ("u_focused", UniformType::_1f),
    ("u_radius", UniformType::_4f),
    ("u_palette_0", UniformType::_4f),
    ("u_palette_1", UniformType::_4f),
    ("u_palette_2", UniformType::_4f),
    ("u_palette_3", UniformType::_4f),
];

/// Values of the built-in uniforms that change from frame to frame.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinValues {
    /// Seconds since niri started.
    pub time: f32,
    /// Size of the window geometry.
    pub size: Size<f64, Logical>,
    /// Position of the top-left corner of the window geometry on the output.
    pub pos: Point<f64, Logical>,
    pub focused: bool,
    pub radius: CornerRadius,
}

/// Uniform names to register when compiling a style shader with the given user parameters.
pub fn uniform_names(user_params: &[String]) -> Vec<UniformName<'static>> {
    let builtin = BUILTIN
        .iter()
        .map(|&(name, type_)| UniformName::new(name, type_));
    let user = user_params
        .iter()
        .map(|name| UniformName::new(name.clone(), UniformType::_1f));
    builtin.chain(user).collect()
}

/// Builds all uniform values for drawing a style shader.
///
/// `user_params` must be the names returned by [`user_params`] for the shader, so that the
/// uniforms match the ones registered with [`uniform_names`].
pub fn uniforms(
    style: &Style,
    values: &BuiltinValues,
    user_params: &[String],
) -> Vec<Uniform<'static>> {
    let palette = &style.palette;
    let color = |c: niri_config::Color| c.to_array_unpremul();

    let mut rv = vec![
        Uniform::new("u_time", values.time),
        Uniform::new("u_size", [values.size.w as f32, values.size.h as f32]),
        Uniform::new("u_pos", [values.pos.x as f32, values.pos.y as f32]),
        Uniform::new("u_focused", if values.focused { 1f32 } else { 0f32 }),
        Uniform::new("u_radius", <[f32; 4]>::from(values.radius)),
        Uniform::new("u_palette_0", color(palette.primary)),
        Uniform::new("u_palette_1", color(palette.secondary)),
        Uniform::new("u_palette_2", color(palette.accent)),
        Uniform::new("u_palette_3", color(palette.background)),
    ];

    rv.extend(user_params.iter().map(|name| {
        let value = style.params.get(name).copied().unwrap_or(0.);
        Uniform::new(name.clone(), value as f32)
    }));

    rv
}

/// Returns the names of the user parameters that the shader declares, in order of declaration.
///
/// These are the `uniform float name;` declarations, other than the built-in ones. Declaring
/// several names in one statement works too.
pub fn user_params(source: &str) -> Vec<String> {
    let source = strip_comments(source);

    // Braces end statements too, so that a declaration right after a function body is found.
    let source = source.replace(['{', '}'], ";");

    let mut rv = Vec::new();
    for statement in source.split(';') {
        let statement = statement.replace(',', " , ");
        let mut words = statement.split_whitespace().peekable();

        if words.next() != Some("uniform") {
            continue;
        }
        if matches!(words.peek(), Some(&("lowp" | "mediump" | "highp"))) {
            words.next();
        }
        if words.next() != Some("float") {
            continue;
        }

        for name in words.filter(|&word| word != ",") {
            let reserved = name.starts_with("u_") || name.starts_with("niri_");
            if !is_identifier(name) || reserved || rv.iter().any(|x| x == name) {
                continue;
            }
            rv.push(name.to_owned());
        }
    }

    rv
}

/// Returns the names of the style's parameters that the shader doesn't declare.
pub fn unused_params<'a>(style: &'a Style, user_params: &[String]) -> Vec<&'a str> {
    style
        .params
        .keys()
        .filter(|name| !user_params.contains(name))
        .map(String::as_str)
        .collect()
}

/// Returns whether the shader output changes with time, so it needs redraws.
pub fn is_animated(source: &str) -> bool {
    let source = strip_comments(source);

    source.match_indices("u_time").any(|(idx, name)| {
        let before = source[..idx].chars().next_back();
        let after = source[idx + name.len()..].chars().next();
        !before.is_some_and(is_identifier_char) && !after.is_some_and(is_identifier_char)
    })
}

fn is_identifier_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(is_identifier_char)
}

fn strip_comments(source: &str) -> String {
    let mut rv = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();

    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('/', Some('/')) => {
                // Keep the newline.
                while chars.next_if(|&c| c != '\n').is_some() {}
            }
            ('/', Some('*')) => {
                chars.next();
                let mut prev = ' ';
                for c in chars.by_ref() {
                    if prev == '*' && c == '/' {
                        break;
                    }
                    prev = c;
                }
                // A comment separates tokens.
                rv.push(' ');
            }
            _ => rv.push(c),
        }
    }

    rv
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(source: &str) -> Vec<String> {
        user_params(source)
    }

    #[test]
    fn builtins_match_prelude() {
        for (name, type_) in BUILTIN {
            let decl = match type_ {
                UniformType::_1f => format!("uniform float {name};"),
                UniformType::_2f => format!("uniform vec2 {name};"),
                UniformType::_4f => format!("uniform vec4 {name};"),
                _ => unreachable!(),
            };
            assert!(PRELUDE.contains(&decl), "prelude is missing `{decl}`");
        }

        let declared = PRELUDE
            .lines()
            .filter(|line| line.starts_with("uniform "))
            .count();
        assert_eq!(declared, BUILTIN.len());
    }

    #[test]
    fn prelude_declares_no_user_params() {
        assert_eq!(names(PRELUDE), Vec::<String>::new());
    }

    #[test]
    fn finds_user_params() {
        let source = "
            precision highp float;
            uniform float glow;
            uniform highp float blur_radius; // trailing comment
            uniform lowp float a, b ,c;
            uniform vec2 not_a_float;
            uniform float arr[4];
            vec4 style_color(vec2 px, vec2 size) { return vec4(0.0); }
            uniform float after_function;
        ";
        assert_eq!(
            names(source),
            ["glow", "blur_radius", "a", "b", "c", "after_function"]
        );
    }

    #[test]
    fn ignores_comments_builtins_and_duplicates() {
        let source = "
            // uniform float commented;
            /* uniform float block; */
            uniform float /* inline */ real;
            uniform float u_time;
            uniform float niri_alpha;
            uniform float real;
        ";
        assert_eq!(names(source), ["real"]);
    }

    #[test]
    fn detects_animated_shaders() {
        assert!(is_animated("float t = u_time * 2.0;"));
        assert!(is_animated("sin(u_time)"));
        assert!(!is_animated("// u_time is not used\nfloat t = 1.0;"));
        assert!(!is_animated("/* u_time */ float t = 1.0;"));
        assert!(!is_animated("float u_time_scale = 1.0; float my_u_time;"));
        assert!(!is_animated(""));
    }

    #[test]
    fn unused_params_are_reported() {
        let mut style = Style::default();
        style.params.insert("glow".to_owned(), 1.);
        style.params.insert("typo".to_owned(), 2.);

        let declared = names("uniform float glow; uniform float other;");
        assert_eq!(unused_params(&style, &declared), ["typo"]);
    }

    #[test]
    fn uniforms_match_registered_names() {
        let mut style = Style::default();
        style.params.insert("glow".to_owned(), 0.5);

        let declared = names("uniform float glow; uniform float missing;");
        let values = BuiltinValues {
            time: 1.5,
            size: Size::from((100., 50.)),
            pos: Point::from((10., 20.)),
            focused: true,
            radius: CornerRadius::default(),
        };

        let registered = uniform_names(&declared);
        let built = uniforms(&style, &values, &declared);

        let registered: Vec<_> = registered.iter().map(|x| x.name.to_string()).collect();
        let built: Vec<_> = built.iter().map(|x| x.name.to_string()).collect();
        assert_eq!(registered, built);
        assert_eq!(built.len(), BUILTIN.len() + 2);
        assert_eq!(&built[BUILTIN.len()..], ["glow", "missing"]);
    }
}
