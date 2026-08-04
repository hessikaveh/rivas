![rivas logo](./Rivas_logo.png)

# Rivas

Rivas is a terminal Markdown viewer focused on rendering rich Markdown content
directly in Kitty-compatible terminals. It parses Markdown, renders terminal
text with iocraft, and displays image-backed content through the Kitty graphics
protocol.

## Features

- Headings, paragraphs, block quotes, thematic breaks, and wrapped text.
- Inline emphasis, strong text, strikethrough, inline code, links, and inline math.
- Ordered, unordered, nested, and task lists.
- Tables with Markdown alignment markers.
- Local raster images (Kitty graphics or text fallback).
- Mermaid diagrams rendered to PNG (Kitty graphics or text fallback).
- LaTeX-style math with two rendering modes:
  - **Unicode** (default): renders formulas as Unicode glyphs, works in any terminal.
  - **Image**: rasterizes via the Kitty graphics protocol for pixel-perfect layout.
- Vim-style source editing with a side-by-side live preview.
- Vim-style keyboard navigation in the rendered viewer.

![demo of rivas features](./rivas_demo_short.gif)

## Requirements

Rivas works best with a Kitty-compatible terminal for image and diagram
rendering. Supported terminals:

- Kitty
- WezTerm
- Ghostty

On other terminals, Rivas runs with text fallbacks: images show as
`[Image: alt]`, mermaid diagrams show as `[Mermaid diagram]` with source code,
and math uses Unicode glyphs.

## 🚀 Installation and Usage

### 🪟 Windows

1. Download the `rivas-x86_64-pc-windows-msvc.zip` from the Assets in releases.
2. Extract the `.exe` file to a folder of your choice.
3. (Optional) Add the folder to your **System PATH** to run `rivas` from any terminal.

Note: Only works with nightly WezTerm on windows (since other versions do not have full kitty support).

### 🍎 macOS

**Via Binary:**

1. Download `rivas-x86_64-apple-darwin.tar.gz` from assets in releases.
2. Extract the file: `rivas-x86_64-apple-darwin.tar.gz`.
3. Move it to your bin folder: `sudo mv rivas /usr/local/bin/`.
4. **Note:** If macOS blocks the app, go to *System Settings > Privacy & Security* and click "Allow Anyway."

### 🐧 Linux

```bash
# Download the binary
curl -LO https://github.com/hessikaveh/rivas/releases/download/v0.3.3/rivas-x86_64-unknown-linux-gnu.tar.gz

# Extract and install
tar -xzf rivas-x86_64-unknown-linux-gnu.tar.gz
sudo install -m 755 rivas /usr/local/bin/rivas
```

Then simply run:

```sh
rivas examples/all-rendering-cases.md
```

## Editing

Rivas supports edit in place with neovim style editting and key shortcuts.

## CLI Options

```sh
rivas [OPTIONS] [FILE]
```

| Option | Description |
|---|---|
| `--math <MODE>` | Math rendering mode: `unicode` (default) or `image`. Unicode works everywhere; image requires Kitty graphics. |
| `--force-kitty` | Force Kitty graphics protocol even if the terminal is not auto-detected. Use if your terminal supports Kitty but isn't recognized. |
| `-t, --theme <THEME>` | Color theme: `dark` (default) or `light`. |
| `--cell-width <PX>` | Override the pixel width of a terminal cell. Graphics are shown at their natural size mapped onto this cell, so this grows/shrinks images, diagrams, and formulas. Use to correct a terminal whose `TIOCGWINSZ` pixel size is wrong (e.g., a secondary display). If only one axis is given, the other defaults to its standard value (16). |
| `--cell-height <PX>` | Override the pixel height of a terminal cell (see `--cell-width`). If only one axis is given, the other defaults to 8. |
| `--debug` | Enable debug logging and visual overlay annotations. |

### Math Modes

**Unicode mode** (`--math unicode`, the default) converts LaTeX formulas to
Unicode glyphs. This works in any terminal and is fast.

**Image mode** (`--math image`) rasterizes formulas via the Kitty graphics
protocol for pixel-perfect rendering. Requires a Kitty-compatible terminal
(Kitty, WezTerm, or Ghostty). If the terminal is not recognized, Rivas
automatically falls back to Unicode mode with a warning.

You can also switch math modes at runtime by pressing `:` in Normal mode and
typing `math unicode`, `math image`, or just `math` to toggle.

### Force Kitty Graphics

If your terminal supports the Kitty graphics protocol but is not auto-detected,
use `--force-kitty` to enable it:

```sh
rivas --force-kitty --math image document.md
```

This overrides the terminal detection and attempts Kitty graphics for images,
diagrams, and image-mode math. If the terminal doesn't actually support the
protocol, images and diagrams will simply not render.

### Adjusting Graphic Sizes

Images and diagrams render at their **natural size** (their own pixels mapped
onto the terminal cell), shrinking if they would overflow the screen. If the
automatic cell-size detection is wrong for your display, you can resize
graphics two ways:

- **At runtime** — in Normal/Visual mode press `[` to shrink or `]` to enlarge
  every image, diagram, and formula, and see the result immediately:

  ```
  :q Quit | C-p Find | j/k Scroll | gg/G Top/Bottom | :math Math mode | 🔍 [ ]
  ```

- **From the CLI** — pin the cell size explicitly with `--cell-width` /
  `--cell-height` (see the table above). This also changes the rasterization
  resolution, so use it when you want sizing to be fixed ahead of time.

## Supported Markdown Notes

Math can be written inline with dollar delimiters:

```md
The quadratic formula is $x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}$.
```

Display math can use `$$` blocks or fenced `math` blocks:

````md
$$
\int_0^\infty e^{-x} \, dx = 1
$$

```math
\Delta(Rivas) = \delta(rivas) \times \frac{2}{2}
```
````

Press `:math` in Normal mode to toggle between Unicode and Image rendering
at runtime, or use `:math unicode` / `:math image` to switch directly.

Mermaid diagrams use fenced `mermaid` blocks:

````md
```mermaid
flowchart LR
  A --> B --> C
```
````

Local images are resolved relative to the Markdown file:

```md
![Seedling](./seedling.png)
```

## Development

Run the test suite:

```sh
cargo test
```

The math tests compile LaTeX-like input through TyLax and Typst, rasterize the
resulting SVG to PNG, and verify that rendered output is not an all-white page.
