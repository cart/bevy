#version 450

layout(location = 0) in vec3 Vertex_Position;


layout(set = 0, binding = 0) uniform CameraViewProj {
    mat4 ViewProj;
};

layout(set = 1, binding = 0) uniform Sprite {
    mat4 Model;
    vec2 SpriteSize;
};

void main() {
    vec3 position = Vertex_Position * vec3(SpriteSize, 1.0);
    gl_Position = ViewProj * Model * vec4(position, 1.0);
}
