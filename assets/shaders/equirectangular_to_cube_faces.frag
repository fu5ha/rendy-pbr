#version 450
#extension GL_ARB_separate_shader_objects : enable

layout(location = 0) in vec3 f_pos;
layout(location = 1) flat in int face_index;

layout(set = 0, binding = 1) uniform sampler equirectangular_sampler;
layout(set = 0, binding = 2) uniform texture2D equirectangular_texture;

const vec2 invAtan = vec2(0.1591, 0.3183);
vec2 SampleSphericalMap(vec3 v)
{
    vec2 uv = vec2(atan(v.z, v.x), asin(v.y));
    uv *= invAtan;
    uv += 0.5;
    return uv;
}

layout(location = 0) out vec4 color[6];

void main() {
    vec2 uv = SampleSphericalMap(normalize(f_pos));
    vec3 col = texture(sampler2D(equirectangular_sampler, equirectangular_texture), uv);
    color[face_index] = vec4(col, 1.0);
}
