#version 450
#extension GL_ARB_separate_shader_objects : enable

layout(location = 0) in vec3 f_pos;
layout(location = 1) flat in int face_index;

layout(set = 0, binding = 0) uniform sampler equirectangular_sampler;
layout(set = 0, binding = 1) uniform texture2D equirectangular_texture;

// Converts from [-Pi, Pi] on X to [-0.5, 0.5], and [-Pi/2, Pi/2] on Y to [-0.5, 0.5]
const vec2 normalize_spherical_coords = vec2(0.1591, 0.3183);
vec2 SampleSphericalMap(vec3 v)
{
    vec2 uv = vec2(atan(v.x, v.z), asin(-v.y));
    uv *= normalize_spherical_coords;
    uv += 0.5;
    return uv;
}

layout(location = 0) out vec4 color;

void main() {
    vec3 pos = f_pos;
    vec2 uv = SampleSphericalMap(normalize(pos));
    vec3 col = texture(sampler2D(equirectangular_texture, equirectangular_sampler), uv).rgb;
    color = vec4(col, 1.0);
    // color = vec4(pos * 0.5 + vec3(0.5), 1.0);
}
