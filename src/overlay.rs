use crate::config::{parse_color, ActionMode, Config};
use crate::hints::{filter_by_prefix, find_exact_match, find_unique_match, HintedElement};
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

/// Result of the overlay selection
#[derive(Debug, Clone)]
pub enum SelectionResult {
    Selected(HintedElement, Option<ActionMode>),
    Cancelled,
}

/// Show the overlay and wait for user selection
pub async fn show_and_select(
    elements: Vec<HintedElement>,
    config: Config,
) -> Result<Option<(HintedElement, Option<ActionMode>)>> {
    let result = tokio::task::spawn_blocking(move || run_overlay(elements, config)).await??;

    match result {
        SelectionResult::Selected(elem, action) => Ok(Some((elem, action))),
        SelectionResult::Cancelled => Ok(None),
    }
}

fn run_overlay(elements: Vec<HintedElement>, config: Config) -> Result<SelectionResult> {
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
        Some("vimium-hints"),
        None,
    );

    layer_surface.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer_surface.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer_surface.set_exclusive_zone(-1);
    layer_surface.commit();

    let pool = SlotPool::new(256 * 256 * 4, &shm).context("Failed to create buffer pool")?;

    let bg_color = parse_color(&config.colors.background);
    let hint_bg_color = parse_color(&config.colors.hint_bg);
    let hint_text_color = parse_color(&config.colors.hint_text);
    let hint_matched_color = parse_color(&config.colors.hint_text_matched);
    let input_bg_color = parse_color(&config.colors.input_bg);
    let input_text_color = parse_color(&config.colors.input_text);

    let mut state = OverlayState {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        layer_surface: Some(layer_surface),
        elements,
        input_buffer: String::new(),
        result: None,
        configured: false,
        width: 0,
        height: 0,
        exit: false,
        keyboard: None,
        modifiers: Modifiers::default(),
        config,
        bg_color,
        hint_bg_color,
        hint_text_color,
        hint_matched_color,
        input_bg_color,
        input_text_color,
    };

    info!("Overlay started, waiting for input...");
    info!("Modifiers: Shift=right-click, Ctrl=middle-click");

    while !state.exit {
        event_queue
            .blocking_dispatch(&mut state)
            .context("Wayland dispatch failed")?;
    }

    state.result.ok_or_else(|| anyhow::anyhow!("No result"))
}

struct OverlayState {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer_surface: Option<LayerSurface>,
    elements: Vec<HintedElement>,
    input_buffer: String,
    result: Option<SelectionResult>,
    configured: bool,
    width: u32,
    height: u32,
    exit: bool,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    modifiers: Modifiers,
    config: Config,
    bg_color: (u8, u8, u8, u8),
    hint_bg_color: (u8, u8, u8, u8),
    hint_text_color: (u8, u8, u8, u8),
    hint_matched_color: (u8, u8, u8, u8),
    input_bg_color: (u8, u8, u8, u8),
    input_text_color: (u8, u8, u8, u8),
}

impl OverlayState {
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

        let (buffer, canvas) = match self
            .pool
            .create_buffer(width as i32, height as i32, stride as i32, wl_shm::Format::Argb8888)
        {
            Ok(b) => b,
            Err(e) => {
                debug!("Failed to create buffer: {}", e);
                return;
            }
        };

        // Clear with background color
        let (r, g, b, a) = self.bg_color;
        for pixel in canvas.chunks_exact_mut(4) {
            pixel[0] = b;
            pixel[1] = g;
            pixel[2] = r;
            pixel[3] = a;
        }

        // Draw hint labels
        let filtered = filter_by_prefix(&self.elements, &self.input_buffer);
        let prefix_len = self.input_buffer.len();
        let padding = self.config.hints.padding;

        for elem in &filtered {
            draw_hint(
                canvas,
                width,
                height,
                elem,
                prefix_len,
                padding,
                self.hint_bg_color,
                self.hint_text_color,
                self.hint_matched_color,
            );
        }

        // Draw input display
        draw_input_display(
            canvas,
            width,
            height,
            &self.input_buffer,
            self.input_bg_color,
            self.input_text_color,
        );

        // Draw modifier indicator
        let mode_text = if self.modifiers.shift {
            "Mode: Right-Click"
        } else if self.modifiers.ctrl {
            "Mode: Middle-Click"
        } else {
            "Mode: Click"
        };
        draw_modifier_indicator(
            canvas,
            width,
            height,
            mode_text,
            self.input_bg_color,
            self.input_text_color,
        );

        layer_surface.wl_surface().attach(Some(buffer.wl_buffer()), 0, 0);
        layer_surface.wl_surface().damage_buffer(0, 0, width as i32, height as i32);
        layer_surface.commit();
    }

    fn get_action_from_modifiers(&self) -> Option<ActionMode> {
        if self.modifiers.shift {
            Some(ActionMode::RightClick)
        } else if self.modifiers.ctrl {
            Some(ActionMode::MiddleClick)
        } else {
            None
        }
    }

    fn select_element(&mut self, elem: &HintedElement) {
        let action = self.get_action_from_modifiers();
        info!("Selected: {} ({}) with action {:?}", elem.hint, elem.element.name, action);
        self.result = Some(SelectionResult::Selected(elem.clone(), action));
        self.exit = true;
    }

    fn handle_key(&mut self, key: Keysym) {
        match key {
            Keysym::Escape => {
                info!("Escape pressed, cancelling");
                self.result = Some(SelectionResult::Cancelled);
                self.exit = true;
            }
            Keysym::BackSpace => {
                self.input_buffer.pop();
                debug!("Backspace, input now: {}", self.input_buffer);
            }
            Keysym::Return => {
                let selected = find_exact_match(&self.elements, &self.input_buffer)
                    .or_else(|| find_unique_match(&self.elements, &self.input_buffer))
                    .cloned();

                if let Some(elem) = selected {
                    self.select_element(&elem);
                }
            }
            _ => {
                if let Some(ch) = keysym_to_char(key) {
                    self.input_buffer.push(ch);
                    debug!("Key pressed: {}, input now: {}", ch, self.input_buffer);

                    if self.config.behavior.auto_select {
                        let selected = find_exact_match(&self.elements, &self.input_buffer).cloned();
                        if let Some(elem) = selected {
                            self.select_element(&elem);
                        }
                    }
                }
            }
        }
    }
}

// Standalone drawing functions to avoid borrow checker issues

fn draw_hint(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    elem: &HintedElement,
    prefix_len: usize,
    padding: u32,
    hint_bg_color: (u8, u8, u8, u8),
    hint_text_color: (u8, u8, u8, u8),
    hint_matched_color: (u8, u8, u8, u8),
) {
    let x = elem.element.x as u32;
    let y = elem.element.y as u32;

    let char_width = 8u32;
    let char_height = 12u32;
    let box_width: u32 = padding * 2 + (elem.hint.len() as u32 * char_width);
    let box_height: u32 = padding * 2 + char_height;

    let hint_chars: Vec<char> = elem.hint.chars().collect();

    // Draw background
    let (hr, hg, hb, ha) = hint_bg_color;
    for dy in 0..box_height {
        for dx in 0..box_width {
            let px = x.saturating_add(dx);
            let py = y.saturating_add(dy);

            if px < width && py < height {
                let idx = ((py * width + px) * 4) as usize;
                if idx + 3 < canvas.len() {
                    canvas[idx] = hb;
                    canvas[idx + 1] = hg;
                    canvas[idx + 2] = hr;
                    canvas[idx + 3] = ha;
                }
            }
        }
    }

    // Draw text
    for (i, ch) in hint_chars.iter().enumerate() {
        let char_x = x + padding + (i as u32 * char_width);
        let char_y = y + padding;

        let (r, g, b) = if i < prefix_len {
            let (r, g, b, _) = hint_matched_color;
            (r, g, b)
        } else {
            let (r, g, b, _) = hint_text_color;
            (r, g, b)
        };

        draw_char(canvas, width, height, char_x, char_y, *ch, r, g, b);
    }
}

fn draw_input_display(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    input_buffer: &str,
    bg_color: (u8, u8, u8, u8),
    text_color: (u8, u8, u8, u8),
) {
    let text = format!("Input: {}_", input_buffer);
    let box_width = 250u32;
    let box_height = 30u32;
    let start_x = 10u32;
    let start_y = 10u32;

    let (ir, ig, ib, ia) = bg_color;
    for dy in 0..box_height {
        for dx in 0..box_width {
            let px = start_x + dx;
            let py = start_y + dy;
            if px < width && py < height {
                let idx = ((py * width + px) * 4) as usize;
                if idx + 3 < canvas.len() {
                    canvas[idx] = ib;
                    canvas[idx + 1] = ig;
                    canvas[idx + 2] = ir;
                    canvas[idx + 3] = ia;
                }
            }
        }
    }

    let (tr, tg, tb, _) = text_color;
    for (i, ch) in text.chars().enumerate() {
        draw_char(canvas, width, height, start_x + 10 + (i as u32 * 8), start_y + 8, ch, tr, tg, tb);
    }
}

fn draw_modifier_indicator(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    mode_text: &str,
    bg_color: (u8, u8, u8, u8),
    text_color: (u8, u8, u8, u8),
) {
    let box_width = 180u32;
    let box_height = 25u32;
    let start_x = 270u32;
    let start_y = 10u32;

    let (ir, ig, ib, ia) = bg_color;
    for dy in 0..box_height {
        for dx in 0..box_width {
            let px = start_x + dx;
            let py = start_y + dy;
            if px < width && py < height {
                let idx = ((py * width + px) * 4) as usize;
                if idx + 3 < canvas.len() {
                    canvas[idx] = ib;
                    canvas[idx + 1] = ig;
                    canvas[idx + 2] = ir;
                    canvas[idx + 3] = ia;
                }
            }
        }
    }

    let (tr, tg, tb, _) = text_color;
    for (i, ch) in mode_text.chars().enumerate() {
        draw_char(canvas, width, height, start_x + 10 + (i as u32 * 8), start_y + 6, ch, tr, tg, tb);
    }
}

fn draw_char(canvas: &mut [u8], width: u32, height: u32, x: u32, y: u32, ch: char, r: u8, g: u8, b: u8) {
    let bitmap = get_char_bitmap(ch);

    for (row, &bits) in bitmap.iter().enumerate() {
        for col in 0..6 {
            if (bits >> (5 - col)) & 1 == 1 {
                for sy in 0..2 {
                    let px = x + col;
                    let py = y + (row as u32 * 2) + sy;

                    if px < width && py < height {
                        let idx = ((py * width + px) * 4) as usize;
                        if idx + 3 < canvas.len() {
                            canvas[idx] = b;
                            canvas[idx + 1] = g;
                            canvas[idx + 2] = r;
                            canvas[idx + 3] = 255;
                        }
                    }
                }
            }
        }
    }
}

fn keysym_to_char(key: Keysym) -> Option<char> {
    match key {
        Keysym::a => Some('a'),
        Keysym::b => Some('b'),
        Keysym::c => Some('c'),
        Keysym::d => Some('d'),
        Keysym::e => Some('e'),
        Keysym::f => Some('f'),
        Keysym::g => Some('g'),
        Keysym::h => Some('h'),
        Keysym::i => Some('i'),
        Keysym::j => Some('j'),
        Keysym::k => Some('k'),
        Keysym::l => Some('l'),
        Keysym::m => Some('m'),
        Keysym::n => Some('n'),
        Keysym::o => Some('o'),
        Keysym::p => Some('p'),
        Keysym::q => Some('q'),
        Keysym::r => Some('r'),
        Keysym::s => Some('s'),
        Keysym::t => Some('t'),
        Keysym::u => Some('u'),
        Keysym::v => Some('v'),
        Keysym::w => Some('w'),
        Keysym::x => Some('x'),
        Keysym::y => Some('y'),
        Keysym::z => Some('z'),
        Keysym::_0 => Some('0'),
        Keysym::_1 => Some('1'),
        Keysym::_2 => Some('2'),
        Keysym::_3 => Some('3'),
        Keysym::_4 => Some('4'),
        Keysym::_5 => Some('5'),
        Keysym::_6 => Some('6'),
        Keysym::_7 => Some('7'),
        Keysym::_8 => Some('8'),
        Keysym::_9 => Some('9'),
        Keysym::semicolon => Some(';'),
        _ => None,
    }
}

fn get_char_bitmap(ch: char) -> [u8; 6] {
    match ch.to_ascii_lowercase() {
        'a' => [0b011100, 0b100010, 0b111110, 0b100010, 0b100010, 0b000000],
        'b' => [0b111100, 0b100010, 0b111100, 0b100010, 0b111100, 0b000000],
        'c' => [0b011110, 0b100000, 0b100000, 0b100000, 0b011110, 0b000000],
        'd' => [0b111100, 0b100010, 0b100010, 0b100010, 0b111100, 0b000000],
        'e' => [0b111110, 0b100000, 0b111100, 0b100000, 0b111110, 0b000000],
        'f' => [0b111110, 0b100000, 0b111100, 0b100000, 0b100000, 0b000000],
        'g' => [0b011110, 0b100000, 0b100110, 0b100010, 0b011110, 0b000000],
        'h' => [0b100010, 0b100010, 0b111110, 0b100010, 0b100010, 0b000000],
        'i' => [0b011100, 0b001000, 0b001000, 0b001000, 0b011100, 0b000000],
        'j' => [0b000010, 0b000010, 0b000010, 0b100010, 0b011100, 0b000000],
        'k' => [0b100010, 0b100100, 0b111000, 0b100100, 0b100010, 0b000000],
        'l' => [0b100000, 0b100000, 0b100000, 0b100000, 0b111110, 0b000000],
        'm' => [0b100010, 0b110110, 0b101010, 0b100010, 0b100010, 0b000000],
        'n' => [0b100010, 0b110010, 0b101010, 0b100110, 0b100010, 0b000000],
        'o' => [0b011100, 0b100010, 0b100010, 0b100010, 0b011100, 0b000000],
        'p' => [0b111100, 0b100010, 0b111100, 0b100000, 0b100000, 0b000000],
        'q' => [0b011100, 0b100010, 0b100010, 0b011100, 0b000010, 0b000000],
        'r' => [0b111100, 0b100010, 0b111100, 0b100100, 0b100010, 0b000000],
        's' => [0b011110, 0b100000, 0b011100, 0b000010, 0b111100, 0b000000],
        't' => [0b111110, 0b001000, 0b001000, 0b001000, 0b001000, 0b000000],
        'u' => [0b100010, 0b100010, 0b100010, 0b100010, 0b011100, 0b000000],
        'v' => [0b100010, 0b100010, 0b100010, 0b010100, 0b001000, 0b000000],
        'w' => [0b100010, 0b100010, 0b101010, 0b110110, 0b100010, 0b000000],
        'x' => [0b100010, 0b010100, 0b001000, 0b010100, 0b100010, 0b000000],
        'y' => [0b100010, 0b010100, 0b001000, 0b001000, 0b001000, 0b000000],
        'z' => [0b111110, 0b000100, 0b001000, 0b010000, 0b111110, 0b000000],
        '0' => [0b011100, 0b100110, 0b101010, 0b110010, 0b011100, 0b000000],
        '1' => [0b001000, 0b011000, 0b001000, 0b001000, 0b011100, 0b000000],
        '2' => [0b011100, 0b100010, 0b001100, 0b010000, 0b111110, 0b000000],
        '3' => [0b111100, 0b000010, 0b011100, 0b000010, 0b111100, 0b000000],
        '4' => [0b100010, 0b100010, 0b111110, 0b000010, 0b000010, 0b000000],
        '5' => [0b111110, 0b100000, 0b111100, 0b000010, 0b111100, 0b000000],
        '6' => [0b011100, 0b100000, 0b111100, 0b100010, 0b011100, 0b000000],
        '7' => [0b111110, 0b000010, 0b000100, 0b001000, 0b001000, 0b000000],
        '8' => [0b011100, 0b100010, 0b011100, 0b100010, 0b011100, 0b000000],
        '9' => [0b011100, 0b100010, 0b011110, 0b000010, 0b011100, 0b000000],
        ':' => [0b000000, 0b001000, 0b000000, 0b001000, 0b000000, 0b000000],
        ';' => [0b000000, 0b001000, 0b000000, 0b001000, 0b010000, 0b000000],
        '_' => [0b000000, 0b000000, 0b000000, 0b000000, 0b111110, 0b000000],
        '-' => [0b000000, 0b000000, 0b111110, 0b000000, 0b000000, 0b000000],
        ' ' => [0b000000, 0b000000, 0b000000, 0b000000, 0b000000, 0b000000],
        '.' => [0b000000, 0b000000, 0b000000, 0b000000, 0b001000, 0b000000],
        _ => [0b111110, 0b100010, 0b100010, 0b100010, 0b111110, 0b000000],
    }
}

// Handler implementations

impl CompositorHandler for OverlayState {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wayland_client::protocol::wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.draw(qh);
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for OverlayState {
    fn output_state(&mut self) -> &mut OutputState { &mut self.output_state }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for OverlayState {
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

impl SeatHandler for OverlayState {
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

impl KeyboardHandler for OverlayState {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}
    fn press_key(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        self.handle_key(event.keysym);
        self.draw(qh);
    }
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn update_modifiers(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, modifiers: Modifiers, _: u32) {
        self.modifiers = modifiers;
        self.draw(qh);
    }
}

impl PointerHandler for OverlayState {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, _: &[PointerEvent]) {}
}

impl ShmHandler for OverlayState {
    fn shm_state(&mut self) -> &mut Shm { &mut self.shm }
}

impl ProvidesRegistryState for OverlayState {
    fn registry(&mut self) -> &mut RegistryState { &mut self.registry_state }
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(OverlayState);
delegate_output!(OverlayState);
delegate_shm!(OverlayState);
delegate_seat!(OverlayState);
delegate_keyboard!(OverlayState);
delegate_pointer!(OverlayState);
delegate_layer!(OverlayState);
delegate_registry!(OverlayState);
