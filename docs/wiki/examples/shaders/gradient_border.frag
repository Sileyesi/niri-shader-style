// Gradient border from the palette, with a soft drop shadow.
//
// The border goes from the primary to the secondary to the accent color along the diagonal of the
// window. It is dimmed when the window isn't focused.
//
// window-rule {
//     style {
//         palette { primary "#89b4fa"; secondary "#cba6f7"; accent "#f38ba8"; }
//         shader "~/.config/niri/shaders/gradient_border.frag"
//         corner-radius 14
//         param "width" 3.0   // Border width in logical pixels.
//         param "shadow" 0.4  // Drop shadow opacity.
//     }
// }

uniform float width;
uniform float shadow;

vec4 style_color(vec2 px, vec2 size) {
    float d = u_window_sdf(px);

    // The border is a band just outside the window geometry.
    float band = smoothstep(-0.5, 0.5, d) * (1.0 - smoothstep(width - 0.5, width + 0.5, d));

    float t = clamp((px.x / size.x + px.y / size.y) * 0.5, 0.0, 1.0);
    vec3 color = t < 0.5
        ? mix(u_palette(U_PRIMARY).rgb, u_palette(U_SECONDARY).rgb, t * 2.0)
        : mix(u_palette(U_SECONDARY).rgb, u_palette(U_ACCENT).rgb, t * 2.0 - 1.0);
    float focus = mix(0.45, 1.0, u_focused);
    vec4 border = u_premul(vec4(color, band * focus));

    // Shadow is the window shape pushed down, blurred by distance.
    float shadow_d = u_window_sdf(px - vec2(0.0, 6.0));
    float shade = (1.0 - smoothstep(-8.0, 14.0, shadow_d)) * shadow;
    vec4 drop = vec4(0.0, 0.0, 0.0, shade * step(0.0, d));

    return u_over(border, drop);
}
