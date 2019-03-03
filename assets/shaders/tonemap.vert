#version 450 

layout (location = 0) out vec2 outUV;

// This is a trick from Sascha Willems which uses just the gl_VertexIndex
// to calculate the position and uv coordinates for one full-scren "quad"
// which is actually just a triangle with two of the vertices positioned
// correctly off screen.
void main() 
{
	outUV = vec2((gl_VertexIndex << 1) & 2, gl_VertexIndex & 2);
	gl_Position = vec4(outUV * 2.0f + -1.0f, 0.0f, 1.0f);
}