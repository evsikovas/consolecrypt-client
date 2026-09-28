// ConsoleCrypt — Liquid Glass edge refraction (LIQUID_GLASS_SPEC §6.5).
//
// Used only through ImageFilter.shader on a BackdropFilter, only on macOS
// with Impeller, only for live transient overlays and the live toolbar
// group. Everything else falls back to frosted / static glass.
//
// The centre of the surface passes through untouched; inside the bezel the
// backdrop is sampled further inside along the SDF normal (a lens edge),
// with optional chromatic aberration and a small specular lift towards the
// top-left light source. Output stays premultiplied.

#version 460 core

#include <flutter/runtime_effect.glsl>

precision mediump float;

// Bound by the engine: size of the filter input texture in physical px.
uniform vec2 u_size;
// Surface rect in physical px: x, y, width, height (window space).
uniform vec4 u_rect;
// Corner radius, bezel width, maximum displacement, chroma offset (px).
uniform float u_radius;
uniform float u_bezel;
uniform float u_displacement;
uniform float u_chroma;
// Specular strength, 0..1.
uniform float u_specular;

// Bound by the engine: the (blurred, saturated) backdrop.
uniform sampler2D u_texture;

out vec4 frag_color;

float roundedRectSdf(vec2 p, vec2 halfSize, float r) {
  vec2 q = abs(p) - halfSize + vec2(r);
  return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

vec4 sampleAt(vec2 px) {
  vec2 uv = clamp(px / u_size, vec2(0.0), vec2(1.0));
#ifdef IMPELLER_TARGET_OPENGLES
  uv.y = 1.0 - uv.y;
#endif
  return texture(u_texture, uv);
}

void main() {
  vec2 frag = FlutterFragCoord().xy;

  // The input is either the whole backdrop (window space) or exactly the
  // filter bounds; detect the latter by its size so both work.
  bool local = abs(u_size.x - u_rect.z) < 2.0 && abs(u_size.y - u_rect.w) < 2.0;
  vec2 origin = local ? vec2(0.0) : u_rect.xy;
  vec2 halfSize = u_rect.zw * 0.5;
  vec2 p = frag - origin - halfSize;
  float r = min(u_radius, min(halfSize.x, halfSize.y));
  float d = roundedRectSdf(p, halfSize, r);

  vec4 color = sampleAt(frag);
  if (u_bezel > 0.0 && d < 0.0 && d > -u_bezel) {
    // Outward normal from the SDF gradient (central differences).
    float e = 0.5;
    vec2 n = vec2(
      roundedRectSdf(p + vec2(e, 0.0), halfSize, r) - roundedRectSdf(p - vec2(e, 0.0), halfSize, r),
      roundedRectSdf(p + vec2(0.0, e), halfSize, r) - roundedRectSdf(p - vec2(0.0, e), halfSize, r)
    );
    n = n / max(length(n), 0.0001);

    // 0 at the inner edge of the bezel, 1 at the rim.
    float t = 1.0 - (-d / u_bezel);
    vec2 src = frag - n * (u_displacement * t * t);

    if (u_chroma > 0.0) {
      vec2 c = n * (u_chroma * t);
      vec4 g = sampleAt(src);
      color = vec4(sampleAt(src + c).r, g.g, sampleAt(src - c).b, g.a);
    } else {
      color = sampleAt(src);
    }

    float light = max(dot(n, vec2(-0.70710678, -0.70710678)), 0.0);
    float spec = u_specular * light * light * light * t * t;
    color.rgb = min(color.rgb + vec3(spec * color.a), vec3(color.a));
  }
  frag_color = color;
}
