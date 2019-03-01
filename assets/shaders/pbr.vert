#version 450
#extension GL_ARB_separate_shader_objects : enable

layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec3 a_norm;
layout(location = 2) in vec2 a_uv;
// vec4[4] is used instead of mat4 due to spirv-cross bug for dx12 backend
layout(location = 3) in vec4 model[4]; // per-instance.

layout(std140, set = 0, binding = 0) uniform Args {
    mat4 proj;
    mat4 view;
    vec3 camera_pos;
};

layout(location = 0) out vec4 frag_world_pos;
layout(location = 1) out vec3 frag_norm;
layout(location = 2) out vec2 frag_uv;
layout(location = 3) out vec3 view_vec;

void main() {
    mat4 model_mat = mat4(model[0], model[1], model[2], model[3]);
    frag_uv = a_uv;
    frag_norm = normalize((vec4(a_norm, 0.0) * model_mat).xyz);
    frag_world_pos = model_mat * vec4(a_pos, 1.0);
    view_vec = camera_pos - frag_world_pos.xyz;
    gl_Position = proj * view * frag_world_pos;
}