#version 450
#extension GL_ARB_separate_shader_objects : enable

layout(location = 0) in vec3 a_pos;

layout(std140, set = 0, binding = 0) uniform UniformArgs {
    mat4 proj;
    mat4 view;
};

layout(location = 0) out vec3 f_pos;

void main() {
    f_pos = a_pos;
    gl_Position = proj * view * vec4(f_pos, 1.0);
}