#version 450
#extension GL_ARB_separate_shader_objects : enable

layout(constant_id = 0) const uint SAMPLE_COUNT = 1024;

layout(location = 0) in vec3 f_pos;
layout(location = 1) flat in int face_index;

layout(std140, set = 0, binding = 0) uniform UniformArgs {
    mat4 proj;
    mat4 view[6];
    float roughness;
};

layout(set = 0, binding = 1) uniform sampler env_sampler;
layout(set = 0, binding = 2) uniform textureCube env_texture;

layout(location = 0) out vec4 color;

const float PI = 3.14159265359;

float RadicalInverse_VdC(uint bits) 
{
    bits = (bits << 16u) | (bits >> 16u);
    bits = ((bits & 0x55555555u) << 1u) | ((bits & 0xAAAAAAAAu) >> 1u);
    bits = ((bits & 0x33333333u) << 2u) | ((bits & 0xCCCCCCCCu) >> 2u);
    bits = ((bits & 0x0F0F0F0Fu) << 4u) | ((bits & 0xF0F0F0F0u) >> 4u);
    bits = ((bits & 0x00FF00FFu) << 8u) | ((bits & 0xFF00FF00u) >> 8u);
    return float(bits) * 2.3283064365386963e-10; // / 0x100000000
}

vec2 Hammersley(uint i, uint N)
{
    return vec2(float(i)/float(N), RadicalInverse_VdC(i));
}

vec3 ImportanceSampleGGX(vec2 Xi, vec3 N, float roughness)
{
    float a = roughness*roughness*roughness;
	
    float phi = 2.0 * PI * Xi.x;
    float cosTheta = sqrt((1.0 - Xi.y) / (1.0 + (a*a - 1.0) * Xi.y));
    float sinTheta = sqrt(1.0 - cosTheta*cosTheta);
	
    // from spherical coordinates to cartesian coordinates
    vec3 H;
    H.x = cos(phi) * sinTheta;
    H.y = sin(phi) * sinTheta;
    H.z = cosTheta;
	
    // from tangent-space vector to world-space sample vector
    vec3 up        = abs(N.z) < 0.999 ? vec3(0.0, 0.0, 1.0) : vec3(1.0, 0.0, 0.0);
    vec3 tangent   = normalize(cross(up, N));
    vec3 bitangent = cross(N, tangent);
	
    vec3 sampleVec = tangent * H.x + bitangent * H.y + N * H.z;
    return normalize(sampleVec);
}  

void main() {
    vec3 pos = f_pos;
    if (face_index < 4) {
        pos.z *= -1.0;
    }
    if (face_index > 3) {
        pos.x *= -1.0;
    }
    vec3 N = normalize(pos);
    vec3 R = N;
    vec3 V = R;

    float total_weight = 0.0;
    vec3 acc = vec3(0.0);

    for (uint i = 0u; i < SAMPLE_COUNT; i++) {
        vec2 Xi = Hammersley(i, SAMPLE_COUNT);
        vec3 H = ImportanceSampleGGX(Xi, N, roughness);
        vec3 L = normalize(2.0 * dot(V, H) * H - V);

        float NdotL = max(dot(N, L), 0.0);
        acc += texture(samplerCube(env_texture, env_sampler), L).rgb * NdotL;
        total_weight += NdotL;
    }

    acc = acc / total_weight;

    // for (int i = 0; i < 6; i++) {
    //     color[i] = vec4(0.0);
    // }

    color = vec4(acc, 1.0);
}
