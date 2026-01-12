mod atspi;
mod click;
mod config;
mod hints;
mod overlay;
mod scroll;

use anyhow::Result;
use clap::{Parser, Subcommand};
use config::{ActionMode, Config};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "vimium-linux")]
#[command(author, version, about = "Keyboard-driven navigation for Wayland", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Path to config file
    #[arg(short, long)]
    config: Option<String>,

    /// Verbose output (can be repeated: -v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

#[derive(Subcommand)]
enum Commands {
    /// Click mode - show hints and click selected element (default)
    Click {
        /// Filter by element role (button, link, input, etc.)
        #[arg(short, long)]
        filter: Option<String>,
    },
    /// Right-click mode
    RightClick {
        #[arg(short, long)]
        filter: Option<String>,
    },
    /// Middle-click mode
    MiddleClick {
        #[arg(short, long)]
        filter: Option<String>,
    },
    /// Scroll mode - select area then use hjkl to scroll
    Scroll,
    /// Text mode - jump to and focus text input fields
    Text,
    /// Generate default config file
    InitConfig,
    /// Show current config
    ShowConfig,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let log_level = match cli.verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter = format!("vimium_linux={}", log_level);
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(filter.parse()?))
        .init();

    // Load config
    let config = if let Some(path) = cli.config {
        Config::load_from_path(path.into())?
    } else {
        Config::load()
    };

    info!("vimium-linux starting...");

    match cli.command {
        Some(Commands::InitConfig) => {
            Config::default().save()?;
            println!("Config file created at: {:?}", Config::config_path());
            return Ok(());
        }
        Some(Commands::ShowConfig) => {
            println!("{}", toml::to_string_pretty(&config)?);
            return Ok(());
        }
        Some(Commands::Click { filter }) => {
            run_click_mode(&config, ActionMode::Click, filter.as_deref()).await?;
        }
        Some(Commands::RightClick { filter }) => {
            run_click_mode(&config, ActionMode::RightClick, filter.as_deref()).await?;
        }
        Some(Commands::MiddleClick { filter }) => {
            run_click_mode(&config, ActionMode::MiddleClick, filter.as_deref()).await?;
        }
        Some(Commands::Scroll) => {
            run_scroll_mode(&config).await?;
        }
        Some(Commands::Text) => {
            run_text_mode(&config).await?;
        }
        None => {
            // Default to click mode
            run_click_mode(&config, config.behavior.default_mode, None).await?;
        }
    }

    info!("vimium-linux done");
    Ok(())
}

/// Run click mode with hints
async fn run_click_mode(config: &Config, action: ActionMode, filter: Option<&str>) -> Result<()> {
    // 1. Query AT-SPI for clickable elements
    let mut elements = atspi::get_clickable_elements().await?;
    info!("Found {} clickable elements", elements.len());

    // Apply filter if specified
    if let Some(role_filter) = filter {
        let role_filter = role_filter.to_lowercase();
        elements.retain(|e| e.role.to_lowercase().contains(&role_filter));
        info!("After filtering: {} elements", elements.len());
    }

    if elements.is_empty() {
        warn!("No clickable elements found");
        println!("No clickable elements found. Make sure:");
        println!("  - The target application supports AT-SPI accessibility");
        println!("  - For Firefox: set accessibility.force_disabled = 0 in about:config");
        println!("  - For Chrome/Electron: launch with --force-renderer-accessibility");
        return Ok(());
    }

    // 2. Generate hints for elements
    let hinted_elements = hints::assign_hints(&elements, &config.hints.chars);

    // 3. Show overlay and wait for user input
    let result = overlay::show_and_select(hinted_elements, config.clone()).await?;

    // 4. Perform action on selected element
    if let Some((element, modifier_action)) = result {
        let (x, y) = element.click_position();

        // Modifier overrides the mode
        let final_action = modifier_action.unwrap_or(action);

        match final_action {
            ActionMode::Click => {
                info!("Clicking element at ({}, {})", x, y);
                click::click_at(x, y)?;
            }
            ActionMode::RightClick => {
                info!("Right-clicking element at ({}, {})", x, y);
                click::right_click_at(x, y)?;
            }
            ActionMode::MiddleClick => {
                info!("Middle-clicking element at ({}, {})", x, y);
                click::middle_click_at(x, y)?;
            }
            _ => {
                click::click_at(x, y)?;
            }
        }
    }

    Ok(())
}

/// Run scroll mode - select a scrollable area then scroll with hjkl
async fn run_scroll_mode(config: &Config) -> Result<()> {
    // Get scrollable elements
    let elements = atspi::get_scrollable_elements().await?;
    info!("Found {} scrollable elements", elements.len());

    if elements.is_empty() {
        warn!("No scrollable elements found");
        println!("No scrollable elements found.");
        return Ok(());
    }

    let hinted_elements = hints::assign_hints(&elements, &config.hints.chars);
    let result = overlay::show_and_select(hinted_elements, config.clone()).await?;

    if let Some((element, _)) = result {
        let (x, y) = element.click_position();
        // Enter scroll mode at this position
        scroll::run_scroll_mode(x, y, config).await?;
    }

    Ok(())
}

/// Run text input mode - focus on text fields
async fn run_text_mode(config: &Config) -> Result<()> {
    // Get only text input elements
    let elements = atspi::get_text_elements().await?;
    info!("Found {} text input elements", elements.len());

    if elements.is_empty() {
        warn!("No text input elements found");
        println!("No text input fields found.");
        return Ok(());
    }

    let hinted_elements = hints::assign_hints(&elements, &config.hints.chars);
    let result = overlay::show_and_select(hinted_elements, config.clone()).await?;

    if let Some((element, _)) = result {
        let (x, y) = element.click_position();
        // Click to focus the text field
        click::click_at(x, y)?;
    }

    Ok(())
}
