use anyhow::{Context, Result};
use atspi::proxy::component::ComponentProxy;
use atspi::Role;
use std::collections::HashSet;
use tracing::{debug, info, warn};
use zbus::{Address, Connection};

/// Represents a clickable UI element with screen coordinates
#[derive(Debug, Clone)]
pub struct ClickableElement {
    pub name: String,
    pub role: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl ClickableElement {
    /// Get the center point of the element (for clicking)
    pub fn center(&self) -> (i32, i32) {
        (self.x + self.width / 2, self.y + self.height / 2)
    }
}

/// Roles that are typically clickable/actionable
fn is_actionable_role(role: Role) -> bool {
    matches!(
        role,
        Role::PushButton
            | Role::ToggleButton
            | Role::RadioButton
            | Role::CheckBox
            | Role::MenuItem
            | Role::Link
            | Role::Entry
            | Role::PasswordText
            | Role::ComboBox
            | Role::PageTab
            | Role::ListItem
            | Role::TreeItem
            | Role::Icon
            | Role::SpinButton
            | Role::Slider
            | Role::TableCell
    )
}

/// Roles that are scrollable containers
fn is_scrollable_role(role: Role) -> bool {
    matches!(
        role,
        Role::ScrollPane
            | Role::Viewport
            | Role::Panel
            | Role::Filler
            | Role::DocumentFrame
            | Role::DocumentWeb
            | Role::Application
            | Role::Frame
            | Role::ScrollBar
    )
}

/// Roles that are text input fields
fn is_text_input_role(role: Role) -> bool {
    matches!(
        role,
        Role::Entry
            | Role::PasswordText
            | Role::SpinButton
            | Role::ComboBox
            | Role::Terminal
    )
}

/// Query AT-SPI for all clickable elements
pub async fn get_clickable_elements() -> Result<Vec<ClickableElement>> {
    collect_elements(|role| is_actionable_role(role)).await
}

/// Query AT-SPI for scrollable elements
pub async fn get_scrollable_elements() -> Result<Vec<ClickableElement>> {
    collect_elements(|role| is_scrollable_role(role)).await
}

/// Query AT-SPI for text input elements
pub async fn get_text_elements() -> Result<Vec<ClickableElement>> {
    collect_elements(|role| is_text_input_role(role)).await
}

/// Get the accessibility bus connection
async fn get_a11y_connection() -> Result<Connection> {
    // First, try to get the a11y bus address from the session bus
    let session_bus = Connection::session()
        .await
        .context("Failed to connect to session bus")?;

    // Try to get the address from org.a11y.Bus
    let bus_proxy = atspi::proxy::bus::BusProxy::new(&session_bus).await;

    if let Ok(proxy) = bus_proxy {
        if let Ok(addr_str) = proxy.get_address().await {
            debug!("Got a11y bus address: {}", addr_str);
            if let Ok(addr) = addr_str.parse::<Address>() {
                if let Ok(conn) = zbus::ConnectionBuilder::address(addr)?.build().await {
                    info!("Connected to accessibility bus via org.a11y.Bus");
                    return Ok(conn);
                }
            }
        }
    }

    // Fallback: try connecting directly to the socket
    // Use XDG_RUNTIME_DIR which is /run/user/<uid>
    let socket_path = if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        format!("unix:path={}/at-spi/bus_0", runtime_dir)
    } else {
        // Fallback to uid-based path
        let uid = std::process::id(); // This is PID, not UID, but we'll try common paths
        format!("unix:path=/run/user/1000/at-spi/bus_0") // Common default
    };
    debug!("Trying direct socket connection: {}", socket_path);

    if let Ok(addr) = socket_path.parse::<Address>() {
        if let Ok(conn) = zbus::ConnectionBuilder::address(addr)?.build().await {
            info!("Connected to accessibility bus via direct socket");
            return Ok(conn);
        }
    }

    // Last resort: try session bus directly (won't work for most setups)
    warn!("Could not connect to accessibility bus, falling back to session bus");
    Ok(session_bus)
}

/// Collect elements from AT-SPI
async fn collect_elements<F>(role_filter: F) -> Result<Vec<ClickableElement>>
where
    F: Fn(Role) -> bool + Send + Sync + 'static,
{
    // Connect to the accessibility bus
    let conn = get_a11y_connection()
        .await
        .context("Failed to connect to accessibility bus")?;

    let mut elements = Vec::new();
    let mut visited = HashSet::new();

    // Get the registry proxy (root of AT-SPI tree)
    let registry = atspi::proxy::accessible::AccessibleProxy::builder(&conn)
        .destination("org.a11y.atspi.Registry")?
        .path("/org/a11y/atspi/accessible/root")?
        .build()
        .await
        .context("Failed to connect to AT-SPI registry")?;

    // Get all children (applications) from the registry
    let children = match registry.get_children().await {
        Ok(kids) => kids,
        Err(e) => {
            warn!("Failed to get desktop children: {}", e);
            return Ok(elements);
        }
    };

    debug!("Desktop has {} children (applications)", children.len());

    // Iterate through applications
    for app_ref in children {
        let dest = app_ref.name.to_string();
        let path = app_ref.path.to_string();

        collect_from_accessible(
            &conn,
            &dest,
            &path,
            &mut elements,
            &mut visited,
            0,
            &role_filter,
        )
        .await;
    }

    debug!("Found {} total elements", elements.len());
    Ok(elements)
}

/// Recursively collect elements from an accessible
async fn collect_from_accessible<F>(
    conn: &Connection,
    dest: &str,
    path: &str,
    elements: &mut Vec<ClickableElement>,
    visited: &mut HashSet<String>,
    depth: usize,
    role_filter: &F,
) where
    F: Fn(Role) -> bool,
{
    const MAX_DEPTH: usize = 20;
    const MAX_ELEMENTS: usize = 500;

    if depth > MAX_DEPTH || elements.len() >= MAX_ELEMENTS {
        return;
    }

    let key = format!("{}:{}", dest, path);
    if visited.contains(&key) {
        return;
    }
    visited.insert(key.clone());

    // Create a proxy for this accessible
    let proxy = match atspi::proxy::accessible::AccessibleProxy::builder(conn)
        .destination(dest)
        .and_then(|b| b.path(path))
    {
        Ok(builder) => match builder.build().await {
            Ok(p) => p,
            Err(_) => return,
        },
        Err(_) => return,
    };

    // Get role
    let role = match proxy.get_role().await {
        Ok(r) => r,
        Err(_) => return,
    };

    // Check if element matches filter
    if role_filter(role) {
        // Try to get extents using the Component interface
        // Create a ComponentProxy for the same object to access Component interface
        if let Ok(component) = ComponentProxy::builder(conn)
            .destination(dest)
            .and_then(|b| b.path(path))
        {
            if let Ok(component) = component.build().await {
                if let Ok((x, y, w, h)) = component.get_extents(atspi::CoordType::Screen).await {
                    // Skip elements with no size or off-screen
                    if w > 0 && h > 0 && x >= 0 && y >= 0 {
                        // Skip very large elements (backgrounds)
                        if w < 3000 && h < 2000 {
                            let name = proxy.name().await.unwrap_or_default();

                            elements.push(ClickableElement {
                                name: name.clone(),
                                role: format!("{:?}", role),
                                x,
                                y,
                                width: w,
                                height: h,
                            });

                            debug!(
                                "Found element: {} ({:?}) at ({}, {}) {}x{}",
                                name, role, x, y, w, h
                            );
                        }
                    }
                }
            }
        }
    }

    // Recurse into children
    if let Ok(children) = proxy.get_children().await {
        for child_ref in children {
            let child_dest = child_ref.name.to_string();
            let child_path = child_ref.path.to_string();

            Box::pin(collect_from_accessible(
                conn,
                &child_dest,
                &child_path,
                elements,
                visited,
                depth + 1,
                role_filter,
            ))
            .await;
        }
    }
}
