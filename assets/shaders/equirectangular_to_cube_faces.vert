#version 450
#extension GL_ARB_separate_shader_objects : enable

layout(location = 0) in vec3 a_pos;

layout(std140, set = 0, binding = 0) uniform UniformArgs {
    mat4 proj;
    mat4 view[6];
};

layout (push_constant) uniform PushConsts {
    int side;
};

layout(location = 0) out vec3 f_pos;
layout(location = 1) flat out int vertex_index;

void main() {
    f_pos = a_pos;
    face_index = gl_VertexIndex / 6;
    gl_Position = proj * view[face_index] * vec4(f_pos, 1.0);
}