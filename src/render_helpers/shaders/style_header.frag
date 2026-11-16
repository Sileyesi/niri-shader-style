precision highp float;

#if defined(DEBUG_FLAGS)
uniform float niri_tint;
#endif

varying vec2 niri_v_coords;
uniform vec2 niri_size;
uniform float niri_alpha;
uniform float niri_scale;
// Offset of the window geometry from the top-left corner of the drawn area, in logical pixels.
uniform vec2 niri_origin;

