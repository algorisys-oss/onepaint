# onepaint manual

## Overview
onepaint is a painting app for drawing with a pen, a mouse or a touch screen. It paints in layers, mixes colour like real paint, and opens and saves Photoshop files as well as its own.

This manual walks through the app area by area. For a list of every shortcut, open **Help ▸ Shortcuts & gestures** (or press `?`).

## The workspace
The window has four parts:

- **The header** across the top. Its first row holds the menus (File, Edit, View, Image, Layer, Select, Adjust, Help), the drawing's name, and buttons for **Brushes**, **Brush settings**, **Guides** and **Layers**. Drag empty space in this row to move the window; double-click it to maximise.
- **The options row** under the menus. It always starts with your colours. For brushes it shows the brush, its angle, the stabilizer and the pressure switches; then come the current tool's own options, and the zoom menu at the far right. If the row is too long for the window, scroll it sideways with the mouse wheel.
- **The tool rail** down the left edge, with every tool as a button. Drag its grip to float it, double-click the grip to turn a floating bar sideways, and drop it at either edge to dock it again.
- **The panels** on the right: Layers is open by default, with Properties below it. Any panel can float, dock on either side, stack or sit in tabs; drag it by its title. **View ▸ Panels** opens and closes them, and **View ▸ Workspace ▸ Reset panel layout** puts everything back.

The size and opacity sliders sit beside the rail. **View ▸ Workspace** moves them to another edge, lets them float, or puts them on the tool rail.

Press `Tab` for **Zen mode**, which hides everything but the canvas. Press it again to bring the workspace back.

In a narrow window the menus fold into a single **Menu**, the zoom moves to the status bar, and the tools become a strip along the bottom.

## Canvas and navigation
- **Pan:** hold `Space` and drag, drag with the middle mouse button, use the Pan tool (`H`), or scroll with two fingers.
- **Zoom:** `Ctrl` + mouse wheel, `=` and `-`, pinch with two fingers, or the zoom menu at the end of the options row. `0` fits the drawing to the window and `1` shows it at 100%.
- **Rotate:** hold `R` and drag, use `Ctrl+[` and `Ctrl+]` for 15° steps, or twist two fingers. `5` straightens the canvas.
- **Scrollbars** appear when the drawing runs past the window. Drag them to pan or click the track to jump a page. Turn them off in **View ▸ Guides ▸ Scrollbars**.

## Tools
Every tool has a button on the rail; its options appear in the options row.

- **Brush** (`B`) paints with the current brush. Click the Brush button again to open the brush library.
- **Eraser** (`E`) erases with eraser brushes, or with any brush when you pick **All brushes** in its library.
- **Fill** (`G`) fills the area of similar colour you click. **Tolerance** sets how similar; **Grow** spreads the fill a few pixels under line art. **Wash** turns the fill into a watercolour wash that spreads and dries.
- **Eyedropper** (`I`) picks a colour from what you see. With any tool, `Alt`-click does the same, and on a touch screen you can touch and hold.
- **Move** (`V`) moves, scales and rotates the layer or the selection. Use the options row to flip, rotate in steps, **Distort** the corners or **Warp** a grid, then **Apply** (or press `Enter`) or **Cancel** (`Esc`).
- **Smudge** (`S`) drags paint around with the current brush's shape.
- **Mixer** (`Shift+B`) mixes the paint on the brush with the paint on the canvas. See *Wet media and paper*.
- **Gradient** fills with a gradient as you drag, either into the pixels or as a live gradient fill layer.
- **Liquify** pushes, twirls, pinches or expands the picture, or reconstructs it back.
- **Text** (`T`), **Pen** (`P`), **Shape** (`U`) and **Panel** have sections of their own below.
- **Pan** (`H`) moves the view.
- **Selection tools:** rectangle (`M`), ellipse, lasso (`L`), polygon and magic wand (`W`). See *Selections and transform*.

The rail also has **Undo** and **Redo** (when the side sliders are off), **Clear layer** and **Zen mode**.

## Brushes
Open the library with the **Brushes** button or by clicking the brush name in the options row. Brushes are grouped by kind; type in the search box to find one by name.

- `[` and `]` make the brush smaller and larger; `{` and `}` turn its tip by 15°. The **Angle** dial in the options row does the same.
- The **Stabilizer** smooths your line like a pulled string. Its amount sets how far behind the line follows; 0 turns it off.
- **Pressure ▸ Size** and **Opacity** make pen pressure change size or opacity for every brush, whatever the brush's own settings.
- `F` turns on **fill mode**: each stroke fills its own outline.
- Hold the pen still at the end of a stroke to straighten it into a line, ellipse or polygon (**QuickShape**).

**Brush settings** opens the brush studio, with every setting of the current brush, a live preview stroke and a pad to try it on. Save your changes, or save them as a new brush. You can import `.abr`, Procreate `.brush` and `.brushset`, and `.happybrush` files from the library.

## Colour
The colours sit at the start of the options row: click the large swatch to open the colour picker; click the small one, or press `X`, to swap foreground and background; press `D` for black and white.

The colour picker also holds **palettes**. Add the current colour to a palette, make new palettes, import `.swatches`, `.aco`, `.ase` and `.gpl` files, or build a palette from the colours in the drawing or in an image.

`Alt+Backspace` fills the selection (or the layer) with the foreground colour, and `Ctrl+Backspace` with the background colour.

## Several drawings at once
New and opened drawings open in tabs above the canvas. Switch with `Ctrl+Alt+←` and `Ctrl+Alt+→`, or click a tab. Closing a drawing with unsaved changes asks first. **File ▸ Save drawings as book** keeps all open drawings together in one file.

## Layers
The Layers panel lists the drawing's layers, top first.

- Add a layer or a group with the buttons at the top, or from the **Layer** menu.
- Drag layers to reorder them or to move them into and out of groups.
- Click the eye to hide a layer. The panel also sets each layer's **opacity**, **blend mode** and colour label.
- **Duplicate**, **Merge down**, **Clear** and **Delete** sit under the list.
- **Layer ▸ Group** puts the layer in a new group; **Ungroup** lifts a group's layers out again.
- **Image ▸ Merge visible to a new layer** copies what you see onto a new layer; **Flatten image** merges everything into one.
- **Alpha lock** keeps painting inside what is already on the layer; **Lock** stops all changes.
- `Ctrl`-click layers to pick several, then use **Align & distribute** (View ▸ Panels) to line them up.

Groups normally act as one picture. Set a group's blend mode to **Pass Through** to let its layers blend with everything below the group.

## Masks and clipping
- **Layer ▸ Add mask** hides nothing at first; paint black on the mask to hide and white to show. **Mask from selection** starts from the selection instead. Click the mask's thumbnail to paint on it, and the layer's thumbnail to go back.
- **Invert mask** swaps hidden and shown, **Apply mask** bakes it into the layer, and **Delete mask** removes it. Properties sets the mask's density and feather.
- **Vector masks** come from paths: draw with the Pen, then use **Create vector mask** from its Path menu.
- A **clipping mask** (Layer ▸ Clipping mask) shows a layer only where the layer below it has paint.
- **Blend If** in Properties hides a layer's light or dark parts, or lets the layers below show through them.

## Adjustment and fill layers
Every adjustment in the **Adjust** menu can be a live **adjustment layer**: it changes everything below it, and you can change its settings or switch it off at any time. Choose **As adjustment layer** in the adjustment's panel.

**Layer ▸ New fill layer** adds a solid colour, gradient or pattern layer. Patterns can be built-in or made from an image of your own.

Select an adjustment or fill layer to edit it in **Properties**. Gradients, blurs, distortions and some effects show handles on the canvas: drag them to place the centre, the ends or the reach.

## Layer styles
Properties holds a layer's **styles**: drop shadow, inner shadow, outer and inner glow, bevel and emboss, satin, colour, gradient and pattern overlays, and stroke. Pick one of the built-in styles for a quick start, add effects one by one, and switch them on and off. **Bake** turns the styles into pixels. Save your own styles as presets, or import Photoshop `.asl` files.

## Selections and transform
- Drag with the rectangle or ellipse, draw freely with the lasso, or click corners with the polygon tool (`Enter` closes it). The magic wand selects similar colour; turn off **Contiguous** to select it everywhere, and turn on **All layers** to sample what you see.
- Hold `Shift` to add to the selection, `Alt` to subtract and both to intersect, or pick the mode in the options row.
- The options row and the **Select** menu expand, contract, smooth, invert and clear the selection. **Feather** softens its edge.
- **Saved** keeps selections with the drawing to load later.
- **Copy to new layer** (`Ctrl+J`) and **Cut to new layer** (`Ctrl+Shift+J`) lift the selection onto a layer of its own.
- Painting, filling and filters stay inside the selection.

## Filters and effects
The **Adjust** menu holds every adjustment and filter, by kind: colour, blur, sharpen and noise, distort, pixelate, stylize, render, fill and effect lines. Each opens a panel with a live preview; **Apply** changes the layer, **As adjustment layer** keeps it live, and **As smart filter** keeps it on the layer where you can change or remove it later.

The **Render** effects (light rays, lens flare, clouds, fog, smoke, nebula, starfield and lightning) draw onto a new layer. **Effect lines** draws focus or speed lines as an editable path layer.

## Text and balloons
Click with the Text tool to add text; turn on **Wrap** in the text panel to wrap it in a box. The text panel sets the font, size, colour, outline and background, and styles part of the text when you select it: bold, italic, underline, strikethrough, size, font and colour. Add a **balloon** (speech, rounded, shout or thought) with a tail you can aim. Text stays editable until you convert it to paint (**Layer ▸ Convert to paint layer**).

## Shapes, pens and paths
- The **Shape** tool draws lines, ellipses, rectangles and polygons with the current brush. `Shift` keeps them square or at 45°, `Alt` draws from the centre, and **Fill** fills them.
- The **Pen** draws paths: click for corners, drag for curves, and click the first point or press `Enter` to finish. On a path layer, drag its points and handles to reshape it.
- The pen's **Path** menu makes a selection from the path, fills or strokes it on the layer below, makes a vector mask, transforms it, or turns it into paint.

## Rulers, guides, symmetry and perspective
Open the **Guides** button in the header, or the **View ▸ Guides** menu.

- **Rulers** (`Shift+R`): drag from a ruler to make a guide line, and drag a guide back onto the ruler to remove it. **Edit guides** (`Shift+G`) moves and turns them; **Measure** (`Shift+D`) reads distances and angles.
- **Grid**: set its size and subdivisions, and snap to it.
- **Symmetry** (`Y`): mirror vertically, horizontally or both, or repeat around a centre with up to 24 spokes. `Shift+Y` moves the axis.
- **Perspective**: one to five vanishing points; strokes can snap to them. `Shift+P` edits the points.

## Comic panels
Pick the **Panel** tool and choose a page layout in the options row, or drag to draw a frame. Drag across a frame to cut it in two, drag a corner to reshape it, and drag a frame line to move it. Each panel clips its own art. The tool's panel sets the gutter, margin and border width for every frame at once.

## Wet media and paper
- **Paper** (File ▸ Paper…) gives the whole drawing a surface: smooth, a watercolour or drawing paper, linen or canvas. Dry brushes catch its tooth and wet paint soaks into it. **Apply paper colour to Background** tints the background like the sheet.
- **Watercolour and oil brushes** keep moving after you lift the pen: the paint spreads, blooms and dries. The header then shows a **tilt pad**, which makes wet paint run downhill, and a **Dry** button that sets it at once.
- **Impasto** brushes build up thick paint that catches the light; blender brushes smooth it.
- The **Mixer** brush mixes like real paint. **Wet** sets how much canvas paint it drags, **Load** how much paint it carries, and **Mix** how much the canvas colour mixes in. **Keep colour** holds what it picked up between strokes, **Clean** empties it, and `Alt`-click (or **Load from canvas**) fills it with the paint under the brush.

## Animation
Turn on animation with **View ▸ Panels ▸ Animation** (`Shift+A`). The timeline adds frames (`N`), holds them, and plays them (`K`). Step with `,` and `.`. Onion skins (`O`) show the frames before and after. Tracks, markers, a play range, and batch timing for frames sit in the timeline too. Export the animation as MP4, an animated GIF or a set of PNG files.

## Recording and time-lapse
**View ▸ Capture ▸ Record drawing** records every step as you paint. **Replay recording** plays a recording back on the canvas, and **Export timelapse** turns it into a video. onepaint can also replay happypaint recordings.

## Files: open, save and export
- **New canvas** (`Ctrl+N`) sets the size, resolution and paper; **New or open drawing** shows the gallery of your recent drawings.
- **Open** (`Ctrl+O`) reads onepaint and happypaint drawings, books and recordings, Photoshop `.psd` and `.psb` files, and PNG, JPEG, WebP, TIFF and PPM images.
- **Save** (`Ctrl+S`) keeps the drawing with all its layers. A Photoshop file saves back as Photoshop, in its own colour mode and depth.
- **Export** (`Ctrl+Shift+E`) writes PNG, JPEG, TIFF, PPM or PSD, at any scale.
- **Insert image** places a picture as a new layer, and **Paste image** pastes one from the clipboard.
- onepaint saves your work as you go. If it closes unexpectedly, it offers to recover the drawing next time.

## Printing and colour checks
**View ▸ Print preview** shows how the drawing will print on a press, with paper white and black ink if you like. Load a press profile (`.icc`) to preview a particular press. **Non-printable colours** marks colours the press can't reach, and **Grayscale** (`Ctrl+Y`) checks your values.

## Settings
**Edit ▸ Settings** (`Ctrl+,`) sets:

- how many drawings can be open, and whether tabs show;
- the largest brush size, and whether brush size follows the canvas or the screen;
- how the brush cursor looks;
- how many steps you can undo;
- your keyboard shortcuts: click one and press the new keys;
- whether onepaint uses the system's title bar (takes effect when it starts again).

The light and dark themes switch from **View ▸ Workspace**.

## Shortcuts and gestures
**Help ▸ Shortcuts & gestures** (`?`) lists every shortcut, including any you have changed. On a touch screen:

- pinch to zoom and rotate, and drag two fingers to pan;
- tap two fingers to undo and three to redo;
- touch and hold to pick a colour;
- swipe three fingers down for copy and paste;
- tap four fingers to hide or show the workspace.

While you draw with a pen, onepaint ignores your palm on the screen.
