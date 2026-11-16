// Neon border that pulses.
//
// A bright line around the window with a glow that fades out into the margin of the style. The
// pulse is faster and stronger on the focused window.
//
// window-rule {
//     style {
//         palette { primary "#00f0ff"; accent "#ff2bd6"; }
//         shader "~/.config/niri/shaders/neon.frag"
//         corner-radius 12
//         margin 40          // Room for the glow.
//         param "width" 2.5  // Line width in logical pixels.
//         param "speed" 2.0  // Pulses per 2π seconds.
//     }
// }
//
// Add `clip-to-geometry true` to the rule to round the corners of windows that don't do it themselves.

uniform float width;
uniform float speed;

vec4 style_color(vec2 px, vec2 size) {
    float d = u_window_sdf(px);

    // Distance from the center line of the neon tube, which sits just outside the window.
    float line_d = abs(d - width * 0.5);
    float line = 1.0 - smoothstep(width * 0.5 - 0.5, width * 0.5 + 0.5, line_d);

    float pulse = 0.5 + 0.5 * sin(u_time * speed);
    float strength = mix(0.55, 0.75 + 0.25 * pulse, u_focused);

    // Color shifts between the two palette colors along the diagonal.
    float t = clamp((px.x / size.x + px.y / size.y) * 0.5, 0.0, 1.0);
    vec3 color = mix(u_palette(U_PRIMARY).rgb, u_palette(U_ACCENT).rgb, t);

    float glow = exp(-max(d, 0.0) / 9.0) * 0.55;
    float outside = step(0.0, d);

    float alpha = (line + glow * outside) * strength;
    // The core of the line is whiter, like a real tube.
    vec3 hot = mix(color, vec3(1.0), line * 0.6);

    return u_premul(vec4(hot, clamp(alpha, 0.0, 1.0)));
}
