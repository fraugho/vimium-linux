# vimium-linux

Keyboard-driven navigation for Wayland desktops. Navigate, click, and interact with any UI element using only your keyboard.

Inspired by [Vimium](https://github.com/philc/vimium) (browser extension) and [Homerow](https://www.homerow.app/) (macOS).

## Demo

```
$ vimium-linux
```

1. Press your keybind to launch
2. Yellow hint labels appear on all clickable elements
3. Type the hint (e.g., `f`, `as`, `dk`) to click that element
4. Auto-selects when your input uniquely matches one element

## Features

- **Vimium-style hints** - Home-row keys prioritized (a, s, d, f, g, h, j, k, l)
- **Universal** - Works with any application that supports AT-SPI accessibility
- **Wayland native** - Uses wlr-layer-shell for overlay
- **Multiple click backends** - ydotool, wlrctl, or dotool
- **Fast** - Async element discovery, efficient overlay rendering

## Requirements

### System

- Wayland compositor with wlr-layer-shell support:
  - Sway
  - Hyprland
  - River
  - Wayfire
  - Other wlroots-based compositors
- AT-SPI enabled (default on most distros)

### Dependencies

Install **one** of these for click simulation:

```bash
# Option 1: ydotool (recommended)
# Fedora
sudo dnf install ydotool
# Arch
sudo pacman -S ydotool
# Start the daemon
sudo systemctl enable --now ydotool

# Option 2: dotool
# From source: https://sr.ht/~geb/dotool/

# Option 3: wlrctl (wlroots only)
# Arch AUR
yay -S wlrctl
```

AT-SPI (usually pre-installed):
```bash
# Fedora
sudo dnf install at-spi2-core

# Arch
sudo pacman -S at-spi2-core
```

### AT-SPI Setup (Important for Wayland WMs)

On standalone Wayland window managers (Hyprland, Sway, etc.), AT-SPI may need manual setup:

1. **Enable toolkit accessibility:**
```bash
gsettings set org.gnome.desktop.interface toolkit-accessibility true
```

2. **Start the AT-SPI registry daemon** (add to your WM startup):
```bash
/usr/libexec/at-spi2-registryd &
```

3. **For Firefox/LibreWolf:** Go to `about:config` and set:
```
accessibility.force_disabled = 0
```

4. **Add to Hyprland config** (`~/.config/hypr/hyprland.conf`):
```
exec-once = /usr/libexec/at-spi2-registryd
env = GTK_MODULES,gail:atk-bridge
```

5. **Verify AT-SPI is working:**
```bash
# Should list running applications
python3 -c "
import gi
gi.require_version('Atspi', '2.0')
from gi.repository import Atspi
desktop = Atspi.get_desktop(0)
print(f'Found {desktop.get_child_count()} applications')
for i in range(desktop.get_child_count()):
    print(f'  - {desktop.get_child_at_index(i).get_name()}')
"
```

## Installation

### Build Dependencies

**Fedora:**
```bash
sudo dnf install libxkbcommon-devel glib2-devel cairo-devel pango-devel cairo-gobject-devel
```

**Arch Linux:**
```bash
sudo pacman -S libxkbcommon glib2 cairo pango
```

**Debian/Ubuntu:**
```bash
sudo apt install libxkbcommon-dev libglib2.0-dev libcairo2-dev libpango1.0-dev
```

### From source

```bash
git clone https://github.com/fraugho/vimium-linux
cd vimium-linux
cargo build --release

# Copy to PATH
sudo cp target/release/vimium-linux /usr/local/bin/
```

### Cargo

```bash
cargo install vimium-linux
```

## Usage

### Commands

```bash
# Click mode (default) - show hints and left-click
vimium-linux
vimium-linux click

# Right-click mode
vimium-linux right-click

# Middle-click mode
vimium-linux middle-click

# Scroll mode - select an area, then use hjkl to scroll
vimium-linux scroll

# Text mode - jump to text input fields
vimium-linux text

# Verbose output for debugging
vimium-linux -vv click
```

### Keybinding Setup

**Sway** (`~/.config/sway/config`):
```
bindsym $mod+semicolon exec vimium-linux
```

**Hyprland** (`~/.config/hypr/hyprland.conf`):
```
bind = $mainMod, semicolon, exec, vimium-linux
```

**River** (`~/.config/river/init`):
```bash
riverctl map normal $mod Semicolon spawn vimium-linux
```

### Keys (Hint Mode)

| Key | Action |
|-----|--------|
| `a-z` | Type hint characters |
| `Escape` | Cancel |
| `Backspace` | Delete last character |
| `Enter` | Confirm selection |
| `Shift` + hint | Right-click instead of left-click |
| `Ctrl` + hint | Middle-click instead of left-click |

### Keys (Scroll Mode)

| Key | Action |
|-----|--------|
| `h` / `Left` | Scroll left |
| `j` / `Down` | Scroll down |
| `k` / `Up` | Scroll up |
| `l` / `Right` | Scroll right |
| `Ctrl+d` | Page down |
| `Ctrl+u` | Page up |
| `g` | Scroll to top |
| `G` | Scroll to bottom |
| `Escape` / `q` | Exit scroll mode |

## How It Works

1. **Element Discovery** - Queries AT-SPI (Assistive Technology Service Provider Interface) for all actionable UI elements
2. **Hint Assignment** - Generates short keyboard hints for each element
3. **Overlay Display** - Creates a Wayland layer-shell surface with semi-transparent overlay
4. **Input Capture** - Takes exclusive keyboard focus
5. **Click Simulation** - Uses ydotool/wlrctl/dotool to click at element coordinates

### Supported Element Types

- Buttons (push, toggle, radio)
- Links
- Text inputs
- Checkboxes
- Menu items
- Tabs
- List/Tree items
- Combo boxes

## Troubleshooting

### "No clickable elements found"

1. Ensure the target application supports AT-SPI:
   ```bash
   # Check if AT-SPI is running
   busctl --user list | grep org.a11y
   ```

2. Some applications need accessibility explicitly enabled:
   - **Firefox**: Set `accessibility.force_disabled = 0` in about:config
   - **Chrome/Electron**: Launch with `--force-renderer-accessibility`

3. GTK applications should work out of the box. Qt applications may need:
   ```bash
   export QT_ACCESSIBILITY=1
   ```

### "No click method available"

Install one of: ydotool, wlrctl, or dotool. See Requirements section.

For ydotool, ensure the daemon is running:
```bash
sudo systemctl status ydotool
```

### Overlay doesn't appear

Your compositor must support `wlr-layer-shell-unstable-v1`. This is standard for wlroots-based compositors but not available on GNOME or KDE (yet).

### Elements appear at wrong positions

On Hyprland, the tool auto-detects monitor offsets. For other compositors, multi-monitor setups may have coordinate issues.

## Configuration

Config file at `~/.config/vimium-linux/config.toml`:

```bash
# Generate default config
vimium-linux init-config

# Show current config
vimium-linux show-config
```

```toml
[hints]
chars = "asdfghjklqwertyuiopzxcvbnm"
font_size = 14
font_family = "monospace"
padding = 4

[colors]
background = "#00000080"    # Semi-transparent dark overlay
hint_bg = "#ffffff"         # White hint boxes
hint_text = "#000000"       # Black text
hint_text_matched = "#888888"  # Gray for typed characters
input_bg = "#ffffffee"      # White input display
input_text = "#000000"      # Black input text

[behavior]
auto_select = true
exit_on_click = true
default_mode = "click"
show_element_names = false

[scroll]
scroll_step = 50
page_step = 500
smooth = true
```

## Roadmap

- [x] Basic click mode
- [x] Multi-monitor support
- [x] Configuration file
- [x] Scroll mode (hjkl scrolling)
- [x] Multi-action (right-click, middle-click)
- [x] Text input focus mode
- [ ] GNOME/KDE support (requires different layer-shell protocol)

## Contributing

Contributions welcome! Areas that need help:

- Testing on different compositors
- Multi-monitor testing
- Packaging (AUR, Nix, etc.)
- Documentation

## License

MIT

## Acknowledgments

- [Vimium](https://github.com/philc/vimium) - The original browser extension
- [Homerow](https://www.homerow.app/) - macOS inspiration
- [smithay-client-toolkit](https://github.com/Smithay/client-toolkit) - Wayland client library
- [atspi-rs](https://github.com/odilia-app/atspi) - AT-SPI bindings for Rust
