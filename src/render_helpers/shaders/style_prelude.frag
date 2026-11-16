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
