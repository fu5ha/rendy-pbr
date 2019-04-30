# `rendy-pbr`

This is a small, realtime physically-based renderer written with `rendy`, a 'make-your-own-renderer' toolkit
which builds on `gfx-hal` by providing a render graph, compile- and run-time safety checks, and
other helpers. It is a testbed for `rendy` and Amethyst; much or all of what is implemented here will eventually be added to the Amethyst renderer in some form.

![](screenshots/ibl1.png)

## Planned features/next steps:

-   [x] Physically based shading model
-   [x] Point lights
-   [x] Basic `glTF` import
-   [x] HDR rendering with a tone mapping pass
-   [x] More robust `glTF` import
-   [x] Scene format for loading models from multiple glTF files
-   [x] Diffuse and specular image based lighting using split-sum approximation
-   [x] Emissive materials
-   [ ] Bloom
-   [ ] Postprocess color correction
-   [ ] Directional lights
-   [ ] Shadow mapping
-   [ ] (Maybe) Vertex skinning/animation

# Scene Description

See `scene.rs` for a description of the scene format, and `assets/scene.ron` for an example. Should be able to load
data from any PBR metallic-roughness based glTF assets. If you encounter issues, please open a ticket in the issue
tracker!

# Controls

### Navigation

-   **Left click**: Rotate camera
-   **Middle click**: Pan camera
-   **Right click/Scroll wheel**: Dolly camera

\* _Note: for now model controls are disabled_

### ~~Model controls~~

-   ~~**X**: Add a row of models in the X direction~~
-   ~~**Y**: Add a row of models in the Y direction~~
-   ~~**Z**: Add a row of models in the Z direction~~

\* _Hold shift to subtract a row_

### Tonemapping/Exposure controls

-   **A**: Use ACES Tonemapping curve
-   **U**: Use Uncharted 2 Tonemapping curve
-   **C**: Display Uncharted 2 and ACES in split-screen configuration
-   **Hold CTRL + left click**: Adjust split screen split
-   **E**: Increase exposure f-stop (hold shift to decrease)

### Environment Mapping/Processed IBL Mapping Display Controls

-   **M**: View HDR environment map
-   **I**: View convoluted irradiance map
-   **S**: View convoluted specular radiance map
-   **S**: View rougher convolution of specular map
-   **Shift+S**: View smoother convolution of specular map

# More Screenshots

![](screenshots/helmet1.png)
![](screenshots/helmet2.png)
![](screenshots/ibl2.png)
![](screenshots/ibl3.png)
![](screenshots/scene1.png)
![](screenshots/sword1.png)
![](screenshots/sword2.png)
![](screenshots/helmet3.png)
![](screenshots/helmet4.png)
