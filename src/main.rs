// based on https://github.com/Smithay/client-toolkit/blob/master/examples/simple_layer.rs

use std::error::Error;
use smithay_client_toolkit::{
    shm::{Shm, ShmHandler},
    shell::wlr_layer::{Layer, LayerShell, Anchor, KeyboardInteractivity, LayerSurface, LayerShellHandler, LayerSurfaceConfigure},
    shell::WaylandSurface,
    seat::pointer::{PointerEvent, PointerHandler},
    seat::{Capability, SeatHandler, SeatState},
    registry::{ProvidesRegistryState, RegistryState},
    output::{OutputHandler, OutputState},
    delegate_compositor,
    delegate_layer,
    delegate_output,
    delegate_pointer,
    delegate_registry,
    delegate_seat,
    delegate_shm,
    registry_handlers,
    compositor::{CompositorHandler, CompositorState},
    shm::slot::SlotPool
};
use wayland_client::{
    protocol::wl_pointer::WlPointer,
    protocol::{wl_pointer, wl_shm},
    protocol::wl_output::WlOutput,
    globals::registry_queue_init,
    Connection,
    QueueHandle,
    protocol::wl_seat::WlSeat,
    protocol::wl_surface::WlSurface
};

fn main() -> Result<(), Box<dyn Error>> {
    let conn = Connection::connect_to_env().unwrap();
    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor is not available");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("layer_shell is not available");

    // using wl_shm for software rendering to buffer
    let shm = Shm::bind(&globals, &qh).expect("wl_shm is not available");

    // get outputs (for now, just use the first one)
    let mut outputs_event_queue = conn.new_event_queue();
    let outputs_qh = outputs_event_queue.handle();
    let outputs_registry_state = RegistryState::new(&globals);
    let output_delegate = OutputState::new(&globals, &outputs_qh);
    let mut list_outputs = ListOutputs {
        registry_state: outputs_registry_state,
        output_state: output_delegate
    };

    outputs_event_queue.roundtrip(&mut list_outputs)?;

    let output = list_outputs.output_state.outputs().next().expect("No outputs found");
    let info = &list_outputs.output_state.info(&output).expect("Output has no info");
    let dims = info.logical_size.expect("Output has no size");
    let (size_x, size_y) = (dims.0 as u32, dims.1 as u32);

    let registry_state = RegistryState::new(&globals);

    let surface = compositor.create_surface(&qh);
    let layer = layer_shell.create_layer_surface(&qh, surface, Layer::Background, Some("m04_dynamic_wallpaper"), None);
    layer.set_size(size_x, size_y);
    layer.set_anchor(Anchor::BOTTOM);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.commit();

    let pool = SlotPool::new((size_x * size_y * 4) as usize, &shm).expect("Failed to create slot pool");

    let mut dw_layer = DWLayer {
        registry_state,
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,

        exit: false,
        first_configure: true,
        pool,
        width: size_x,
        height: size_y,
        shift: None,
        layer,
        pointer: None
    };

    loop {
        event_queue.blocking_dispatch(&mut dw_layer).unwrap();

        if dw_layer.exit {
            println!("Exiting");
            break;
        }
    }

    Ok(())
}

struct ListOutputs {
    registry_state: RegistryState,
    output_state: OutputState
}

impl OutputHandler for ListOutputs {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, conn: &Connection, qh: &QueueHandle<Self>, output: WlOutput) {
        // TODO
    }

    fn update_output(&mut self, conn: &Connection, qh: &QueueHandle<Self>, output: WlOutput) {
        // TODO
    }

    fn output_destroyed(&mut self, conn: &Connection, qh: &QueueHandle<Self>, output: WlOutput) {
        // TODO
    }
}

delegate_output!(ListOutputs);
delegate_registry!(ListOutputs);

impl ProvidesRegistryState for ListOutputs {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers!(OutputState); // OutputState needs to get events re creation/destruction of globals
}

struct DWLayer {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,

    exit: bool,
    first_configure: bool,
    pool: SlotPool,
    width: u32,
    height: u32,
    shift: Option<u32>,
    layer: LayerSurface,
    pointer: Option<wl_pointer::WlPointer>
}

impl CompositorHandler for DWLayer {
    fn scale_factor_changed(&mut self, conn: &Connection, qh: &QueueHandle<Self>, surface: &WlSurface, new_factor: i32) {
        // probably not needed?
    }

    fn frame(&mut self, _conn: &Connection, qh: &QueueHandle<Self>, _surface: &WlSurface, _time: u32) {
        self.draw(&qh);
    }
}

// required for CompositorHandler
impl OutputHandler for DWLayer {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, conn: &Connection, qh: &QueueHandle<Self>, output: WlOutput) {}
    fn update_output(&mut self, conn: &Connection, qh: &QueueHandle<Self>, output: WlOutput) {}
    fn output_destroyed(&mut self, conn: &Connection, qh: &QueueHandle<Self>, output: WlOutput) {}
}

impl LayerShellHandler for DWLayer {
    fn closed(&mut self, conn: &Connection, qh: &QueueHandle<Self>, layer: &LayerSurface) {
        self.exit = true;
    }

    fn configure(&mut self, conn: &Connection, qh: &QueueHandle<Self>, layer: &LayerSurface, configure: LayerSurfaceConfigure, serial: u32) {
        // if configure.new_size.0 == 0 || configure.new_size.1 == 0 {
        //     self.width =
        // }

        if self.first_configure {
            self.first_configure = false;
            self.draw(qh);
        }
    }
}

impl SeatHandler for DWLayer {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, conn: &Connection, qh: &QueueHandle<Self>, seat: WlSeat) {}

    fn new_capability(&mut self, conn: &Connection, qh: &QueueHandle<Self>, seat: WlSeat, capability: Capability) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            let pointer = self.seat_state.get_pointer(qh, &seat).expect("Death create pointer");
            self.pointer = Some(pointer);
        }
    }

    fn remove_capability(&mut self, conn: &Connection, qh: &QueueHandle<Self>, seat: WlSeat, capability: Capability) {
        if capability == Capability::Pointer && self.pointer.is_some() {
            self.pointer.take().unwrap().release();
        }
    }
    fn remove_seat(&mut self, conn: &Connection, qh: &QueueHandle<Self>, seat: WlSeat) {}
}

impl PointerHandler for DWLayer {
    fn pointer_frame(&mut self, conn: &Connection, qh: &QueueHandle<Self>, pointer: &WlPointer, events: &[PointerEvent]) {
        use smithay_client_toolkit::seat::pointer::PointerEventKind::*;
        for event in events {
            if &event.surface != self.layer.wl_surface() {
                continue;
            }
            // TODO
        }
    }
}

impl ShmHandler for DWLayer {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl DWLayer {
    pub fn draw(&mut self, qh: &QueueHandle<Self>) {
        let width = self.width;
        let height = self.height;
        let stride = self.width * 4;

        let (buffer, canvas) = self.pool.create_buffer(width as i32, height as i32, stride as i32, wl_shm::Format::Argb8888).expect("failed to create buffer");

        let shift = self.shift.unwrap_or(0);
        canvas.chunks_exact_mut(4).enumerate().for_each(|(index, chunk)| {
            let x = ((index + shift as usize) % width as usize) as u32;
            let y = (index / width as usize) as u32;

            let a = 0xFF;
            let r = u32::min(((width - x) * 0xFF) / width, ((height - y) * 0xFF) / height);
            let g = u32::min((x * 0xFF) / width, ((height - y) * 0xFF) / height);
            let b = u32::min(((width - x) * 0xFF) / width, (y * 0xFF) / height);
            let color = (a << 24) + (r << 16) + (g << 8) + b;

            let array: &mut [u8; 4] = chunk.try_into().unwrap();
            *array = color.to_le_bytes();
        });

        if let Some(shift) = &mut self.shift {
            *shift = (*shift + 1) % width;
        }

        // damage entire window
        self.layer.wl_surface().damage_buffer(0, 0, width as i32, height as i32);

        self.layer.wl_surface().frame(qh, self.layer.wl_surface().clone());
        buffer.attach_to(self.layer.wl_surface()).expect("failed to attach buffer");
        self.layer.commit();

        // TODO - save and reuse buffer when window size unchanged (especially useful for damage tracking)
    }
}

delegate_compositor!(DWLayer);
delegate_output!(DWLayer);
delegate_shm!(DWLayer);
delegate_seat!(DWLayer);
delegate_pointer!(DWLayer);
delegate_layer!(DWLayer);
delegate_registry!(DWLayer);

impl ProvidesRegistryState for DWLayer {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![SeatState];
}