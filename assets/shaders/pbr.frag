#version 450
#extension GL_ARB_separate_shader_objects : enable

layout(early_fragment_tests) in;

layout(location = 0) in vec4 f_world_pos;
layout(location = 1) in vec3 f_norm;
layout(location = 2) in vec2 f_uv;
layout(location = 3) in vec3 view_vec;

layout(location = 0) out vec4 color;

struct Light {
    vec3 pos;
    float pad;
    vec3 color;
    float pad1;
    float intensity;
};

layout(std140, set = 0, binding = 0) uniform Args {
    mat4 proj;
    mat4 view;
    vec3 camera_pos;
    float pad;
    int lights_count;
    int pad1[3];
    Light lights[32];
};

layout(set = 0, binding = 1) uniform sampler tex_sampler;

layout(set = 1, binding = 0) uniform texture2D albedo_map;
layout(set = 1, binding = 1) uniform texture2D normal_map;
layout(set = 1, binding = 2) uniform texture2D metallic_roughness_map;
layout(set = 1, binding = 3) uniform texture2D ao_map;

// http://www.thetenthplanet.de/archives/1180
mat3 cotangent_frame( vec3 N, vec3 p, vec2 uv ) {
    // get edge vectors of the pixel triangle
    vec3 dp1 = dFdx( p );
    vec3 dp2 = dFdy( p );
    vec2 duv1 = dFdx( uv );
    vec2 duv2 = dFdy( uv );
 
    // solve the linear system
    vec3 dp2perp = cross( dp2, N );
    vec3 dp1perp = cross( N, dp1 );
    vec3 T = dp2perp * duv1.x + dp1perp * duv2.x;
    vec3 B = dp2perp * duv1.y + dp1perp * duv2.y;
 
    // construct a scale-invariant frame 
    float invmax = inversesqrt( max( dot(T,T), dot(B,B) ) );
    return mat3( T * invmax, B * invmax, N );
}

vec3 saturate(vec3 v) {
    return min(vec3(1.0), max(vec3(0.0), v));
}

void main() {

    vec3 albedo = texture(sampler2D(albedo_map, tex_sampler), f_uv).rgb;
    vec3 normal = texture(sampler2D(normal_map, tex_sampler), f_uv).rgb;
    normal = normal * 2.0 - 1.0;

    vec3 V = normalize(view_vec);

    mat3 TBN = cotangent_frame(normalize(f_norm), -V, f_uv);

    vec3 N = normalize(TBN * normal);

    vec3 diffuse_acc = vec3(0.0);
    vec3 specular_acc = vec3(0.0);
    for (int i = 0; i < lights_count; ++i) {
        vec3 L = lights[i].pos - f_world_pos.xyz;
        float d2 = dot(L, L);
        L = normalize(L);
        vec3 H = normalize(N + L);
        vec3 l_contrib = lights[i].color * lights[i].intensity / d2;
        float NdotL = dot(N, L);
        float NdotH = dot(N, H);
        diffuse_acc += l_contrib * NdotL;
        specular_acc += l_contrib * pow(NdotH, 10.0);
    }
    color = vec4(saturate(diffuse_acc * albedo + specular_acc), 1.0);
}