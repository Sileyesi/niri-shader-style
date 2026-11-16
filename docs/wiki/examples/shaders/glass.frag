// Frosted glass window.
//
// Draws a tinted, lit pane under the window and a thin highlight on its edge. For the frosted
// look, use a semitransparent window and blur what is behind it, see the config below.
//
// window-rule {
//     match app-id="firefox"
//     opacity 0.85
//     clip-to-geometry true
//     background-effect { blur true; }
//     style {
//         palette { primary "#cdd6f4"; secondary "#89b4fa"; background "#1e1e2e"; }
//         shader "~/.config/niri/shaders/glass.frag"
//         corner-radius 16
//         param "tint" 0.5   // How strongly the pane is tinted with the background color.
//         param "glow" 0.35  // Brightness of the glow around the window.
//     }
// }

uniform float tint;
uniform float glow;

vec4 style_color(vec2 px, vec2 size) {
    float d = u_window_sdf(px);
    float inside = 1.0 - smoothstep(-0.5, 0.5, d);

    // Light comes from the top-left corner.
    float lit = 1.0 - clamp((px.x / size.x + px.y / size.y) * 0.5, 0.0, 1.0);

    vec4 background = u_palette(U_BACKGROUND);
    vec3 pane = background.rgb + vec3(0.10) * lit;
    vec4 fill = u_premul(vec4(pane, background.a * tint)) * inside;

    // One pixel highlight on the inside of the edge, brighter towards the light.
    float rim = smoothstep(-2.0, -0.5, d) * inside;
    vec3 rim_color = mix(u_palette(U_SECONDARY).rgb, u_palette(U_PRIMARY).rgb, lit);
    vec4 edge = u_premul(vec4(rim_color, rim * (0.25 + 0.5 * lit)));

    // Soft glow outside, dimmer when the window isn't focused.
    float outside = exp(-max(d, 0.0) / 10.0) * (1.0 - inside);
    float focus = mix(0.5, 1.0, u_focused);
    vec4 halo = u_premul(vec4(u_palette(U_SECONDARY).rgb, outside * glow * focus));

    return u_over(edge, u_over(fill, halo));
}
