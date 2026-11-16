

void main() {
    vec2 px = niri_v_coords * niri_size / niri_scale - niri_origin;
    vec4 color = style_color(px, u_size);

    color = color * niri_alpha;

#if defined(DEBUG_FLAGS)
    if (niri_tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
