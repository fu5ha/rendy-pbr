#version 450
#extension GL_ARB_separate_shader_objects : enable

layout(early_fragment_tests) in;

layout(location = 0) in vec4 f_world_pos;
layout(location = 1) in vec3 f_norm;
layout(location = 2) in vec3 f_tang;
layout(location = 3) flat in float f_tbn_handedness;
layout(location = 4) in vec2 f_uv;

layout(std140) struct Light {
    vec3 pos;
    float intensity;
    vec3 color;
};

layout(set = 0, binding = 0) uniform sampler tex_sampler;
layout(set = 0, binding = 1) uniform textureCube spec_cube_map;
layout(set = 0, binding = 2) uniform textureCube irradiance_cube_map;
layout(set = 0, binding = 3) uniform texture2D spec_brdf_map;

layout(std140, set = 1, binding = 0) uniform Args {
    layout(offset = 0) mat4 proj;
    layout(offset = 64) mat4 view;
    layout(offset = 128) vec3 camera_pos;
    layout(offset = 140) int lights_count;
    layout(offset = 144) Light lights[32];
};

layout(set = 2, binding = 0) uniform texture2D albedo_map;
layout(set = 2, binding = 1) uniform texture2D normal_map;
layout(set = 2, binding = 2) uniform texture2D metallic_roughness_map;
layout(set = 2, binding = 3) uniform texture2D ao_map;
layout(set = 2, binding = 4) uniform texture2D emissive_map;
layout(std140, set = 2, binding = 5) uniform MatData {
    vec3 emissive_factor;
};

layout(location = 0) out vec4 color;

const float MAX_SPEC_LOD = 4.0;

vec3 f_schlick(const vec3 f0, const float vh) {
	return f0 + (1.0 - f0) * exp2((-5.55473 * vh - 6.98316) * vh);
}

float v_smithschlick(const float nl, const float nv, const float a) {
	return 1.0 / ((nl * (1.0 - a) + a) * (nv * (1.0 - a) + a));
}

float d_ggx(const float nh, const float a) {
	float a2 = a * a;
	float denom = pow(nh * nh * (a2 - 1.0) + 1.0, 2.0);
	return a2 * (1.0 / 3.1415926535) / denom;
}

vec3 specularBRDF(const vec3 f0, const float roughness, const float nl, const float nh, const float nv, const float vh) {
	float a = roughness * roughness;
	return d_ggx(nh, a) * clamp(v_smithschlick(nl, nv, a), 0.0, 1.0) * f_schlick(f0, vh) / 4.0;
}

vec3 lambertDiffuseBRDF(const vec3 albedo, const float nl) {
	return albedo * max(0.0, nl);
}

vec3 saturate(vec3 v) {
    return clamp(v, vec3(0.0), vec3(1.0));
}

float saturate(float v) {
    return clamp(v, 0.0, 1.0);
}

void main() {
    vec3 albedo = texture(sampler2D(albedo_map, tex_sampler), f_uv).rgb;
    vec3 normal = texture(sampler2D(normal_map, tex_sampler), f_uv).rgb;
    vec2 metallic_roughness = texture(sampler2D(metallic_roughness_map, tex_sampler), f_uv).bg;
    float metallic = metallic_roughness.x;
    float roughness = metallic_roughness.y;
    float ao = texture(sampler2D(ao_map, tex_sampler), f_uv).r;
    vec3 emissive = texture(sampler2D(emissive_map, tex_sampler), f_uv).rgb;

    normal = normal * 2.0 - 1.0;

    vec3 V = normalize(camera_pos - f_world_pos.xyz);

    vec3 N = normalize(f_norm);
    vec3 T = normalize(f_tang - N * dot(N, f_tang));
    vec3 B = normalize(cross(N, T)) * f_tbn_handedness;
    mat3 TBN = mat3(T, B, N);

    N = normalize(TBN * normal);
    vec3 R = reflect(-V, N);

    float NdotV = abs(dot(N, V)) + 0.00001;

    vec3 f0 = mix(vec3(0.04), albedo, metallic);

    vec3 ambient_irradiance = texture(samplerCube(irradiance_cube_map, tex_sampler), N).rgb;
    vec3 ambient_spec = textureLod(samplerCube(spec_cube_map, tex_sampler), R, roughness * MAX_SPEC_LOD).rgb;
    vec2 env_brdf = texture(sampler2D(spec_brdf_map, tex_sampler), vec2(NdotV, roughness)).rg;

    vec3 ambient_spec_fres = f_schlick(f0, NdotV);

    vec3 ambient_diffuse_fac = vec3(1.0) - ambient_spec_fres;
    ambient_diffuse_fac *= 1.0 - metallic;

    vec3 ambient = (ambient_irradiance * albedo * ambient_diffuse_fac) + (ambient_spec * (ambient_spec_fres * env_brdf.x + env_brdf.y));

    float a = roughness * roughness;
    vec3 acc = vec3(0.0);
    for (int i = 0; i < lights_count; ++i) {
        vec3 L = lights[i].pos - f_world_pos.xyz;
        float d2 = dot(L, L);
        L = normalize(L);
        vec3 H = normalize(V + L);
        vec3 l_contrib = lights[i].color * lights[i].intensity / d2;

        float NdotL = saturate(dot(N, L));
        float NdotH = saturate(dot(N, H));
        float VdotH = saturate(dot(H, V));
        vec3 fresnel = f_schlick(f0, VdotH);
        vec3 k_D = vec3(1.0) - fresnel;
        k_D *= 1.0 - metallic;
        
        vec3 specular = d_ggx(NdotH, a) * clamp(v_smithschlick(NdotL, NdotV, a), 0.0, 1.0) * fresnel;
        specular /= max(4.0 * NdotV * NdotL, 0.001);

        vec3 diffuse = albedo / 3.1415926535 * k_D;

        acc += (diffuse + specular) * NdotL * l_contrib;
    }

    vec3 final = ambient * ao + acc + emissive * emissive_factor;
    color = vec4(final, 1.0);
}