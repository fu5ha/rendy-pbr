#version 450
#extension GL_ARB_separate_shader_objects : enable

layout(location = 0) in vec3 f_pos;

layout(set = 0, binding = 1) uniform sampler cube_sampler;
layout(set = 0, binding = 2) uniform textureCube cube_map;

layout(location = 0) out vec4 color;

void main() {
    vec3 col = texture(samplerCube(cube_map, cube_sampler), f_pos, 1.2).rgb;
    color = vec4(col, 1.0);
}
