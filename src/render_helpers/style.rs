//! Window style shaders.
//!
//! A style shader defines `vec4 style_color(vec2 px, vec2 size)`, which returns the premultiplied
//! color for the pixel at `px` (logical pixels from the top-left corner of the window geometry,
//! negative outside of it) of a window with the given geometry `size`. It is drawn in an area
//! around the window geometry, behind the window, so it can paint the background, border, glow and
//! shadow in one pass.
//!
//! The shader gets a fixed set of built-in uniforms (see `shaders/style_prelude.frag`) and one
//! `uniform float` for each parameter that it declares itself. A parameter is set from the `param`
//! nodes of the style, or stays 0 if the style doesn't have it.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use niri_config::style::strip_comments;
use niri_config::{CornerRadius, Palette, Style};
use smithay::backend::renderer::element::{Element, Id, Kind, RenderElement, UnderlyingStorage};
use smithay::backend::renderer::gles::{
    GlesError, GlesFrame, GlesRenderer, Uniform, UniformName, UniformType,
};
use smithay::backend::renderer::utils::{CommitCounter, DamageSet, OpaqueRegions};
use smithay::gpu_span_location;
use smithay::utils::user_data::UserDataMap;
use smithay::utils::{Buffer, Logical, Physical, Point, Rectangle, Scale, Size, Transform};

use super::renderer::AsGlesFrame as _;
use super::shader_element::{ShaderProgram, ShaderRenderElement};
use super::shaders::{ProgramType, Shaders};
use crate::backend::tty::{TtyFrame, TtyRenderer, TtyRendererError};

/// Declarations that every style shader starts with.
const HEADER: &str = include_str!("shaders/style_header.frag");
/// Declarations of the built-in uniforms and helper functions.
pub const PRELUDE: &str = include_str!("shaders/style_prelude.frag");
/// The `main()` function that calls the user's `style_color()`.
const EPILOGUE: &str = include_str!("shaders/style_epilogue.frag");

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

/// Uniforms that niri sets for its own use, declared in the header.
const INTERNAL: [(&str, UniformType); 1] = [("niri_origin", UniformType::_2f)];

/// Seconds after which `u_time` wraps around, to keep it precise as a `float`.
const TIME_WRAP_SECS: f64 = 3600.;

/// Values of the built-in uniforms that change from frame to frame.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinValues {
    /// Time since an arbitrary point, which wraps around every hour.
    pub time: f32,
    /// Size of the window geometry.
    pub size: Size<f64, Logical>,
    /// Position of the top-left corner of the window geometry in the workspace view.
    pub pos: Point<f64, Logical>,
    pub focused: bool,
    pub radius: CornerRadius,
}

/// Programs of the style shaders, compiled on demand.
///
/// The programs are looked up by the hash of the shader path, so that a changed file replaces the
/// previous program.
#[derive(Debug, Default)]
pub struct StylePrograms {
    entries: HashMap<u64, Entry>,
}

#[derive(Debug)]
struct Entry {
    /// Hash of the source that was compiled (or tried to be compiled) last.
    tried_hash: u64,
    /// The last program that compiled successfully.
    ///
    /// If the shader file changes to something that doesn't compile, this stays as is.
    good: Option<ShaderProgram>,
}

impl StylePrograms {
    pub fn get(&self, key: u64) -> Option<ShaderProgram> {
        self.entries.get(&key)?.good.clone()
    }
}

/// Makes sure that the program for the style's shader is compiled, and returns whether it is
/// usable.
///
/// Programs are cached by the shader path and compiled again only when the source changes. If a
/// new version fails to compile, the previous working version stays in use.
pub fn ensure_program(renderer: &mut GlesRenderer, style: &Style) -> bool {
    let Some(shader) = &style.shader else {
        return false;
    };
    // Failing to read the file has been logged when loading the config.
    let Ok(source) = &shader.source else {
        return false;
    };

    if let Some(entry) = Shaders::get(renderer)
        .styles
        .borrow()
        .entries
        .get(&shader.key)
    {
        if entry.tried_hash == shader.hash {
            return entry.good.is_some();
        }
    }

    let _span = tracy_client::span!("style::ensure_program");

    let user_params = user_params(source);
    let result = compile(renderer, source, &user_params);

    let mut old = None;
    let usable = match result {
        Ok(program) => {
            for name in unused_params(style, &user_params) {
                warn!(
                    "style shader {:?} doesn't declare `uniform float {name};` \
                     for the `param \"{name}\"` of the style",
                    shader.path,
                );
            }

            let mut styles = Shaders::get(renderer).styles.borrow_mut();
            let entry = styles.entries.entry(shader.key).or_insert(Entry {
                tried_hash: shader.hash,
                good: None,
            });
            entry.tried_hash = shader.hash;
            old = entry.good.replace(program);
            true
        }
        Err(err) => {
            // The shader source is preceded by our declarations, so line numbers in the compiler
            // output are shifted.
            let offset = HEADER.lines().count() + PRELUDE.lines().count();
            warn!(
                "error compiling style shader {:?}: {err:?} \
                 (line numbers in the log are {offset} higher than in the file)",
                shader.path,
            );

            let mut styles = Shaders::get(renderer).styles.borrow_mut();
            let entry = styles.entries.entry(shader.key).or_insert(Entry {
                tried_hash: shader.hash,
                good: None,
            });
            entry.tried_hash = shader.hash;
            if entry.good.is_some() {
                warn!("keeping the previous working version of the shader");
            } else {
                warn!("windows using it will have the default style");
            }
            entry.good.is_some()
        }
    };

    if let Some(old) = old {
        if let Err(err) = old.destroy(renderer) {
            warn!("error destroying previous style shader: {err:?}");
        }
    }

    usable
}

fn compile(
    renderer: &mut GlesRenderer,
    source: &str,
    user_params: &[String],
) -> Result<ShaderProgram, GlesError> {
    let mut program = String::from(HEADER);
    program.push_str(PRELUDE);
    program.push_str(source);
    program.push_str(EPILOGUE);

    ShaderProgram::compile(renderer, &program, &uniform_names(user_params), &[])
}

/// Uniform names to register when compiling a style shader with the given user parameters.
pub fn uniform_names(user_params: &[String]) -> Vec<UniformName<'static>> {
    let builtin = BUILTIN.iter().chain(&INTERNAL);
    let builtin = builtin.map(|&(name, type_)| UniformName::new(name, type_));
    let user = user_params
        .iter()
        .map(|name| UniformName::new(name.clone(), UniformType::_1f));
    builtin.chain(user).collect()
}

/// Builds all uniform values for drawing a style shader.
///
/// This includes all the params of the style. The ones that the shader doesn't declare are ignored
/// when drawing.
pub fn uniforms(style: &Style, values: &BuiltinValues) -> Vec<Uniform<'static>> {
    let palette = &style.palette;
    let color = |c: niri_config::Color| c.to_array_unpremul();
    let margin = style.margin as f32;

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
        Uniform::new("niri_origin", [margin, margin]),
    ];

    rv.extend(
        style
            .params
            .iter()
            .map(|(name, value)| Uniform::new(name.clone(), *value as f32)),
    );

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

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Renders a window style shader in the area around the window geometry.
#[derive(Debug, Clone)]
pub struct StyleRenderElement {
    inner: ShaderRenderElement,
    params: Option<Parameters>,
}

#[derive(Debug, Clone, PartialEq)]
struct Parameters {
    key: u64,
    margin: f64,
    time: f32,
    size: Size<f64, Logical>,
    pos: Point<f64, Logical>,
    focused: bool,
    radius: CornerRadius,
    palette: Palette,
    params: BTreeMap<String, f64>,
    // Should only be used for visual improvements, i.e. corner radius anti-aliasing.
    scale: f32,
    alpha: f32,
}

impl StyleRenderElement {
    pub fn new() -> Self {
        Self {
            inner: ShaderRenderElement::empty(ProgramType::Style(0), Kind::Unspecified),
            params: None,
        }
    }

    pub fn damage_all(&mut self) {
        self.inner.damage_all();
    }

    /// Updates the element for the style, which must have a shader.
    ///
    /// The area to draw is the window geometry extended by the style's margin on all sides. Put it
    /// in place with [`with_location()`](Self::with_location()).
    pub fn update(&mut self, style: &Style, values: &BuiltinValues, scale: f32, alpha: f32) {
        let Some(shader) = &style.shader else {
            return;
        };

        let params = Parameters {
            key: shader.key,
            margin: style.margin,
            time: values.time,
            size: values.size,
            pos: values.pos,
            focused: values.focused,
            radius: values.radius,
            palette: style.palette,
            params: style.params.clone(),
            scale,
            alpha,
        };
        if self.params.as_ref() == Some(&params) {
            return;
        }

        let size = values.size + Size::from((style.margin, style.margin)).upscale(2.);

        self.inner.set_program(ProgramType::Style(shader.key));
        self.inner.update(
            size,
            None,
            scale,
            alpha,
            Rc::from(uniforms(style, values)),
            HashMap::new(),
        );
        self.params = Some(params);
    }

    /// Margin of the area that the element draws around the window geometry.
    pub fn margin(&self) -> f64 {
        self.params.as_ref().map_or(0., |params| params.margin)
    }

    pub fn with_location(mut self, location: Point<f64, Logical>) -> Self {
        self.inner = self.inner.with_location(location);
        self
    }
}

impl Default for StyleRenderElement {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for StyleRenderElement {
    fn id(&self) -> &Id {
        self.inner.id()
    }

    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
    }

    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.inner.geometry(scale)
    }

    fn transform(&self) -> Transform {
        self.inner.transform()
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        self.inner.src()
    }

    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.inner.damage_since(scale, commit)
    }

    fn opaque_regions(&self, scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        self.inner.opaque_regions(scale)
    }

    fn alpha(&self) -> f32 {
        self.inner.alpha()
    }

    fn kind(&self) -> Kind {
        self.inner.kind()
    }
}

impl RenderElement<GlesRenderer> for StyleRenderElement {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
        cache: Option<&UserDataMap>,
    ) -> Result<(), GlesError> {
        let _span = tracy_client::span!("StyleRenderElement::draw");
        frame.with_gpu_span(gpu_span_location!("StyleRenderElement::draw"), |frame| {
            RenderElement::<GlesRenderer>::draw(
                &self.inner,
                frame,
                src,
                dst,
                damage,
                opaque_regions,
                cache,
            )
        })
    }

    fn underlying_storage(&self, renderer: &mut GlesRenderer) -> Option<UnderlyingStorage<'_>> {
        self.inner.underlying_storage(renderer)
    }
}

impl<'render> RenderElement<TtyRenderer<'render>> for StyleRenderElement {
    fn draw(
        &self,
        frame: &mut TtyFrame<'_, '_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
        cache: Option<&UserDataMap>,
    ) -> Result<(), TtyRendererError<'render>> {
        let frame = frame.as_gles_frame();
        RenderElement::<GlesRenderer>::draw(self, frame, src, dst, damage, opaque_regions, cache)?;
        Ok(())
    }

    fn underlying_storage(
        &self,
        renderer: &mut TtyRenderer<'render>,
    ) -> Option<UnderlyingStorage<'_>> {
        self.inner.underlying_storage(renderer)
    }
}

/// Converts the time from the animation clock to the value for `u_time`.
pub fn time_uniform(time: std::time::Duration) -> f32 {
    (time.as_secs_f64() % TIME_WRAP_SECS) as f32
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
    fn internal_uniforms_match_header() {
        for (name, type_) in INTERNAL {
            let decl = match type_ {
                UniformType::_2f => format!("uniform vec2 {name};"),
                _ => unreachable!(),
            };
            assert!(HEADER.contains(&decl), "header is missing `{decl}`");
        }
    }

    #[test]
    fn prelude_and_header_declare_no_user_params() {
        assert_eq!(names(PRELUDE), Vec::<String>::new());
        assert_eq!(names(HEADER), Vec::<String>::new());
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
    fn unused_params_are_reported() {
        let mut style = Style::default();
        style.params.insert("glow".to_owned(), 1.);
        style.params.insert("typo".to_owned(), 2.);

        let declared = names("uniform float glow; uniform float other;");
        assert_eq!(unused_params(&style, &declared), ["typo"]);
    }

    #[test]
    fn built_uniforms_are_registered() {
        let mut style = Style::default();
        style.params.insert("glow".to_owned(), 0.5);
        style.params.insert("typo".to_owned(), 0.5);

        let declared = names("uniform float glow; uniform float missing;");
        let values = BuiltinValues {
            time: 1.5,
            size: Size::from((100., 50.)),
            pos: Point::from((10., 20.)),
            focused: true,
            radius: CornerRadius::default(),
        };

        let registered: Vec<_> = uniform_names(&declared)
            .iter()
            .map(|x| x.name.to_string())
            .collect();
        let built: Vec<_> = uniforms(&style, &values)
            .iter()
            .map(|x| x.name.to_string())
            .collect();

        // Everything the shader can have is registered, plus the declared params.
        assert_eq!(registered.len(), BUILTIN.len() + INTERNAL.len() + 2);
        assert!(registered.contains(&"glow".to_owned()));
        assert!(registered.contains(&"missing".to_owned()));

        // All uniforms other than the extra params are registered.
        let (fixed, params) = built.split_at(BUILTIN.len() + INTERNAL.len());
        assert!(fixed.iter().all(|name| registered.contains(name)));
        assert_eq!(params, ["glow", "typo"]);
    }

    #[test]
    fn time_wraps() {
        use std::time::Duration;

        assert_eq!(time_uniform(Duration::from_millis(1500)), 1.5);
        assert_eq!(time_uniform(Duration::from_secs(3600 * 5 + 2)), 2.);
    }
}
