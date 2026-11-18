# raylib

This package binds [raylib](https://www.raylib.com), a C library for
windows, 2D and 3D drawing, input, images, and audio. It shows how an Aura
package wraps a C graphics library through
[FFI](../../../docs/manual/ffi.md).

| File | What it is |
| --- | --- |
| `src/raylib.au` | The binding: raylib's structs, constants, colors, and functions |
| `src/main.au` | A brick game: a window, 2D shapes, text, keyboard and mouse input |
| `src/scene.au` | A 3D scene: a camera, cubes, a sphere, and a model built from a mesh |
| `src/picture.au` | Draws into an image and writes a PNG, with no window |
| `src/demo.au` | What the window programs share |
| `tools/generate_binding.py` | Writes `src/raylib.au` from raylib's API description |

## Install raylib

The package links the raylib your system has. The binding is generated for
raylib 6.0.

| System | Command |
| --- | --- |
| macOS | `brew install raylib` |
| Debian and Ubuntu, where the release has it | `sudo apt-get install libraylib-dev` |
| Any Linux | build it from source, as below |
| Windows | install a release from the raylib site and add its `lib` folder to `library_paths` in `Aura.toml` |

To build raylib from source as a shared library:

```sh
git clone --depth 1 --branch 6.0 https://github.com/raysan5/raylib.git
cmake -S raylib -B raylib/build -DBUILD_SHARED_LIBS=ON -DBUILD_EXAMPLES=OFF
cmake --build raylib/build --parallel
sudo cmake --install raylib/build
```

## Run The Programs

Run each program from this directory:

```sh
aura run src/main.au
aura run src/scene.au
aura run src/picture.au
```

`aura build -o bricks src/main.au` writes a native executable that loads
raylib when it starts.

`picture.au` prints the size of the image it wrote and two of its colors:

```text
picture.png is 320 by 200
the sun is 255, 203, 0
the grass is 0, 117, 44
```

## Use The Binding

Names are raylib's own, so the raylib cheatsheet reads across. `InitWindow`
in C is `raylib.InitWindow` here.

```aura fragment
import raylib

def main() -> int32:
    raylib.InitWindow(640, 360, "hello")
    while not raylib.WindowShouldClose():
        raylib.BeginDrawing()
        raylib.ClearBackground(raylib.RAYWHITE)
        raylib.DrawText("hello from Aura", 190, 160, 30, raylib.DARKGRAY)
        raylib.EndDrawing()
    raylib.CloseWindow()
    return 0
```

How C types read in the binding:

| C | Aura |
| --- | --- |
| `Vector2`, `Color`, `Rectangle`, `Camera3D`, and other structs | an `extern "C"` class, built with keyword arguments and passed by value |
| `const char *` | `cstr`, which takes and returns a `str` |
| `Image *image`, `Camera *camera` | a `mut` parameter, which takes a mutable variable |
| `int *count` | a `mut int32` out-parameter |
| a pointer inside a struct, such as `Image.data` | a `uint64` address that raylib owns |
| a returned pointer, such as `LoadFileData` | a `uint64` address to hand back to the matching `Unload` function |
| `KEY_SPACE`, `FLAG_VSYNC_HINT`, and other enum values | `int32` constants |
| `RED`, `RAYWHITE`, and other colors | `Color` constants |

The binding covers 556 of raylib's 600 functions. The end of
`src/raylib.au` lists the others with the reason for each: structs with a
fixed-size array field, functions that take an array of structs, variadic
functions, and callbacks.

## Run The Tests

The repository's FFI acceptance tests run the three programs on both
backends. They skip on a host that cannot link raylib, and the two window
programs also need a display. Two environment variables make a window
program stop on its own:

| Variable | Effect |
| --- | --- |
| `RAYLIB_DEMO_FRAMES` | Stop after this many frames |
| `RAYLIB_DEMO_SHOT` | Write the last frame to this PNG file before closing |

On a Linux machine with no screen, a virtual display is enough:

```sh
RAYLIB_DEMO_FRAMES=90 RAYLIB_DEMO_SHOT=scene.png xvfb-run -a aura run src/scene.au
```

## Regenerate The Binding

raylib ships a description of its API as
`tools/rlparser/output/raylib_api.json`. To rebuild the binding for another
raylib version:

```sh
python3 tools/generate_binding.py path/to/raylib_api.json > src/raylib.au
aura fmt src/raylib.au
```
