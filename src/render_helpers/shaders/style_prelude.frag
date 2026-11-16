// Built-in uniforms of window style shaders.
//
// Every uniform here is set by niri. Parameters from the `param` nodes of the style are set from the
// `uniform float` declarations found in the shader itself.

// Seconds since niri started. Only changes when the shader mentions `u_time`.
uniform float u_time;
// Size of the window geometry in logical pixels.
uniform vec2 u_size;
// Position of the top-left corner of the window geometry on the output, in logical pixels.
uniform vec2 u_pos;
// 1.0 if the window is focused, 0.0 otherwise.
uniform float u_focused;
// Corner radii of the window geometry in logical pixels: top-left, top-right, bottom-right,
// bottom-left.
uniform vec4 u_radius;
// Position of the pointer relative to the window's top-left corner, in logical pixels, like `px` in
// `style_color()`. Keeps the last position after the pointer leaves the window.
uniform vec2 u_pointer;
// 1.0 while the pointer is over the window, 0.0 when it isn't, eased over a short time.
uniform float u_pointer_amount;

// Palette colors, straight (not premultiplied) alpha. Use `u_palette(i)` to index them.
uniform vec4 u_palette_0;
uniform vec4 u_palette_1;
uniform vec4 u_palette_2;
uniform vec4 u_palette_3;

#define U_PRIMARY 0
#define U_SECONDARY 1
#define U_ACCENT 2
#define U_BACKGROUND 3

vec4 u_palette(int i) {
    if (i == 0)
        return u_palette_0;
    if (i == 1)
        return u_palette_1;
    if (i == 2)
        return u_palette_2;
    return u_palette_3;
}

// Premultiplies a straight alpha color, which is what `style_color()` should return.
vec4 u_premul(vec4 color) {
    return vec4(color.rgb * color.a, color.a);
}

// Signed distance from `px` to the rounded edge of the window geometry, in logical pixels.
//
// Negative inside the window, positive outside. `px` is relative to the window's top-left corner,
// like in `style_color()`.
float u_window_sdf(vec2 px) {
    vec2 center = px - u_size * 0.5;

    // Pick the corner radius of the quadrant that `px` is in.
    float radius;
    if (center.x < 0.0)
        radius = center.y < 0.0 ? u_radius.x : u_radius.w;
    else
        radius = center.y < 0.0 ? u_radius.y : u_radius.z;

    vec2 q = abs(center) - u_size * 0.5 + radius;
    return min(max(q.x, q.y), 0.0) + length(max(q, 0.0)) - radius;
}

// Draws premultiplied color `top` over premultiplied color `bottom`.
vec4 u_over(vec4 top, vec4 bottom) {
    return top + bottom * (1.0 - top.a);
}

// Color of what is behind the window at `px`, relative to the window's top-left corner, like in
// `style_color()`. Straight alpha. Points outside of the drawn area get the nearest edge color.
//
// A shader that uses this gets the backdrop captured every frame, which costs some performance.
vec4 u_backdrop_at(vec2 px) {
    vec3 uv = niri_backdrop_matrix * vec3(px + niri_origin, 1.0);
    return texture2D(u_backdrop, clamp(uv.xy, 0.001, 0.999));
}
