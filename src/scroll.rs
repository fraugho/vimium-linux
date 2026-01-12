use crate::click::{scroll_at, ScrollDirection};
use crate::config::Config;
use anyhow::{Context, Result};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers},
        pointer::{PointerEvent, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use tracing::{debug, info};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};

pub async fn run_scroll_mode(x: i32, y: i32, config: &Config) -> Result<()> {
    let config = config.clone();
    tokio::task::spawn_blocking(move || run_scroll_overlay(x, y, &config)).await??;
    Ok(())
}

fn run_scroll_overlay(target_x: i32, target_y: i32, config: &Config) -> Result<()> {
    let conn = Connection::connect_to_env().context("Failed to connect to Wayland")?;

    let (globals, mut event_queue) =
        registry_queue_init(&conn).context("Failed to init registry")?;
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).context("wl_compositor not available")?;
    let layer_shell = LayerShell::bind(&globals, &qh).context("layer_shell not available")?;
    let shm = Shm::bind(&globals, &qh).context("wl_shm not available")?;

    let surface = compositor.create_surface(&qh);

    let layer_surface = layer_shell.create_layer_surface(
        &qh,
        surface,
        Layer::Overlay,
        Some("vimium-scroll"),
        None,
    );

    layer_surface.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer_surface.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer_surface.set_exclusive_zone(-1);
    layer_surface.commit();

    let pool = SlotPool::new(256 * 256 * 4, &shm).context("Failed to create buffer pool")?;

    let mut state = ScrollState {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        layer_surface: Some(layer_surface),
        target_x,
        target_y,
        scroll_step: config.scroll.scroll_step,
        page_step: config.scroll.page_step,
        configured: false,
        width: 0,
        height: 0,
        exit: false,
        keyboard: None,
        modifiers: Modifiers::default(),
    };

    info!("Scroll mode started at ({}, {}). Use hjkl to scroll, Escape to exit.", target_x, target_y);

    while !state.exit {
        event_queue.blocking_dispatch(&mut state).context("Wayland dispatch failed")?;
    }

    Ok(())
}

struct ScrollState {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer_surface: Option<LayerSurface>,
    target_x: i32,
    target_y: i32,
    scroll_step: i32,
    page_step: i32,
    configured: bool,
    width: u32,
    height: u32,
    exit: bool,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    modifiers: Modifiers,
}

impl ScrollState {
    fn draw(&mut self, _qh: &QueueHandle<Self>) {
        if !self.configured || self.width == 0 || self.height == 0 {
            return;
        }

        let layer_surface = match &self.layer_surface {
            Some(ls) => ls,
            None => return,
        };

        let width = self.width;
        let height = self.height;
        let stride = width * 4;

        let (buffer, canvas) = match self.pool.create_buffer(
            width as i32, height as i32, stride as i32, wl_shm::Format::Argb8888
        ) {
            Ok(b) => b,
            Err(_) => return,
        };

        // Very transparent background
        for pixel in canvas.chunks_exact_mut(4) {
            pixel[0] = 0;
            pixel[1] = 0;
            pixel[2] = 0;
            pixel[3] = 50;
        }

        // Draw crosshair at target position
        let tx = self.target_x as u32;
        let ty = self.target_y as u32;

        // Horizontal line
        if ty < height {
            for x in tx.saturating_sub(20)..=(tx + 20).min(width - 1) {
                let idx = ((ty * width + x) * 4) as usize;
                if idx + 3 < canvas.len() {
                    canvas[idx] = 0;
                    canvas[idx + 1] = 255;
                    canvas[idx + 2] = 0;
                    canvas[idx + 3] = 255;
                }
            }
        }

        // Vertical line
        for y in ty.saturating_sub(20)..=(ty + 20).min(height - 1) {
            let idx = ((y * width + tx) * 4) as usize;
            if idx + 3 < canvas.len() {
                canvas[idx] = 0;
                canvas[idx + 1] = 255;
                canvas[idx + 2] = 0;
                canvas[idx + 3] = 255;
            }
        }

        // Draw help bar at top
        draw_help_bar(canvas, width, height);

        layer_surface.wl_surface().attach(Some(buffer.wl_buffer()), 0, 0);
        layer_surface.wl_surface().damage_buffer(0, 0, width as i32, height as i32);
        layer_surface.commit();
    }

    fn handle_key(&mut self, key: Keysym) {
        let step = if self.modifiers.ctrl {
            self.page_step
        } else {
            self.scroll_step
        };

        match key {
            Keysym::Escape | Keysym::q => {
                info!("Exiting scroll mode");
                self.exit = true;
            }
            Keysym::h | Keysym::Left => {
                debug!("Scroll left");
                let _ = scroll_at(self.target_x, self.target_y, ScrollDirection::Left, step);
            }
            Keysym::j | Keysym::Down => {
                debug!("Scroll down");
                let _ = scroll_at(self.target_x, self.target_y, ScrollDirection::Down, step);
            }
            Keysym::k | Keysym::Up => {
                debug!("Scroll up");
                let _ = scroll_at(self.target_x, self.target_y, ScrollDirection::Up, step);
            }
            Keysym::l | Keysym::Right => {
                debug!("Scroll right");
                let _ = scroll_at(self.target_x, self.target_y, ScrollDirection::Right, step);
            }
            Keysym::d if self.modifiers.ctrl => {
                debug!("Page down");
                let _ = scroll_at(self.target_x, self.target_y, ScrollDirection::Down, self.page_step);
            }
            Keysym::u if self.modifiers.ctrl => {
                debug!("Page up");
                let _ = scroll_at(self.target_x, self.target_y, ScrollDirection::Up, self.page_step);
            }
            Keysym::g => {
                debug!("Scroll to top");
                let _ = scroll_at(self.target_x, self.target_y, ScrollDirection::Up, 10000);
            }
            Keysym::G => {
                debug!("Scroll to bottom");
                let _ = scroll_at(self.target_x, self.target_y, ScrollDirection::Down, 10000);
            }
            _ => {}
        }
    }
}

fn draw_help_bar(canvas: &mut [u8], width: u32, height: u32) {
    let box_height = 25u32;
    let box_width = 400u32.min(width);

    for dy in 0..box_height {
        for dx in 0..box_width {
            if dy < height {
                let idx = ((dy * width + dx) * 4) as usize;
                if idx + 3 < canvas.len() {
                    canvas[idx] = 40;
                    canvas[idx + 1] = 40;
                    canvas[idx + 2] = 40;
                    canvas[idx + 3] = 230;
                }
            }
        }
    }
}

impl CompositorHandler for ScrollState {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wayland_client::protocol::wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.draw(qh);
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for ScrollState {
    fn output_state(&mut self) -> &mut OutputState { &mut self.output_state }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for ScrollState {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }

    fn configure(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &LayerSurface, configure: LayerSurfaceConfigure, _: u32) {
        self.width = configure.new_size.0;
        self.height = configure.new_size.1;
        self.configured = true;

        let size = (self.width * self.height * 4) as usize;
        if self.pool.len() < size {
            self.pool.resize(size).ok();
        }

        self.draw(qh);
    }
}

impl SeatHandler for ScrollState {
    fn seat_state(&mut self) -> &mut SeatState { &mut self.seat_state }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self.seat_state.get_keyboard(qh, &seat, None).ok();
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Keyboard { self.keyboard = None; }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for ScrollState {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}
    fn press_key(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        self.handle_key(event.keysym);
        self.draw(qh);
    }
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, modifiers: Modifiers, _: u32) {
        self.modifiers = modifiers;
    }
}

impl PointerHandler for ScrollState {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, _: &[PointerEvent]) {}
}

impl ShmHandler for ScrollState {
    fn shm_state(&mut self) -> &mut Shm { &mut self.shm }
}

impl ProvidesRegistryState for ScrollState {
    fn registry(&mut self) -> &mut RegistryState { &mut self.registry_state }
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(ScrollState);
delegate_output!(ScrollState);
delegate_shm!(ScrollState);
delegate_seat!(ScrollState);
delegate_keyboard!(ScrollState);
delegate_pointer!(ScrollState);
delegate_layer!(ScrollState);
delegate_registry!(ScrollState);
