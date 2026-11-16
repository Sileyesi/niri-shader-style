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

// What is behind the window, and the matrix to find a point of the drawn area in it.
uniform sampler2D u_backdrop;
uniform mat3 niri_backdrop_matrix;

