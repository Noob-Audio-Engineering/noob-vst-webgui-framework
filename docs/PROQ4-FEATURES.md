# FabFilter Pro-Q 4 — Feature Inventory (from the official user manual)

> The reference inventory I used to build Noob-Q, my humorous spoof of Pro-Q 4 that exercises vst3-web-stratum. It is not a claim of parity with FabFilter's product.

Source: FabFilter Pro-Q 4 User Manual (64 pages). Sections below follow the manual's own chapter order.
Values marked **(manual)** are stated explicitly in the manual text. Values marked **(screenshot)** are read from
manual screenshots only. Items marked **(not in manual)** are gaps the manual does not resolve; treat them as
open decisions for the re-implementation.

Conventions used in this document:

- Modifier keys are written Windows-first: `Ctrl` = `Cmd` on macOS, `Alt` = `Alt/Option` on macOS.
- In Pro Tools the plug-in follows Pro Tools conventions: reset = `Alt+click`, fine-tune = `Ctrl+drag` (Win) / `Cmd+drag` (mac), linked knobs = `Shift`.

---

## 0. Product-level facts (Introduction / About / Quick start)

| Fact | Value |
|---|---|
| Max EQ bands | 24 **(manual)** |
| Plug-in formats | VST, VST3, CLAP, AU, AAX Native, AudioSuite; AUv3 on iPad **(manual)** |
| Channel layouts | mono, stereo, surround/immersive up to 9.1.6 Dolby Atmos (DAW/format dependent) **(manual)** |
| Requirements | Windows 11/10/8/7/Vista, 64-bit or 32-bit; macOS 10.13+, 64-bit only, Intel or Apple Silicon **(manual)** |
| Internal headroom | unlimited; plug-in never clips internally **(manual)** |
| Display ranges | ±3 dB and ±6 dB (mastering), ±12 dB and ±30 dB (mixing) **(manual)** |
| Frequency axis | 10 Hz to 30 kHz (display extends to 30 kHz so filters can sit above 20 kHz) **(manual)** |
| Evaluation | 30-day trial with evaluation dialog (Evaluate / Buy Now / Enter License) **(manual)** |
| Preset file extension | `.ffp` (FabFilter Preset), same format on Windows and macOS **(manual)** |

### New in version 4 (manual's own list)

- EQ Sketch (draw the whole EQ curve in one gesture).
- Instance list (control all Pro-Q 4 / Pro-C 3 / Pro-DS / Pro-G instances in the session from one UI).
- Spectral dynamics.
- Character modes (Clean / Subtle / Warm vintage saturation).
- Dynamic EQ improved: Attack and Release settings, optional free side-chain filtering, less distortion.
- Fractional slopes (e.g. 3.5 dB/oct low/high cut).
- Improved precision in linear phase processing.
- New All Pass filter shape.
- Copy and paste EQ bands or presets, also between instances or via the instance list.
- Improved analog matching in zero latency and natural phase modes.
- Parameters can be changed directly in the EQ parameter display (drag or mouse wheel).
- New design.

### Carried over from Pro-Q 3 (manual's "other key features")

Dynamic EQ, Natural Phase and Linear Phase processing, universal slope support for all shapes, EQ Match, resizable
interface with full screen, up to 24 bands, band solo, stereo or mid/side processing, multi-band selection/editing,
spectrum grab, GPU-accelerated graphics, double-click text entry, four display ranges, Pro Tools hardware control
surface support, MIDI Learn, undo/redo, A/B comparison, interactive help hints.

---

## 1. Overview — interface layout

### 1.1 Top bar (left to right) **(manual + screenshot)**

| Element | Purpose |
|---|---|
| FabFilter logo + "Pro-Q 4" name | branding; Pro Tools "Key Input" menu sits above the logo in AAX |
| Undo button | steps back through the undo history (disabled when nothing to undo) |
| Redo button | steps forward (disabled when nothing to redo) |
| A/B button | toggles between state A and state B; highlights active state |
| Copy button | copies active A/B state to the inactive one; disables itself once both are equal |
| Previous-preset arrow `<` | load previous preset without opening the browser |
| Preset name button | opens the preset browser; name dims when preset has been modified |
| Next-preset arrow `>` | load next preset |
| Help menu | help, version info, options (see §26) |
| Full Screen button (top-right corner) | fills the whole screen; `Esc` or click again to exit |

### 1.2 Main area **(manual)**

- Interactive EQ display (fills the window).
- Floating band controls (appear under the selected bands when one or more are selected).
- EQ parameter display (small pop-up next to each band dot).
- Output level meter at the far right (shows all channels on surround layouts, with labels).
- Two vertical scales: yellow scale = EQ curves (display range), gray scale at far right = spectrum analyzer and output meter.
- Display range drop-down in the top-right corner of the display (`12 dB` etc.).
- Frequency scale at the bottom of the display (above the bottom bar); also used for horizontal zooming.

### 1.3 Left edge, just above the bottom bar **(manual)**

| Element | Purpose |
|---|---|
| Piano display button | toggles between frequency scale and piano keyboard |
| EQ Sketch button | starts EQ Sketch mode |

### 1.4 Bottom bar (left to right) **(manual + screenshot)**

| Element | Purpose |
|---|---|
| MIDI Learn button + small drop-down | enters MIDI Learn mode; drop-down opens the MIDI Learn menu |
| Processing Mode button | Zero Latency / Natural Phase / Linear Phase |
| Processing Resolution button | visible in Linear Phase mode, or whenever any band is Spectral (then also shows a spectral icon); warning icon appears when Very High / Maximum is combined with dynamic bands |
| Instance button (centre; shows current track name) | opens the instance list |
| "Analyzer:" button with summary text (e.g. `Pre+Post+SC`) | hover opens analyzer panel; click makes it sticky |
| Character button | Clean / Subtle / Warm |
| Global Bypass button | bypasses the whole plug-in; red line above button when bypassed |
| Output options button (shows gain scale `%` and output gain `dB`, e.g. `100%  0.0 dB`) | hover opens output panel; click makes it sticky; drag vertically to change output gain / gain scale directly; blue line = phase invert, yellow line = auto gain |
| Resize button (far right) | size and scaling menu |

---

## 2. Knobs — generic control behaviour

All round knobs support three control modes **(manual)**:

1. **Vertical drag**: click and drag up/down; speed-sensitive (slow drag = fine).
2. **Mouse wheel**: hover and scroll (also works for panning rings). On Windows the plug-in window may need a click first to become active.
3. **Text entry**: double-click a knob to type an exact value.

Gestures **(manual)**:

| Action | Gesture |
|---|---|
| Reset knob to default | `Ctrl+click` (Win) / `Cmd+click` (mac); Pro Tools: `Alt+click` |
| Fine-tune (drag or wheel) | hold `Shift`; Pro Tools: `Ctrl+drag` (Win) / `Cmd+drag` (mac) |
| Linked knobs (adjust two at once, opposite direction) | hold `Alt` while dragging (Pro Tools: `Shift`) |
| Parameter value pop-up | appears on hover; shows name and current value |

Text-entry shortcuts **(manual)**:

- Frequencies: `1k` → 1000 Hz, note names such as `A4` → 440 Hz, note+cents such as `C#3+13`, `D#5 +13`.
- dB values: `2x` → +6 dB (two times louder).
- Any value: a percentage such as `50%` places the knob exactly at its middle position.

---

## 3. Display and workflow

### 3.1 Creating bands **(manual)**

| Gesture | Result |
|---|---|
| Click the yellow overall curve and drag up/down | creates a band at that position; a preview of the curve type is shown when dragging starts |
| Hover the display background | subtle curve preview appears showing the band that would be created |
| Click or double-click display background | creates a band of the previewed type (single-click only works when no bands are selected) |
| Double-click, or `Ctrl+click`, on background when bands are selected | creates a band (single click would only deselect; the preview shows as a dashed line) |
| `Alt` + any creation gesture (`Alt+drag`, `Alt+double-click`, `Alt+Ctrl+click`) | creates a **dynamic** band (initialised with dynamic range instead of gain) |
| `Alt+Shift` + creation gesture (`Alt+Shift+click`) | creates a **spectral** band |
| Click-and-drag left to right from a fresh state | EQ Sketch (see §4) |

Shape chosen by position **(manual)**:

- Far low area of the display → Notch (double-click).
- Far left / far right areas → Low Cut / High Cut.
- Drag the yellow curve at the left or right end → Low Shelf / High Shelf.
- Elsewhere → Bell.

### 3.2 Selecting bands **(manual)**

| Gesture | Result |
|---|---|
| Click a band dot or its coloured area | select it |
| Click-drag a rectangle on the background | select adjacent bands inside the rectangle |
| `Ctrl+click` another dot | add to / toggle multi-selection |
| `Shift+click` a dot | select a consecutive range of bands |
| Click the display background | deselect all (also hides the band controls) |

### 3.3 Adjusting and editing bands in the display **(manual)**

| Gesture | Effect |
|---|---|
| Drag a selected dot | frequency (horizontal) and gain (vertical); with multiple bands, gains scale relative to each other |
| Drag a dynamic band's dynamic-range indicator up/down | dynamic range |
| Mouse wheel over a curve (or while dragging) | Q (narrower/wider); for Low/High Cut it steps the **slope** through fixed values; `Shift+wheel` sets fractional slope |
| `Ctrl+drag` vertically | Q of all selected bands |
| `Alt+wheel` | dynamic range |
| `Ctrl+wheel` | gain |
| `Alt+Ctrl+wheel` | linked change: trades gain for dynamic range |
| `Shift` while dragging or wheeling | fine-tune |
| `Alt+drag` | constrain to horizontal (frequency) or vertical (gain, or Q when combined with `Ctrl`) |
| `Alt+click` a dot | toggle band bypass |
| `Ctrl+Alt+click` a dot | cycle band shape |
| `Alt+Shift+click` a dot | change slope |
| Double-click a dot | enter values in the EQ parameter display (`Tab` steps through Frequency, Gain, Q) |
| Double-click a value in the EQ parameter display | edit that value directly |
| Right-click a dot | band pop-up menu (curve menu) |

### 3.4 Copying and pasting bands **(manual)**

- Right-click a band dot → **Copy** copies the band or the whole selection.
- Right-click the display background → **Paste** pastes into any Pro-Q 4 instance.
- Right-click background → **Copy** copies all bands.
- Whole-plug-in copy/paste (incl. output settings and processing mode) lives in the preset browser / preset button right-click menu (§24).

### 3.5 Curve (band) menu items mentioned across the manual

Opened by right-clicking a band dot, or via the triangular menu button in the EQ parameter display. Items named in the manual:

- Copy / Paste
- Make Dynamic
- Make Spectral
- Stereo placement (Left / Right / Stereo / Mid / Side) and speaker set (surround)
- Reset Placement/Speakers
- "and more" **(not itemised in manual)**

### 3.6 EQ parameter display **(manual + screenshot)**

Pop-up next to each band dot showing exact values with quick controls:

| Control | Notes |
|---|---|
| Bypass (power icon) | toggles band |
| Frequency value | double-click to edit; drag or wheel to change; shows note name (+cents) when piano display is on |
| Gain value | same |
| Q value | same; screenshot shows a `Cmd` hint next to Q |
| Slope value (LP/HP only) | click opens slope menu; `Shift` while dragging sets fractional slope (Pro Tools: `Ctrl`/`Cmd`) |
| Solo button (headphones icon) | click-and-hold to solo (see §10) |
| Delete (`x`) | deletes the band |
| Shape button | opens shape picker (row of shape icons in screenshot) |
| Menu button (triangle) | opens the band (curve) menu |

Can be disabled with **Help > Show EQ Parameter Display**.

### 3.7 Display range **(manual)**

- Drop-down in the display's top-right corner: **±3 dB, ±6 dB, ±12 dB, ±30 dB**. Screenshots show `12 dB` on the default preset **(screenshot)**.
- When dragging a curve outside the current range, the range expands automatically. Disable via **Help > Auto-Adjust Display Range**.

### 3.8 Horizontal zooming **(manual)**

Operate on the frequency scale at the bottom of the display:

| Gesture | Effect |
|---|---|
| Click and drag up/down | zoom in/out around the clicked frequency |
| Drag left/right (while zoomed) | scroll the frequency scale |
| Double-click the scale | return to full range |

### 3.9 Display-related Help-menu options **(manual)**

- **Auto-EQ Sketch**: enables/disables curve previews (and sketch-on-drag) in the display.
- **Auto-Adjust Display Range**.
- **Show EQ Parameter Display**.
- **Show Frequency On Hover**: shows frequency under the cursor in the scale and highlights the piano key.
- **Use Accessible Colors**: brighter colour for collision highlighting.

---

## 4. EQ Sketch

What it does: draw the whole EQ curve with one left-to-right mouse gesture; Pro-Q 4 adds bands along the way. **(manual)**

- Auto-starts on a default (empty) preset when you click on the display and drag from left to right.
- With existing curves, click the **EQ Sketch button** (bottom-left) first, then sketch anywhere.
- Rules while sketching:
  - Move left to right to draw the desired result curve.
  - Steepness of the movement sets the **slope** (Low Cut / High Cut) or **Q** (other shapes) of the band being drawn.
  - A band is finalised when the cursor returns near the 0 dB line; moving far enough from 0 dB starts a new band whose type depends on position.
  - Moving the mouse back (right to left) during the same gesture removes bands added earlier in that gesture so the section can be redrawn.
- Undo reverts the whole sketch.
- Can be disabled in the Help menu (Auto-EQ Sketch); the dedicated button still works.
- Also available inside the instance list at higher zoom levels.

---

## 5. Band controls (floating panel)

Appears under the selected bands; hidden when nothing is selected. Left to right **(manual)**:

### 5.1 Bypass button
Bypasses selected bands. Band dims in the display, red light in the button. Also `Alt+click` on the dot.

### 5.2 Shape button — filter shapes **(manual)**

| # | Shape | Description | Gain? | Dynamic/Spectral capable? |
|---|---|---|---|---|
| 1 | Bell | parametric peak | yes | yes |
| 2 | Low Shelf | boost/attenuate lows | yes | yes |
| 3 | Low Cut | removes below cutoff | no | no |
| 4 | High Shelf | boost/attenuate highs | yes | yes |
| 5 | High Cut | removes above cutoff | no | no |
| 6 | Notch | cuts a small section | no | no |
| 7 | Band Pass | isolates a section | no | no |
| 8 | Tilt Shelf | tilts the spectrum around a frequency | yes | no (manual lists dynamic range for Bell, Shelving, Flat Tilt only) |
| 9 | Flat Tilt | tilts with a flat curve around a frequency | yes | yes (dynamic range "Bell, Shelving and Flat Tilt") |
| 10 | All Pass | phase adjustment without gain change; alternative to phase inversion | no | no |

Shape can also be cycled with `Ctrl+Alt+click` on the dot. Gain-Q interaction applies only to Bell.

### 5.3 Slope button **(manual)**

| Property | Value |
|---|---|
| Range | 0 dB/oct to 96 dB/oct; up to **Brickwall** for Low Cut and High Cut |
| Fractional slopes | any value in between (e.g. 3.5 dB/oct) |
| Minimum slope per shape | Bell, Notch: 12 dB/oct; Low Cut, High Cut, Band Pass: 0 dB/oct; all other shapes: 6 dB/oct |
| Default shown in screenshots | 12 dB/oct **(screenshot)** |
| UI | click opens menu with fixed slope values plus a draggable indicator for in-between values |
| Mouse wheel over the button | steps through the traditional fixed slopes |
| `Shift+drag` (Pro Tools: `Ctrl`/`Cmd`+drag) | set any fractional slope |
| Q lock | Q cannot be adjusted when slope is 6 dB/oct |
| Display shortcut | `Alt+Shift+click` on dot changes slope |

Fixed slope values are not itemised in the manual **(not in manual)**; the range 0–96 dB/oct plus Brickwall is stated.

### 5.4 Frequency knob
Range **10 Hz – 30 kHz** **(manual)**. Multiple selected bands move in parallel. Accepts note names in text entry.

### 5.5 Gain knob
Range **-30 dB to +30 dB** **(manual)**. Only used by Bell, Shelving (Low/High Shelf, Tilt Shelf) and Flat Tilt. Screenshot shows `0 dB` readout above the knob **(screenshot)**.

### 5.6 Dynamic range ring (around the Gain knob)
Range **-30 dB to +30 dB**, possibly limited by the gain limits **(manual)**. Non-zero value makes the band dynamic and reveals dynamic controls. Available for Bell, Shelving and Flat Tilt. See §6.

### 5.7 Q knob
Sets bandwidth. **Q = 1 is the default bandwidth**; value semantics differ from other EQs; shelf Q values are chosen internally for useful shelf shapes **(manual)**. Not adjustable at 6 dB/oct. Screenshot shows `1` readout **(screenshot)**.

### 5.8 Gain-Q interaction button (between Gain and Q; gear-like icon in screenshot)
Analog-console-style coupling: Q narrows automatically as gain increases; gain rises slightly as Q narrows. Bell only. Last chosen setting is remembered and used for new instances **(manual)**.

### 5.9 Previous / Next band buttons with band number
Step through bands in display order; number between them is the band's automation index. Bands are numbered 1,2,3… on creation; deleting a band does **not** renumber the others **(manual)**.

### 5.10 Delete button (`x`, top right)
Removes selected bands; restorable via Undo.

### 5.11 Stereo placement button
Chooses channels affected (see §13). Caption shows selected speakers; extra label (e.g. "Left only") appears when needed. On surround layouts opens the surround panel (§14).

### 5.12 Split button (scissors icon)
Duplicates the selected band into two identical copies, one Left and one Right (or Mid and Side) **(manual)**.

### 5.13 Band-control tips **(manual)**
- Double-click any knob for text entry.
- `Alt` while changing Gain or Dynamic Range knobs changes the other in reverse-linked fashion (trade gain for dynamic range).
- Deselect all bands to hide the panel.

---

## 6. Dynamic EQ

What it does: changes a band's gain dynamically according to the input level (compressor/expander-like). Works with Bell and Shelf shapes at any slope, in all processing modes (Linear Phase only up to **High** resolution) **(manual)**.

Program-dependent behaviour: attack, release and knee adapt to the audio, the band's frequency range and the current dynamic range **(manual)**. Default trigger = band-limited version of the plug-in input matching the band's frequency range **(manual)**.

### 6.1 Ways to make a band dynamic **(manual)**
1. Select bands, turn the **dynamic range ring** around Gain to a positive or negative value.
2. Hover a band in the display and use `Alt+mouse wheel`.
3. Band menu → **Make Dynamic**.
4. Create with `Alt` held: `Alt+drag` the curve, `Alt+double-click`, or `Alt+Ctrl+click`.
5. Drag the band's dynamic-range indicator in the display.

### 6.2 Dynamic controls (shown only for dynamic bands) **(manual + screenshot)**

| Control | Behaviour | Values |
|---|---|---|
| Dynamic range ring | amount of dynamic gain change; positive = expansion, negative = compression; current dynamic gain shown as yellow bar inside the ring on top of the red range indication | -30 … +30 dB (limited by gain limits) |
| Expand `>>` button | toggles auto mode vs customised mode; reveals the dynamics panel; clicking again hides it **and reverts everything to auto** | auto / custom |
| Threshold slider (vertical, in expanded panel) | trigger threshold; top position = automatic threshold (shows `A`); shows the live trigger-signal level; soft knee starts slightly below threshold | top = Auto, else manual level |
| External side chain button | trigger from external side-chain input instead of plug-in input; side chain is band-limited the same way (Band/Free) | off / on |
| Attack knob (`A`) | speed of dynamic gain onset; centre (50%) = auto | 0–100%, 50% = auto |
| Release knob (`R`) | speed of return; centre (50%) = auto | 0–100%, 50% = auto |
| Triggering button | `Band` = trigger on the band's own frequency range; `Free` = reveals low-cut and high-cut trigger-filter controls (screenshot shows a two-handle range bar) | Band (default) / Free |
| Audition button (headphones icon in dynamics panel) | click-and-hold to listen to the current trigger signal | momentary |
| Spectral button (top of the dynamic range ring) | toggles normal vs Spectral dynamics | off / on |
| Bypass dynamics button (top-left of the ring) | bypasses dynamic behaviour of selected bands; ring shown inactive, red light in button | off / on |
| Clear dynamics button (`x` near the ring) | resets dynamic range to 0 dB → back to a static band | action |

### 6.3 Linear Phase interaction **(manual)**
- Dynamic EQ works in Linear Phase up to **High** resolution; attack/release response differs slightly from Zero Latency / Natural Phase.
- At **Very High** or **Maximum** a warning icon appears next to the Processing Mode button and dynamic EQ is not possible; lower the resolution.

### 6.4 Gesture summary for dynamics **(manual)**
- `Alt+wheel` over band: dynamic range.
- `Alt+Ctrl+wheel` over band: linked gain vs dynamic range.
- `Alt` while turning Gain or Dynamic Range knob: reverse-linked change.

---

## 7. Spectral dynamics

What it does: instead of changing the whole band's gain, triggers only on specific frequencies inside the band that exceed the threshold, leaving other frequencies untouched **(manual)**.

| Control / behaviour | Details |
|---|---|
| Enable | add a Bell or Shelf band, set a dynamic range, click the **Spectral** icon above the gain/dynamic range knobs; or `Alt+Shift+click` in the display to create; or band menu → **Make Spectral** |
| Auto mode | threshold, attack, release automatic; **expand >>** to adjust manually like normal dynamic EQ |
| Spectral Density slider | selectivity: low = wide triggered ranges, high = very narrow/specific |
| Spectral Tilt button (top-right of expanded section) | applies a 3 dB/oct tilt to the input spectrum before triggering; **on by default** for new spectral bands |
| Processing | a spectral band always uses **linear phase** processing; other bands keep the global mode |
| Processing Resolution control | appears in bottom bar whenever any band is spectral (with spectral icon if global mode is not linear phase); Low/Medium usually enough for high-frequency work; Very High and Maximum unavailable for spectral/dynamic |
| External side chain | usable as trigger just like normal dynamic EQ |
| Latency | depends on the chosen linear-phase resolution |

---

## 8. Instance list

Opens from the instance button in the bottom bar; shows every Pro-Q 4, Pro-C 3, Pro-DS and Pro-G instance in the session, grouped per track in DAW order (where the format/DAW supports it) **(manual)**.

### 8.1 Toolbar and global controls **(manual)**
| Control | Behaviour |
|---|---|
| Zoom slider (top) | zoom levels from "essentials only" (spectrum + collisions for Pro-Q, Threshold/Wet Gain for Pro-C) to nearly full functionality |
| Auto-zoom toggle (next to zoom slider) | hovered track zooms in automatically |
| Filter text field | filters tracks by name as you type |
| Options button (right of filter field) | **Quick Jump** floating search panel; choose typing behaviour: **Type to Filter** or **Type to Quick Jump** |
| Filter Pinned button | show only pinned tracks vs all |
| Minimap (top right, appears with many tracks) | click/drag or wheel to navigate; shows pinned tracks; toggled with **Show Minimap** button next to Full Screen |
| Close button (top right) or `Esc` | closes instance list |

### 8.2 Per-instance controls (appear on hover) **(manual)**
| Control | Behaviour |
|---|---|
| Bypass (top-left) | enable/disable the plug-in instance |
| Maximize (next to bypass; larger zoom levels) | enlarges track to maximum; click empty space to unzoom; at smallest zoom clicking an instance activates and zooms it |
| Output button (bottom-right) | access to that instance's output settings (level/pan etc.) |
| Emphasize button (above the output knob) | momentarily raises the instance level to identify a track |
| Menu button (centre-right) | preset menu, Copy/Paste, **EQ Match** (Pro-Q 4), **Pin Similar Colors** |
| Track name (left) | auto from DAW; double-click to rename in the list |
| Track colour dot | from DAW when available |
| Pin icon | pin track; `Shift+click` pins a range, `Alt+click` pins one track uniquely; pinned sets can be saved/restored via toolbar options |
| Collision reference icon (red, above track name) | marks the collision reference track; click on another track to make it the reference; also selects the external spectrum shown in the main UI |

### 8.3 Editing inside the list **(manual)**
- At higher zoom, add and edit curves directly (drag dots, edit values in the parameter display, right-click for curve menu, EQ Sketch).
- Band controls panel is **not** shown in the list.
- Drag-and-drop a preset file onto an instance to load it; drag an audio file onto an instance to start EQ Match with that file as reference.
- Best used in Full Screen mode.

### 8.4 Known DAW limitations (manual)
- Pro Tools: correct track positions since 2025.12; no track colours.
- Audio Units (Logic): no colours/ordering; tracks alphabetical.
- FL Studio: ordering may break with latency-introducing plug-ins; CLAP has no track name/location.
- Fallback ordering is alphabetical; stop/restart playback to refresh order.

---

## 9. Collision detection

- Lives in the instance list and analyzer settings. The current track is the **collision reference** by default (red icon above track name); other Pro-Q 4 instances show collisions with the reference track **(manual)**.
- **Show Collisions** button in the analyzer panel enables a red glow on the main analyzer where the current spectrum may collide with the selected external spectrum; same highlighting in the instance list **(manual)**.
- Indication only, not exact science; use High or Maximum analyzer resolution for low-frequency detection **(manual)**.
- **Use Accessible Colors** (Help menu) switches the dark red to a brighter colour **(manual)**.

---

## 10. Solo

- Click-and-hold the **solo button** (headphones icon in the EQ parameter display) to hear only the part of the spectrum the band affects; other bands and the overall curve dim **(manual)**.
- While holding: drag horizontally = band frequency; drag vertically = **solo listening level** **(manual)**.
- Low Cut / High Cut bands solo the frequencies being *cut* **(manual)**.
- `Ctrl` while dragging (Bell / Shelf) changes Q; for gain-less shapes (Low/High Cut, Notch, Band Pass) plain drag changes Frequency and Q **(manual)**.
- With piano display on, the parameter display also shows the note number **(manual)**.

---

## 11. Full Screen mode, resizing and scaling

| Feature | Details **(manual + screenshot)** |
|---|---|
| Full Screen | button at top-right; exit with `Esc` or the button; plug-in auto-chooses larger scaling in full screen |
| Resize menu (bottom-right button) | **Mini** (equals AUv3 default size on iOS), **Small**, **Medium** (default), **Large**, **Extra Large** (labelled "Very Large" in one screenshot) |
| Scaling submenu | 100%, 125%, 150%, 175%, 200% (Monitor Default on Retina), 225%, 250%, 300% |
| Persistence | chosen size becomes default for new instances; scaling remembered separately for normal vs full screen and per monitor type (Retina/High-DPI vs regular) |
| VST3 | free resizing by dragging window edges |
| Greyed options | sizes/scalings too big for the current display are disabled |
| iOS | no resizing or full screen from inside the plug-in (host provides it) |

---

## 12. Piano display

- Toggle with the **Piano Display button** (bottom-left, above the bottom bar) **(manual)**.
- Shows an 88-key grand piano layout **A0 (27.5 Hz) to C8 (4186.01 Hz)** **(manual)**.
- Middle C is displayed as **C4** (Roland convention; Cubase shows C3) **(manual)**.
- Each band has a dot on the keyboard:
  - Click a dot once → quantise band frequency to the exact note.
  - Click-and-drag a dot → change frequency while staying quantised to notes.
- Parameter displays show frequency as note plus cents offset while active; note names may be typed at any time (e.g. `D#5 +13`, `A4`) **(manual)**.
- **Show Frequency On Hover** (Help menu): highlights the key under the cursor and shows its note label **(manual)**.

---

## 13. Stereo options (per-band placement)

Stereo placement button in the band controls. Menu **(manual + screenshot)**:

| Option | Meaning | Curve colour |
|---|---|---|
| Left | process only L | white |
| Right | process only R | red |
| Stereo (default) | both channels | yellow |
| Mid | mono/sum information | green |
| Side | stereo/difference information | blue |

- **Split** (scissors) below the menu: duplicates band into L + R (or M + S) copies **(manual)**.
- The display groups curves that work on the same channels **(manual)**.
- Mono instance: stereo placement and output panning unavailable **(manual)**.
- Presets with channel-specific bands loaded on an incompatible track show those bands disabled; use band menu **Reset Placement/Speakers** to reset placement and speaker settings **(manual)**.
- Linear phase recommended when filtering L/R/M/S differently to avoid phase changes **(manual)**. Using both L/R-specific and M/S-specific bands in Linear Phase doubles latency (two stages) **(manual)**.

---

## 14. Surround and Dolby Atmos

- Supports surround/immersive formats up to 9.1.6 Atmos; UI adapts; output meter shows all channels with labels **(manual)**.
- Stereo placement button opens a **surround panel** **(manual + screenshot)**:
  - **All** button (top-left): include/exclude LFE channels; by default all speakers selected except LFE.
  - Stereo placement selector at top (Left/Right/Stereo/Mid/Side) applies to the selected speakers.
  - Rows of speaker pairs (screenshot: Center, L/R, Lss/Rss, Lsr/Rsr, LFE); click a row to select only that row; click individual speaker icons for single speakers.
  - L/R + Center combination: with L/R selected click Center to add it (L/C/R), and vice versa; click a row again to select it exclusively.
- Band-control caption shows selected speakers and an extra placement label when needed **(manual)**.
- Output panning is not available in surround layouts **(manual)**.
- **Reset Placement/Speakers** in the curve menu re-enables bands loaded from incompatible presets **(manual)**.

---

## 15. Processing mode

Bottom-bar Processing Mode button, menu: **Zero Latency**, **Natural Phase**, **Linear Phase** **(manual)**. Screenshot of the menu shows Natural Phase checked **(screenshot)**; the manual does not state the factory default **(not in manual)**.

| Mode | Behaviour |
|---|---|
| Zero Latency | matches analog magnitude response, no latency, most efficient |
| Natural Phase | matches analog magnitude and phase response; no noticeable pre-ring; most accurate |
| Linear Phase | magnitude only, phase untouched; adds latency and possible pre-ring; enables **Processing Resolution** |

### 15.1 Linear Phase resolutions (latency at 44.1 kHz) **(manual)**

| Resolution | Samples | ≈ ms | Notes |
|---|---|---|---|
| Low | 3072 | 70 | low Q / mid-high work only |
| Medium | 5120 | 116 | recommended general choice |
| High | 9216 | 209 | high Q on low end; highest resolution that still allows dynamic/spectral EQ |
| Very High | 17408 | 395 | no dynamic/spectral EQ |
| Maximum | 66560 | 1509 | no dynamic/spectral EQ; possible pre-echo |

- Latency in samples adapts at other sample rates to keep approximately the same low-frequency resolution/ms latency **(manual)**.
- L/R-specific plus M/S-specific bands → two linear-phase stages → doubled latency **(manual)**.
- Spectral bands always use linear phase regardless of global mode **(manual)**.
- Frequency changes in Linear Phase are zipper-free **(manual)**.
- CPU usage stays low even with 24 bands and does not vary much across resolutions **(manual)**.

Oversampling: the manual does not mention an oversampling control **(not in manual)**.

---

## 16. Character modes

Character button at the right of the bottom bar **(manual)**:

| Mode | Behaviour |
|---|---|
| Clean (default) | original transparent Pro-Q sound |
| Subtle | subtle vintage saturation; program- and frequency-dependent; affected by EQ bands |
| Warm | more apparent tube-like saturation and colour |

Tip: save a character mode into the Default Setting to "mix into it" on every track.

---

## 17. Spectrum analyzer

Analyzer panel pops up on hover over the Analyzer button; click once to make it sticky, click again to hide **(manual)**. Bottom-bar summary reads e.g. `Pre+Post+SC` **(screenshot)**.

### 17.1 Panel controls **(manual + screenshot)**

| Control | Behaviour | Values |
|---|---|---|
| Pre button | show pre-EQ spectrum | on/off |
| Post button | show post-EQ spectrum | on/off |
| SC/Ext button + drop-down | show external spectrum; menu lists **Sidechain Input** plus post-EQ spectrum of every other Pro-Q 4 instance (by track name); shown with light red outline | off / Sidechain Input / instance name |
| Range | vertical range of analyzer | **60 dB, 90 dB (default), 120 dB** |
| Resolution | FFT-like precision; higher = slower update/attack | **Low = 1024, Medium = 2048, High = 4096, Maximum = 8192 points** |
| Speed | release speed of the spectrum | fast ↔ slow; screenshot shows `Medium`; full list not stated **(not in manual)** |
| Tilt | tilts measured spectrum around 1 kHz | dB/oct; **default 4.5 dB/oct**; other values not listed **(not in manual)** |
| Freeze button | stops the spectrum falling and builds a peak-hold maximum; blue line at top of Analyzer button while enabled; click-and-hold = temporary freeze | on/off/momentary |
| Spectrum Grab button | enables/disables automatic Spectrum Grab (default enabled) | on/off |
| Show Collisions button | red-glow collision highlighting vs the selected external spectrum | on/off |

### 17.2 Behaviour notes **(manual)**
- Global bypass disables the analyzer (no audio is handled).
- Analyzer settings are not changed when browsing presets but are saved in songs.
- External spectrum can also be chosen via the instance list collision reference button.
- Hovering the display shows the frequency under the cursor in the frequency scale (Help > Show Frequency On Hover).
- Horizontal zooming gestures (§3.8) apply.

---

## 18. EQ Match

Started from the **instance list**: hover an instance, click its menu button, choose **EQ Match** **(manual)**. Also started by dragging an audio file onto an instance or onto the EQ Match panel **(manual)**.

### 18.1 Step 1 — Choose reference (EQ Match panel above the instance) **(manual + screenshot)**

| Control | Behaviour |
|---|---|
| Input record/pause button | starts/pauses analysis of the plug-in input (running by default when panel opens) |
| Reference source button (drop-down) | **Input**, **Sidechain**, **External ▸** (submenu of all other Pro-Q 4 instances; pinned track auto-selected), **Load File…** (analyse an audio file), **Save Input As Reference Spectrum…**, list of saved reference spectrums ("(No Saved Reference Spectrums)" when empty) |
| Reference record/pause button | starts/pauses analysis of the reference |
| Match > button | enabled only once both input and reference spectra are valid |
| Close (`x`) | abandons |
| Difference curve | thick white line shows reference minus input once both are available |
| Warning | shown when no audio is detected on input or reference |

Spectra average over time; about 30 s is usually enough **(manual)**. Uses the same **Resolution** as the analyzer; raise to High/Maximum for more low-frequency detail **(manual)**.

### 18.2 Step 2 — Match **(manual + screenshot)**

| Control | Behaviour |
|---|---|
| Number of Bands slider (screenshot value 8) | how many bands are used to approximate the difference curve; more = finer match |
| < Analyze button | go back to step 1 |
| Finish button (or click outside the panel) | permanently adds the proposed bands |

---

## 19. Spectrum Grab

- With Pre- or Post-EQ analyzer active, leaving the mouse over the spectrum for a few seconds enters Spectrum Grab mode: existing bands dim, spectrum freezes, major peaks get labels (frequency, or note when piano display is on) **(manual)**.
- Drag a peak on the white spectrum line to create a **Bell** band with an automatically chosen Q; release to return to normal and refine with band controls **(manual)**.
- **Permanent Spectrum Grab**: click-and-hold in the spectrum area until the highlight turns blue; grab multiple peaks; click the background (not the curve) to exit **(manual)**.
- Works best with Post-EQ enabled; works with Pre-EQ only but the cut is not reflected **(manual)**.
- Enabled by default; disable via the Analyzer panel's Spectrum Grab button **(manual)**.
- iOS: press and hold on the spectrum until it turns blue **(manual)**.

---

## 20. Output options

Right-hand side of the bottom bar **(manual + screenshot)**.

| Control | Behaviour | Range / values |
|---|---|---|
| Global Bypass button | bypasses the whole plug-in with latency compensation in Linear/Natural Phase and soft bypass (no clicks); display dims and a red line appears above the button | on/off |
| Phase Invert toggle (`ø`) | flips output polarity; button and line above output button turn blue | on/off |
| Auto Gain (`A`) | static make-up gain estimated from the current EQ settings (not measured); button and line turn yellow | on/off |
| Output Level Metering button | show/hide the meter at far right; clipping shown as warning only | on/off |
| Gain Scale slider (below the knob; drag horizontally) | scales the gain of all bands with a gain setting (Bell, Shelving, Flat Tilt); automatable; bottom bar shows `%` | percentage; screenshot shows 100% |
| Output Gain knob (large) | overall output level; bottom bar shows dB | **-∞ to +36 dB**; screenshot default 0.0 dB |
| Output Pan ring (around the knob; stereo only) | relative L/R or M/S level | not stated |
| Output Pan Mode (stereo only; `L/R` label in screenshot) | Left/Right or Mid/Side panning | L/R, M/S |

Tips **(manual)**: panel can be made sticky by clicking the output button; drag the output button vertically to change output gain or gain scale directly; double-click the output knobs for text entry; set Auto Gain default for new instances via Options > Save As Default.

---

## 21. MIDI Learn

- **MIDI Learn button** (bottom bar) enters learn mode: interface dims, learnable parameters highlighted, each with a balloon showing its assigned controller number **(manual)**.
- Procedure: click a control (red square marks it) → move a MIDI knob/slider → association made; re-assigning replaces silently **(manual)**.
- Exit with the MIDI Learn button or the **Close** button at the top **(manual)**.
- **MIDI Learn menu** (small drop-down next to button) **(manual)**:
  - **Enable MIDI**: global on/off of MIDI control.
  - **Clear** submenu: lists associations; delete individually or clear all.
  - **Revert**: back to last saved mapping (or start-up state).
  - **Save**: saves mapping (auto-saved on plug-in close).
- **Band targeting** **(manual)**: band name drop-down in learn mode selects **Band N** (specific) or **Active Band**; in active-band mode, MIDI buttons (value 127 after lower value) can trigger **previous/next band** and **delete band**.
- Host routing notes for Pro Tools, Logic, Ableton Live, Cubase (MIDI track to plug-in; Logic via AU MIDI-controlled Effects with side-chain input).
- Presets can be changed by MIDI program/bank change when **Enable MIDI Program Changes** is on (§24).

---

## 22. Undo, redo, A/B switch

| Control | Behaviour **(manual)** |
|---|---|
| Undo | steps back through history; every UI change (knob drag, preset load) creates a state |
| Redo | steps forward |
| A/B | switches between two stored states; current state saved before switching; highlights active letter |
| Copy | copies active state to inactive; disables itself when both are equal |

Notes: parameter changes via MIDI or host automation do not create undo states; Undo/Redo disable when empty.

---

## 23. Using on iOS (touch gestures) **(manual)**

| Action | Gesture |
|---|---|
| Adjust knob | touch and drag up/down |
| Fine adjust | hold a second finger while dragging |
| Reset knob | double-tap |
| Type value | press and hold on a knob (or on a value in the parameter display) |
| Create band | drag the result curve or double-tap background |
| Multi-select | draw a rectangle from the background |
| Adjust Q | double-tap and drag |
| Band menu | press and hold the band dot, or tap the small menu button in the parameter display |
| Delete band | tap `X` in the parameter display |
| Spectrum Grab | press and hold on the spectrum until it turns blue |
| Presets | swipe left on a preset/folder to delete/edit/rename/move |

Standalone app hosts the AUv3 with an **Audio On/Off** switch. iOS instance list shows only Pro-Q instances.

---

## 24. Presets

### 24.1 Preset button and browser **(manual + screenshot)**
- Preset button shows the current preset; name dims when modified.
- Browser: folder list (left), preset list, details panel (right) with **name, author, tags, description**; all editable (double-click author/description; `+` adds a tag; double-click a tag to rename; hover and click `x` to remove).
- Factory preset names visible in screenshot: Clean, Click Bug, Default Setting, Drums, Flat 5 Bands, Flat 6 Bands, Flat 7 Bands, High Boost, High Cut, High Cut Brickwall, High Shelf, High Shelf Brickwall, Low Boost, Low Cut, … **(screenshot, partial)**. "Default Setting" (by FabFilter, tags default/clean/start) is an empty canvas.
- Search field with **Type To Search**; star icon next to search shows only favourites; star next to preset name marks favourite.
- Bottom of browser: **Copy**, **Paste**, **Save As**.

Navigation **(manual)**:

| Gesture | Result |
|---|---|
| Hover folder | opens it |
| Click preset | loads; browser closes if mouse leaves, otherwise stays open |
| Arrow keys | navigate |
| `Enter` | load and close |
| `Right arrow` | load without closing |
| `Esc` | close without loading |
| `[` / `]` (browser open) | previous / next preset |
| `<` / `>` buttons around preset button | previous / next preset |
| Typing | filters by folder name, preset name or tag |

### 24.2 Options menu (gear icon left of search) **(manual + screenshot)**
- **Type To Search** (toggle).
- **Enable MIDI Program Changes**: presets numbered bank 0 / program 0 upward, shown as `(0/65) My Preset`; recommend a `__Programs` folder.
- **Save As Default**: overwrites the Default Setting loaded at start-up.
- **Open Other Preset…**
- **Change Preset Folder…**
- **Restore Factory Presets**
- **Refresh**
- Also in the preset menu: **V3 Preset Folder**, **V2 Preset Folder**, **V1 Preset Folder** (older-version presets; may sound slightly different) **(manual)**.

### 24.3 Preset button right-click menu **(manual)**
- **Favorite**, **Save** (overwrite, with confirmation), **Save As**, plus **Copy** / **Paste** of the entire plug-in state (used to migrate from Pro-Q 2/3).

### 24.4 Saving and storage **(manual)**
- Save As opens a standard Save dialog; rename/delete/create folders there; folders become categories.
- Location: `Documents/FabFilter/Presets/Pro-Q 4` (Windows and macOS); older macOS location `~/Library/Audio/Presets/FabFilter/FabFilter Pro-Q 4`.
- Pro Tools control surfaces: use the **Flat 7 Bands** preset as default (fixed band count).

---

## 25. External side chaining

- Side-chain input usable for dynamic/spectral triggering and as analyzer "Sidechain Input" external spectrum **(manual)**.
- Verification: choose **Side Chain** in the analyzer's external spectrum list and confirm the spectrum appears **(manual)**.
- Host walkthroughs for Pro Tools (Key Input menu above the FabFilter logo, bus send), Studio One (Sends > Sidechains), Ableton Live (device Sidechain drop-down), Logic Pro (Side Chain menu in plug-in header), Cubase (Activate Side-Chain button, VST3 only) **(manual)**.
- Automation from Pro-Q 1/2/3 cannot be read by Pro-Q 4 **(manual)**.

---

## 26. Help menu (consolidated from all sections) **(manual)**

- Help / manual access and interactive help hints.
- **About** (version info).
- **Show EQ Parameter Display**.
- **Auto-EQ Sketch** (curve previews and sketch-on-drag).
- **Auto-Adjust Display Range**.
- **Show Frequency On Hover**.
- **Use Accessible Colors**.
- **Enter License**.
- **Deauthorize**.

---

## 27. Colour conventions **(manual)**

| Item | Colour |
|---|---|
| Overall result curve / stereo bands | yellow |
| Left-only curve | white |
| Right-only curve | red |
| Mid curve | green |
| Side curve | blue |
| External spectrum | light red outline |
| Collision area | red glow (brighter with Accessible Colors) |
| Dynamic range indication in ring | red; current dynamic gain = yellow bar |
| Band bypass / dynamics bypass indicator | red light |
| Global bypass | red line above button |
| Phase invert | blue button and line |
| Auto gain | yellow button and line |
| Freeze active | blue line above Analyzer button |
| Permanent Spectrum Grab | blue highlight |
| Selected preset in browser | blue |

---

## 28. Consolidated keyboard and mouse shortcut table

### 28.1 Keyboard

| Key | Context | Action |
|---|---|---|
| `Esc` | Full Screen / instance list / preset browser | exit / close without loading |
| `Enter` | preset browser | load and close |
| `Right arrow` | preset browser | load, keep open |
| Arrow keys | preset browser | navigate |
| `[` / `]` | preset browser open | previous / next preset |
| `Tab` | value editing in EQ parameter display | step Frequency → Gain → Q |
| Typing | preset browser (Type To Search on) | filter |
| Typing | instance list | filter or quick-jump (configurable) |

### 28.2 Mouse (Win / mac in parentheses)

| Gesture | Action |
|---|---|
| Click/drag yellow curve | create band |
| Click / double-click background | create band (double-click required when bands selected) |
| `Ctrl(Cmd)+click` background | create band while bands selected |
| `Alt` + create | dynamic band |
| `Alt+Shift` + create | spectral band |
| Left-to-right drag on empty preset / after EQ Sketch button | EQ Sketch |
| Click dot / area | select band |
| Drag rectangle on background | select adjacent bands |
| `Ctrl(Cmd)+click` dot | add to selection |
| `Shift+click` dot | select range |
| Click background | deselect all |
| Drag dot | frequency + gain |
| `Ctrl(Cmd)+drag` dot vertically | Q |
| `Alt+drag` dot | constrain axis |
| `Shift+drag` | fine-tune |
| Wheel over curve | Q (or slope steps on LC/HC) |
| `Shift+wheel` over LC/HC | fractional slope |
| `Alt+wheel` | dynamic range |
| `Ctrl(Cmd)+wheel` | gain |
| `Alt+Ctrl(Cmd)+wheel` | linked gain/dynamic range |
| `Alt+click` dot | toggle bypass |
| `Ctrl+Alt(Cmd+Alt)+click` dot | change shape |
| `Alt+Shift+click` dot | change slope |
| Double-click dot | value entry |
| Right-click dot | band menu |
| Right-click background | Copy all / Paste |
| Drag dynamic range indicator | dynamic range |
| Click-hold solo button, drag H/V | solo; frequency / listening level |
| `Ctrl(Cmd)+drag` in solo (Bell/Shelf) | Q |
| Double-click knob | text entry |
| `Ctrl(Cmd)+click` knob | reset (Pro Tools: `Alt+click`) |
| `Shift+drag/wheel` knob | fine (Pro Tools: `Ctrl/Cmd+drag`) |
| `Alt+drag` knob | linked pair (Pro Tools: `Shift`) |
| `Shift+drag` slope button | fractional slope (Pro Tools: `Ctrl/Cmd`) |
| Wheel over slope button | step fixed slopes |
| Drag frequency scale up/down | zoom |
| Drag frequency scale left/right | scroll while zoomed |
| Double-click frequency scale | reset zoom |
| Hover spectrum a few seconds | Spectrum Grab |
| Click-hold spectrum until blue | Permanent Spectrum Grab |
| Click-hold Freeze | temporary freeze |
| Click Analyzer / Output button | toggle sticky panel |
| Drag output button vertically | output gain / gain scale |
| Click piano dot | quantise to note |
| Drag piano dot | move quantised |
| `Shift+click` pin (instance list) | pin range |
| `Alt+click` pin (instance list) | pin uniquely |
| Double-click track name (instance list) | rename |
| Drag preset file onto instance | load preset |
| Drag audio file onto instance / EQ Match panel | EQ Match with file as reference |
| Right-click preset button | Favorite / Save / Save As / Copy / Paste |

---

## 29. Gaps the manual does not resolve (for the re-implementation)

- Factory default processing mode (screenshots show Natural Phase selected in menu and bottom bar on the default preset).
- Full lists for analyzer **Speed** and **Tilt** values, and for the fixed slope steps.
- Q range and default numeric limits (only "Q = 1 is default bandwidth" and "not adjustable at 6 dB/oct" are stated).
- Output Pan range/units and Attack/Release units beyond "0–100%, centre = auto".
- Threshold slider numeric range.
- Spectral Density slider range.
- Free-trigger low-cut/high-cut filter ranges.
- Complete band (curve) menu item list.
- Whether any oversampling control exists (none is described).

