# `rendy-pbr`

This is a toy realtime physically-based renderer written with `rendy`, a 'make-your-own-renderer' toolkit
which builds on `gfx-hal` by providing a render graph, compile- and run-time safety checks, and
other helpers.

## Planned features/next steps:

* [x] Physically based shading model
* [x] Point lights
* [x] Basic `glTF` import
* [x] HDR rendering with a tone mapping pass
* [ ] Diffuse and specular environment mapping
* [ ] Directional lights
* [ ] Shadow mapping
* [ ] More robust `glTF` import

# Controls

### Navigation
* **Left click**: Rotate camera
* **Middle click**: Pan camera
* **Right click/Scroll wheel**: Dolly camera

### Model controls
* **X**: Add a row of models in the X direction
* **Y**: Add a row of models in the X direction
* **Z**: Add a row of models in the X direction

\* *Hold shift to subtract a row*

### Tonemapping/Exposure controls
* **A**: Use ACES Tonemapping curve
* **U**: Use Uncharted 2 Tonemapping curve
* **C**: Display Uncharted 2 and ACES in split-screen configuration
* **Hold CTRL + left click**: Adjust split screen split

# Screenshots

![](screenshots/helmet1.png)
![](screenshots/helmet2.png)
![](screenshots/helmet3.png)
![](screenshots/helmet4.png)