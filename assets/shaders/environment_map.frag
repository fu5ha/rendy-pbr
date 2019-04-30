#version 450
#extension GL_ARB_separate_shader_objects : enable

layout(location = 0) in vec3 f_pos;

layout(std140, set = 0, binding = 0) uniform UniformArgs {
    mat4 proj;
    mat4 view;
    float roughness;
};

layout(set = 1, binding = 0) uniform sampler cube_sampler;
layout(set = 1, binding = 1) uniform textureCube cube_map;

layout(location = 0) out vec4 color;

void main() {
    vec3 col = textureLod(samplerCube(cube_map, cube_sampler), f_pos, roughness).rgb;
    color = vec4(col, 1.0);
}
