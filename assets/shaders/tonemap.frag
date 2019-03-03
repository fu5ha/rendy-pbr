#version 450
#extension GL_ARB_separate_shader_objects : enable

layout(location = 0) in vec2 f_uv;

layout(set = 0, binding = 0) uniform sampler tex_sampler;
layout(set = 0, binding = 1) uniform texture2D hdr_tex;

layout(std140, set = 0, binding = 2) uniform Args {
    float exposure;
    int curve;
    float comparison_factor;
};

layout(location = 0) out vec4 color;

vec3 uncharted2Tonemap(const vec3 x) {
	const float A = 0.15;
	const float B = 0.50;
	const float C = 0.10;
	const float D = 0.20;
	const float E = 0.02;
	const float F = 0.30;
	return ((x * (A * x + C * B) + D * E) / (x * (A * x + B) + D * F)) - E / F;
}

// http://filmicworlds.com/blog/filmic-tonemapping-operators/
// outputs LINEAR tonemapped data, so should still be used with an SRGB render target
vec3 tonemapUncharted2(const vec3 color) {
	const float W = 11.2; // Hardcoded white point
	vec3 curr = uncharted2Tonemap(color);
	vec3 whiteScale = 1.0 / uncharted2Tonemap(vec3(W));
	return curr * whiteScale;
}

// ACES fit by Stephen Hill (@self_shadow), adapted from the HLSL implementation
// here https://github.com/TheRealMJP/BakingLab/blob/master/BakingLab/ACES.hlsl
vec3 rrt_and_odt( in vec3 v ) {
    vec3 a;
    vec3 b;

    a = ((v * (v + 0.0245786)) - 0.000090537);
    b = ((v * ((0.983729 * v) + 0.432951)) + 0.238081);
    return (a / b);
}

// Linear Rec709 .. XYZ .. D65_D60 .. AP1 .. RRT_SAT
const mat3 ACESInput = mat3 (
    0.59719, 0.07600, 0.02840,
    0.35458, 0.90834, 0.13383,
    0.04823, 0.01566, 0.83777
);

// ODT_SAT .. XYZ .. D60_D65 .. Linear Rec709
const mat3 ACESOutput = mat3 (
    1.60475, -0.10208, -0.00327,
    -0.53108, 1.10813, -0.00605,
    -0.00327, -0.07276, 1.07602
);

vec3 aces_fitted(vec3 color)
{
    color = ACESInput * color;

    // Apply RRT and ODT
    color = rrt_and_odt(color);

    color = ACESOutput * color;
    // Clamp to [0, 1]
    color = clamp(color, 0, 1);

    return color;
}

void main() {
    vec3 hdrColor = texture(sampler2D(hdr_tex, tex_sampler), f_uv).rgb;

    hdrColor *= exposure; // exposure

    float factor;

    if (curve == 0) {
        factor = 0.0;
    } else if (curve == 1) {
        factor = 1.0;
    } else if (curve == 2) {
        factor = comparison_factor;
    }

    vec3 mapped = mix(aces_fitted(hdrColor), tonemapUncharted2(hdrColor), step(f_uv.x, factor));

    color = vec4(mapped, 1.0);
}
