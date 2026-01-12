use anyhow::{Context, Result};
use std::io::Write;
use std::process::Command;
use std::thread;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Detect if running on Hyprland
fn is_hyprland() -> bool {
    std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok()
}

/// Get the focused monitor's offset from Hyprland
/// Returns (x_offset, y_offset) for coordinate adjustment
fn get_hyprland_monitor_offset() -> (i32, i32) {
    let output = match Command::new("hyprctl")
        .args(["monitors", "-j"])
        .output()
    {
        Ok(o) => o,
        Err(_) => return (0, 0),
    };

    let json_str = match String::from_utf8(output.stdout) {
        Ok(s) => s,
        Err(_) => return (0, 0),
    };

    // Simple JSON parsing - track current monitor's x,y and check for focused
    let mut current_x = 0i32;
    let mut current_y = 0i32;
    let mut found_focused = false;

    for line in json_str.lines() {
        let line = line.trim();

        // Reset on new monitor object
        if line == "{" {
            current_x = 0;
            current_y = 0;
        }

        // Parse x coordinate
        if line.starts_with("\"x\":") {
            if let Ok(val) = line.trim_start_matches("\"x\":").trim().trim_end_matches(',').parse::<i32>() {
                current_x = val;
            }
        }

        // Parse y coordinate
        if line.starts_with("\"y\":") {
            if let Ok(val) = line.trim_start_matches("\"y\":").trim().trim_end_matches(',').parse::<i32>() {
                current_y = val;
            }
        }

        // Check if this is the focused monitor
        if line.contains("\"focused\": true") {
            found_focused = true;
            break;
        }
    }

    if found_focused {
        debug!("Hyprland focused monitor offset: ({}, {})", current_x, current_y);
        (current_x, current_y)
    } else {
        (0, 0)
    }
}

/// Click at the given screen coordinates
/// Tries multiple methods: hyprctl (Hyprland), ydotool, wlrctl, dotool
pub fn click_at(x: i32, y: i32) -> Result<()> {
    info!("Clicking at ({}, {})", x, y);

    // Try hyprctl first (for Hyprland - handles coordinates correctly)
    if is_hyprland() {
        if try_hyprctl_click(x, y, ClickButton::Left).is_ok() {
            return Ok(());
        }
    }

    // Try ydotool (most common on Wayland)
    if try_ydotool_click(x, y, ClickButton::Left).is_ok() {
        return Ok(());
    }

    // Try wlrctl (for wlroots compositors)
    if try_wlrctl_click(x, y, ClickButton::Left).is_ok() {
        return Ok(());
    }

    // Try dotool
    if try_dotool_click(x, y, ClickButton::Left).is_ok() {
        return Ok(());
    }

    // Try wtype + cursor positioning
    if try_wtype_click(x, y, ClickButton::Left).is_ok() {
        return Ok(());
    }

    anyhow::bail!(
        "No click method available. Please install one of: ydotool, wlrctl, dotool, or wtype"
    )
}

/// Perform a right-click at the given coordinates
pub fn right_click_at(x: i32, y: i32) -> Result<()> {
    info!("Right-clicking at ({}, {})", x, y);
    perform_click(x, y, ClickButton::Right)
}

/// Perform a middle-click at the given coordinates
pub fn middle_click_at(x: i32, y: i32) -> Result<()> {
    info!("Middle-clicking at ({}, {})", x, y);
    perform_click(x, y, ClickButton::Middle)
}

/// Scroll at the given position
pub fn scroll_at(x: i32, y: i32, direction: ScrollDirection, amount: i32) -> Result<()> {
    debug!("Scrolling {:?} by {} at ({}, {})", direction, amount, x, y);

    // Try hyprctl for positioning on Hyprland
    if is_hyprland() {
        if try_hyprctl_scroll(x, y, direction, amount).is_ok() {
            return Ok(());
        }
    }

    // Try ydotool
    if try_ydotool_scroll(x, y, direction, amount).is_ok() {
        return Ok(());
    }

    // Try dotool
    if try_dotool_scroll(x, y, direction, amount).is_ok() {
        return Ok(());
    }

    // Try wlrctl
    if try_wlrctl_scroll(direction, amount).is_ok() {
        return Ok(());
    }

    anyhow::bail!("No scroll method available")
}

#[derive(Debug, Clone, Copy)]
pub enum ClickButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy)]
pub enum ScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

fn perform_click(x: i32, y: i32, button: ClickButton) -> Result<()> {
    // Try hyprctl first (for Hyprland - handles coordinates correctly)
    if is_hyprland() {
        if try_hyprctl_click(x, y, button).is_ok() {
            return Ok(());
        }
    }
    if try_ydotool_click(x, y, button).is_ok() {
        return Ok(());
    }
    if try_wlrctl_click(x, y, button).is_ok() {
        return Ok(());
    }
    if try_dotool_click(x, y, button).is_ok() {
        return Ok(());
    }
    if try_wtype_click(x, y, button).is_ok() {
        return Ok(());
    }
    anyhow::bail!("No click method available for {:?} button", button)
}

/// Try clicking using hyprctl (for Hyprland)
fn try_hyprctl_click(x: i32, y: i32, button: ClickButton) -> Result<()> {
    debug!("Trying hyprctl...");

    // Get the focused monitor's offset and apply it to coordinates
    let (offset_x, offset_y) = get_hyprland_monitor_offset();
    let adjusted_x = x + offset_x;
    let adjusted_y = y + offset_y;

    debug!("Adjusted coordinates: ({}, {}) -> ({}, {})", x, y, adjusted_x, adjusted_y);

    // Move cursor using hyprctl
    let status = Command::new("hyprctl")
        .args(["dispatch", "movecursor", &adjusted_x.to_string(), &adjusted_y.to_string()])
        .status()
        .context("Failed to run hyprctl movecursor")?;

    if !status.success() {
        anyhow::bail!("hyprctl movecursor failed");
    }

    // Small delay to ensure cursor moved
    thread::sleep(Duration::from_millis(10));

    // Click using ydotool (cursor is now in correct position)
    let button_code = match button {
        ClickButton::Left => "0xC0",
        ClickButton::Right => "0xC1",
        ClickButton::Middle => "0xC2",
    };

    let status = Command::new("ydotool")
        .args(["click", button_code])
        .status()
        .context("Failed to run ydotool click")?;

    if !status.success() {
        anyhow::bail!("ydotool click failed");
    }

    info!("Clicked using hyprctl + ydotool ({:?})", button);
    Ok(())
}

/// Try clicking using ydotool
fn try_ydotool_click(x: i32, y: i32, button: ClickButton) -> Result<()> {
    debug!("Trying ydotool...");

    // ydotool needs ydotoold daemon running
    // Move to absolute position
    let status = Command::new("ydotool")
        .args(["mousemove", "--absolute", "-x", &x.to_string(), "-y", &y.to_string()])
        .status()
        .context("Failed to run ydotool mousemove")?;

    if !status.success() {
        anyhow::bail!("ydotool mousemove failed");
    }

    // Button codes: left=0xC0, right=0xC1, middle=0xC2
    let button_code = match button {
        ClickButton::Left => "0xC0",
        ClickButton::Right => "0xC1",
        ClickButton::Middle => "0xC2",
    };

    let status = Command::new("ydotool")
        .args(["click", button_code])
        .status()
        .context("Failed to run ydotool click")?;

    if !status.success() {
        anyhow::bail!("ydotool click failed");
    }

    info!("Clicked using ydotool ({:?})", button);
    Ok(())
}

/// Try clicking using wlrctl (for wlroots compositors like Sway)
fn try_wlrctl_click(x: i32, y: i32, button: ClickButton) -> Result<()> {
    debug!("Trying wlrctl...");

    let status = Command::new("wlrctl")
        .args(["pointer", "move", &x.to_string(), &y.to_string()])
        .status()
        .context("Failed to run wlrctl")?;

    if !status.success() {
        anyhow::bail!("wlrctl move failed");
    }

    let button_name = match button {
        ClickButton::Left => "left",
        ClickButton::Right => "right",
        ClickButton::Middle => "middle",
    };

    let status = Command::new("wlrctl")
        .args(["pointer", "click", button_name])
        .status()
        .context("Failed to run wlrctl click")?;

    if !status.success() {
        anyhow::bail!("wlrctl click failed");
    }

    info!("Clicked using wlrctl ({:?})", button);
    Ok(())
}

/// Try clicking using dotool
fn try_dotool_click(x: i32, y: i32, button: ClickButton) -> Result<()> {
    debug!("Trying dotool...");

    let button_name = match button {
        ClickButton::Left => "left",
        ClickButton::Right => "right",
        ClickButton::Middle => "middle",
    };

    // dotool reads commands from stdin
    let input = format!("mouseto {} {}\nclick {}\n", x, y, button_name);

    let mut child = Command::new("dotool")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("Failed to run dotool")?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes()).context("Failed to write to dotool")?;
    }

    let status = child.wait().context("Failed to wait for dotool")?;

    if !status.success() {
        anyhow::bail!("dotool failed");
    }

    info!("Clicked using dotool ({:?})", button);
    Ok(())
}

/// Try clicking using wtype (keyboard-focused but can do mouse)
fn try_wtype_click(x: i32, y: i32, button: ClickButton) -> Result<()> {
    debug!("Trying wtype...");

    // wtype doesn't directly support mouse, but we can try via ydotool for positioning
    // This is a fallback that might work on some systems

    // First try to move cursor with ydotool (if available)
    let move_result = Command::new("ydotool")
        .args(["mousemove", "--absolute", "-x", &x.to_string(), "-y", &y.to_string()])
        .status();

    if move_result.is_err() {
        anyhow::bail!("wtype method requires ydotool for cursor positioning");
    }

    // Then click with wlrctl as fallback
    let button_name = match button {
        ClickButton::Left => "left",
        ClickButton::Right => "right",
        ClickButton::Middle => "middle",
    };

    let status = Command::new("wlrctl")
        .args(["pointer", "click", button_name])
        .status()?;

    if !status.success() {
        anyhow::bail!("wtype click failed");
    }

    info!("Clicked using wtype fallback ({:?})", button);
    Ok(())
}

/// Try scrolling using hyprctl for positioning (Hyprland)
fn try_hyprctl_scroll(x: i32, y: i32, direction: ScrollDirection, amount: i32) -> Result<()> {
    debug!("Trying hyprctl scroll...");

    // Get the focused monitor's offset and apply it to coordinates
    let (offset_x, offset_y) = get_hyprland_monitor_offset();
    let adjusted_x = x + offset_x;
    let adjusted_y = y + offset_y;

    debug!("Adjusted scroll coordinates: ({}, {}) -> ({}, {})", x, y, adjusted_x, adjusted_y);

    // Move cursor to position using hyprctl
    let status = Command::new("hyprctl")
        .args(["dispatch", "movecursor", &adjusted_x.to_string(), &adjusted_y.to_string()])
        .status()
        .context("Failed to run hyprctl movecursor")?;

    if !status.success() {
        anyhow::bail!("hyprctl movecursor failed");
    }

    thread::sleep(Duration::from_millis(10));

    // Scroll using ydotool (cursor is now in correct position)
    let (wheel_arg, wheel_amount) = match direction {
        ScrollDirection::Up => ("--wheel", amount.to_string()),
        ScrollDirection::Down => ("--wheel", (-amount).to_string()),
        ScrollDirection::Left => ("--hwheel", (-amount).to_string()),
        ScrollDirection::Right => ("--hwheel", amount.to_string()),
    };

    let status = Command::new("ydotool")
        .args(["mousemove", wheel_arg, &wheel_amount])
        .status()?;

    if !status.success() {
        anyhow::bail!("ydotool scroll failed");
    }

    Ok(())
}

/// Try scrolling using ydotool
fn try_ydotool_scroll(x: i32, y: i32, direction: ScrollDirection, amount: i32) -> Result<()> {
    debug!("Trying ydotool scroll...");

    // Move to position first
    Command::new("ydotool")
        .args(["mousemove", "--absolute", "-x", &x.to_string(), "-y", &y.to_string()])
        .status()?;

    // Scroll - ydotool uses wheel direction
    let (wheel_arg, wheel_amount) = match direction {
        ScrollDirection::Up => ("--wheel", amount.to_string()),
        ScrollDirection::Down => ("--wheel", (-amount).to_string()),
        ScrollDirection::Left => ("--hwheel", (-amount).to_string()),
        ScrollDirection::Right => ("--hwheel", amount.to_string()),
    };

    let status = Command::new("ydotool")
        .args(["mousemove", wheel_arg, &wheel_amount])
        .status()?;

    if !status.success() {
        anyhow::bail!("ydotool scroll failed");
    }

    Ok(())
}

/// Try scrolling using dotool
fn try_dotool_scroll(x: i32, y: i32, direction: ScrollDirection, amount: i32) -> Result<()> {
    debug!("Trying dotool scroll...");

    let scroll_cmd = match direction {
        ScrollDirection::Up => format!("scroll {}", amount),
        ScrollDirection::Down => format!("scroll -{}", amount),
        ScrollDirection::Left => format!("hscroll -{}", amount),
        ScrollDirection::Right => format!("hscroll {}", amount),
    };

    let input = format!("mouseto {} {}\n{}\n", x, y, scroll_cmd);

    let mut child = Command::new("dotool")
        .stdin(std::process::Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes())?;
    }

    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("dotool scroll failed");
    }

    Ok(())
}

/// Try scrolling using wlrctl
fn try_wlrctl_scroll(direction: ScrollDirection, amount: i32) -> Result<()> {
    debug!("Trying wlrctl scroll...");

    // wlrctl has limited scroll support
    let scroll_dir = match direction {
        ScrollDirection::Up => "up",
        ScrollDirection::Down => "down",
        _ => anyhow::bail!("wlrctl doesn't support horizontal scroll"),
    };

    // Repeat scroll commands for the amount
    let clicks = (amount.abs() / 15).max(1);
    for _ in 0..clicks {
        let status = Command::new("wlrctl")
            .args(["pointer", "scroll", scroll_dir])
            .status()?;

        if !status.success() {
            anyhow::bail!("wlrctl scroll failed");
        }
    }

    Ok(())
}

/// Move cursor to position without clicking
pub fn move_cursor_to(x: i32, y: i32) -> Result<()> {
    debug!("Moving cursor to ({}, {})", x, y);

    // Try hyprctl first (for Hyprland)
    if is_hyprland() {
        // Apply monitor offset for correct positioning
        let (offset_x, offset_y) = get_hyprland_monitor_offset();
        let adjusted_x = x + offset_x;
        let adjusted_y = y + offset_y;

        debug!("Adjusted cursor move: ({}, {}) -> ({}, {})", x, y, adjusted_x, adjusted_y);

        if Command::new("hyprctl")
            .args(["dispatch", "movecursor", &adjusted_x.to_string(), &adjusted_y.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return Ok(());
        }
    }

    // Try ydotool
    if Command::new("ydotool")
        .args(["mousemove", "--absolute", "-x", &x.to_string(), "-y", &y.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return Ok(());
    }

    // Try wlrctl
    if Command::new("wlrctl")
        .args(["pointer", "move", &x.to_string(), &y.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return Ok(());
    }

    // Try dotool
    let input = format!("mouseto {} {}\n", x, y);
    let mut child = Command::new("dotool")
        .stdin(std::process::Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes())?;
    }
    child.wait()?;

    Ok(())
}

/// Hold mouse button down (for drag operations)
pub fn button_down(button: ClickButton) -> Result<()> {
    let button_code = match button {
        ClickButton::Left => "0x40",   // down only
        ClickButton::Right => "0x41",
        ClickButton::Middle => "0x42",
    };

    if Command::new("ydotool")
        .args(["click", button_code])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return Ok(());
    }

    // dotool alternative
    let button_name = match button {
        ClickButton::Left => "left",
        ClickButton::Right => "right",
        ClickButton::Middle => "middle",
    };
    let input = format!("buttondown {}\n", button_name);
    let mut child = Command::new("dotool")
        .stdin(std::process::Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes())?;
    }
    child.wait()?;

    Ok(())
}

/// Release mouse button (for drag operations)
pub fn button_up(button: ClickButton) -> Result<()> {
    let button_code = match button {
        ClickButton::Left => "0x80",   // up only
        ClickButton::Right => "0x81",
        ClickButton::Middle => "0x82",
    };

    if Command::new("ydotool")
        .args(["click", button_code])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return Ok(());
    }

    // dotool alternative
    let button_name = match button {
        ClickButton::Left => "left",
        ClickButton::Right => "right",
        ClickButton::Middle => "middle",
    };
    let input = format!("buttonup {}\n", button_name);
    let mut child = Command::new("dotool")
        .stdin(std::process::Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes())?;
    }
    child.wait()?;

    Ok(())
}
