// Liquid glass: the window edge bends what is behind it, like a thick lens.
//
// Inspired by the "Liquid Terminal" look from ShojiWM. This is a new implementation for niri's
// `u_backdrop_at()`, which gives a style shader the pixels behind the window.
//
// The glass is drawn under the window, so the window itself should be semitransparent, for
// example a terminal with `background_opacity 0.3`.
//
// window-rule {
//     match app-id="kitty"
//     clip-to-geometry true
//     style {
//         palette { primary "#ffffff"; }
//         shader "~/.config/niri/shaders/liquid_glass.frag"
//         corner-radius 28
//         param "depth" 30      // Width of the bending rim, in logical pixels.
//         param "strength" 1.0  // How far the rim bends the backdrop.
//         param "chroma" 3.0    // Color fringing at the edge, in logical pixels.
//         param "tint" 1.0      // Brightness of the glass.
//     }
// }
//
// Note: the backdrop isn't available while the window is opening or closing, so the glass shows up
// only after the animation.

uniform float depth;
uniform float strength;
uniform float chroma;
uniform float tint;

// Direction pointing out of the window at `px`.
vec2 outward(vec2 px) {
    vec2 grad = vec2(
        u_window_sdf(px + vec2(1.0, 0.0)) - u_window_sdf(px - vec2(1.0, 0.0)),
        u_window_sdf(px + vec2(0.0, 1.0)) - u_window_sdf(px - vec2(0.0, 1.0))
    );
    return grad / max(length(grad), 0.0001);
}

vec4 style_color(vec2 px, vec2 size) {
    float d = u_window_sdf(px);
    float inside = 1.0 - smoothstep(-0.5, 0.5, d);

    // Soft shadow around the glass.
    float shade = exp(-max(d, 0.0) / 14.0) * 0.4 * (1.0 - inside);
    if (inside <= 0.0)
        return vec4(0.0, 0.0, 0.0, shade);

    vec2 dir = outward(px);

    // 1.0 at the edge, 0.0 at `depth` inside. The lens bends the strongest at the very edge.
    float t = 1.0 - clamp(-d / depth, 0.0, 1.0);
    float bend = 1.0 - sqrt(1.0 - t * t);

    // Sample from further inside the window, so the edge pulls in the content.
    vec2 offset = dir * bend * depth * strength;
    vec2 shift = dir * t * chroma;
    vec2 at = px - offset;
    vec3 color = vec3(
        u_backdrop_at(at - shift).r,
        u_backdrop_at(at).g,
        u_backdrop_at(at + shift).b
    );
    color *= tint;

    // Light from the top-left corner: bright edge on that side, faint on the opposite one.
    float light = max(dot(dir, normalize(vec2(-1.0, -1.0))), 0.0);
    float rim = smoothstep(0.75, 1.0, t);
    color += u_palette(U_PRIMARY).rgb * rim * (0.12 + 0.5 * light);

    // A thin bright line right on the edge.
    float line = smoothstep(-1.5, -0.5, d) * inside;
    color += u_palette(U_PRIMARY).rgb * line * (0.15 + 0.35 * light);

    return vec4(color * inside, inside);
}
