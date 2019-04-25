#version 450
#extension GL_ARB_separate_shader_objects : enable

layout(location = 0) in vec3 a_pos;

layout(std140, set = 0, binding = 0) uniform UniformArgs {
    mat4 unproject[6];
};

const vec2[6] verts = {
    vec2(-1.0, 1.0),
    vec2(1.0, 1.0),
    vec2(-1.0, -1.0),
    vec2(-1.0, -1.0),
    vec2(1.0, 1.0),
    vec2(1.0, -1.0)
};

layout(location = 0) out vec3 f_pos;
layout(location = 1) flat out int face_index;

void main() {
    face_index = gl_InstanceIndex / 6;
    vec4 pos = vec3(verts[gl_VertexIndex], 0.0, 1.0);

    vec4 unproj_pos = unproject[face_index] * pos;
    f_pos = unproj_pos.xyz;

    pos.y /= 6.0;
    pos.y += 1.0 - (1.0 / 6.0);
    pos.y -= float(face_index) * (1.0 / 6.0);
    pos.w = 1.0;
    gl_Position = pos;
}