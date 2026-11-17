//! T1 technical spike: validates the Slint risks 1-4 from the design (§8)
//! and measures memory usage.
//!
//! Interactive: `cargo run --release --example spike_slint`
//! Scripted:    set `SPIKE_AUTO=1` to run a timed open/close sequence that
//!              logs memory at each phase and then exits.
//! Renderer:    `SLINT_BACKEND=winit-software` or `SLINT_BACKEND=winit-femtovg`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use slint::ComponentHandle;
use slint::winit_030::WinitWindowAccessor;

slint::slint! {
    #[edition = "1.19"]
    import { Button, TabWidget, LineEdit, SpinBox, Switch, Palette, VerticalBox, HorizontalBox } from "std-widgets.slint";

    export component Tray inherits SystemTrayIcon {
        in property <string> tip: "Rustpira";
        in property <bool> breaking: false;
        callback open-config();
        callback show-break();
        callback quit();
        icon: @image-url("recursos/rustpira-256.png");
        tooltip: root.tip;

        Menu {
            MenuItem {
                title: "Configuración";
                enabled: !root.breaking;
                activated => { root.open-config(); }
            }
            Menu {
                title: "Desactivar recordatorios";
                enabled: !root.breaking;
                MenuItem { title: "15 min"; }
                MenuItem { title: "30 min"; }
                MenuItem { title: "1 h"; }
                MenuItem { title: "2 h"; }
                MenuItem { title: "4 h"; }
            }
            MenuItem {
                title: "Probar descanso (spike)";
                enabled: !root.breaking;
                activated => { root.show-break(); }
            }
            MenuSeparator { }
            MenuItem { title: "Acerca de"; }
            MenuItem {
                title: "Salir";
                enabled: !root.breaking;
                activated => { root.quit(); }
            }
        }
    }

    export component Overlay inherits Window {
        in property <bool> primary: false;
        in property <string> countdown: "05:00";
        callback cancel();
        title: "Rustpira - descanso";
        no-frame: true;
        always-on-top: true;
        background: #1e3a5fcc;

        if root.primary: VerticalLayout {
            alignment: center;
            spacing: 18px;
            HorizontalLayout {
                alignment: center;
                Image {
                    source: @image-url("recursos/rustpira-256.png");
                    width: 128px;
                    height: 128px;
                }
            }
            Text {
                text: "Tomate un descanso";
                font-size: 40px;
                color: white;
                horizontal-alignment: center;
            }
            Text {
                text: "Estirá las piernas y mirá lejos.\nRelajá los hombros.";
                font-size: 22px;
                color: white;
                horizontal-alignment: center;
            }
            Text {
                text: root.countdown;
                font-size: 72px;
                color: white;
                horizontal-alignment: center;
            }
            HorizontalLayout {
                alignment: center;
                Button {
                    text: "Cancelar descanso";
                    clicked => { root.cancel(); }
                }
            }
        }
    }

    export component ConfigWindow inherits Window {
        in-out property <bool> dark: false;
        title: "Rustpira - Configuración";
        icon: @image-url("recursos/rustpira-256.png");
        preferred-width: 560px;
        preferred-height: 400px;

        public function apply-theme() {
            Palette.color-scheme = root.dark ? ColorScheme.dark : ColorScheme.light;
        }

        TabWidget {
            Tab {
                title: "General";
                VerticalBox {
                    alignment: start;
                    HorizontalBox {
                        Text { text: "Intervalo (min)"; vertical-alignment: center; }
                        SpinBox { minimum: 1; maximum: 240; value: 50; }
                    }
                    HorizontalBox {
                        Text { text: "Tema oscuro"; vertical-alignment: center; }
                        Switch {
                            checked <=> root.dark;
                            toggled => { root.apply-theme(); }
                        }
                    }
                    LineEdit { text: "Tomate un descanso"; }
                }
            }
            Tab {
                title: "Horarios";
                Text { text: "(prueba)"; }
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct MonitorRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    primary: bool,
}

/// Enumerates monitors in physical pixels (the process is per-monitor DPI
/// aware once winit has initialized).
fn monitors() -> Vec<MonitorRect> {
    use windows_sys::Win32::Foundation::{LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
    };
    use windows_sys::core::BOOL;
    const MONITORINFOF_PRIMARY: u32 = 1;

    unsafe extern "system" fn collect(
        monitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        // SAFETY: `data` is the `&mut Vec` passed to EnumDisplayMonitors below,
        // alive for the whole enumeration.
        let list = unsafe { &mut *(data as *mut Vec<MonitorRect>) };
        // SAFETY: MONITORINFO is plain data; zeroed is a valid initial state.
        let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
        info.cbSize = size_of::<MONITORINFO>() as u32;
        // SAFETY: `monitor` comes from the enumeration and `info` is sized.
        if unsafe { GetMonitorInfoW(monitor, &mut info) } != 0 {
            let r = info.rcMonitor;
            list.push(MonitorRect {
                x: r.left,
                y: r.top,
                width: (r.right - r.left) as u32,
                height: (r.bottom - r.top) as u32,
                primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
            });
        }
        1
    }

    let mut list: Vec<MonitorRect> = Vec::new();
    // SAFETY: the callback only touches `list`, which outlives the call.
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(collect),
            &mut list as *mut Vec<MonitorRect> as LPARAM,
        );
    }
    list
}

/// Returns (working set MB, private commit MB) for this process.
fn memory_mb() -> (f64, f64) {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    // SAFETY: plain data struct, zeroed is valid.
    let mut counters: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
    counters.cb = size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    // SAFETY: the EX struct starts with the base struct layout and `cb` is set.
    unsafe {
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS,
            counters.cb,
        );
    }
    const MB: f64 = 1024.0 * 1024.0;
    (
        counters.WorkingSetSize as f64 / MB,
        counters.PrivateUsage as f64 / MB,
    )
}

fn log_memory(phase: &str) {
    let (ws, private) = memory_mb();
    println!("[mem] {phase:<28} working set {ws:6.1} MB | private {private:6.1} MB");
}

struct App {
    tray: Tray,
    overlays: RefCell<Vec<Overlay>>,
    config: RefCell<Option<ConfigWindow>>,
    remaining_secs: Cell<u32>,
}

impl App {
    fn open_overlays(self: &Rc<Self>) {
        if !self.overlays.borrow().is_empty() {
            return;
        }
        for monitor in monitors() {
            let overlay = Overlay::new().expect("create overlay");
            overlay.set_primary(monitor.primary);
            overlay
                .window()
                .set_position(slint::PhysicalPosition::new(monitor.x, monitor.y));
            overlay
                .window()
                .set_size(slint::PhysicalSize::new(monitor.width, monitor.height));
            let weak_app = Rc::downgrade(self);
            overlay.on_cancel(move || {
                if let Some(app) = weak_app.upgrade() {
                    // Defer: do not drop a component from inside its own callback.
                    slint::Timer::single_shot(Duration::ZERO, move || app.close_overlays());
                }
            });
            overlay.show().expect("show overlay");
            overlay.window().with_winit_window(|w| {
                use slint::winit_030::winit::platform::windows::WindowExtWindows;
                w.set_skip_taskbar(true);
                w.focus_window();
            });
            self.overlays.borrow_mut().push(overlay);
        }
        self.tray.set_breaking(true);
        println!(
            "[spike] overlays opened on {} monitor(s)",
            self.overlays.borrow().len()
        );
    }

    fn close_overlays(&self) {
        for overlay in self.overlays.borrow_mut().drain(..) {
            let _ = overlay.hide();
        }
        self.tray.set_breaking(false);
    }

    fn open_config(self: &Rc<Self>) {
        if let Some(existing) = self.config.borrow().as_ref() {
            let _ = existing.show();
            return;
        }
        let window = ConfigWindow::new().expect("create config window");
        let weak_app = Rc::downgrade(self);
        window.window().on_close_requested(move || {
            if let Some(app) = weak_app.upgrade() {
                slint::Timer::single_shot(Duration::ZERO, move || app.close_config());
            }
            slint::CloseRequestResponse::HideWindow
        });
        window.show().expect("show config window");
        *self.config.borrow_mut() = Some(window);
    }

    fn set_config_dark(&self, dark: bool) {
        if let Some(window) = self.config.borrow().as_ref() {
            window.set_dark(dark);
            window.invoke_apply_theme();
        }
    }

    fn close_config(&self) {
        if let Some(window) = self.config.borrow_mut().take() {
            let _ = window.hide();
        }
    }

    fn tick(&self) {
        let left = self.remaining_secs.get().saturating_sub(1);
        self.remaining_secs.set(left);
        let (h, m, s) = (left / 3600, (left / 60) % 60, left % 60);
        self.tray
            .set_tip(format!("Siguiente descanso en {h:02}:{m:02}:{s:02}").into());
        let countdown = format!("{:02}:{:02}", (left / 60) % 60, s);
        for overlay in self.overlays.borrow().iter() {
            overlay.set_countdown(countdown.clone().into());
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let scripted = std::env::var_os("SPIKE_AUTO").is_some();
    println!(
        "[spike] backend={} scripted={scripted}",
        std::env::var("SLINT_BACKEND").unwrap_or_else(|_| "default".into())
    );

    let tray = Tray::new()?;
    tray.show()?;
    let app = Rc::new(App {
        tray,
        overlays: RefCell::new(Vec::new()),
        config: RefCell::new(None),
        remaining_secs: Cell::new(50 * 60),
    });

    let weak = Rc::downgrade(&app);
    app.tray.on_open_config(move || {
        if let Some(app) = weak.upgrade() {
            app.open_config();
        }
    });
    let weak = Rc::downgrade(&app);
    app.tray.on_show_break(move || {
        if let Some(app) = weak.upgrade() {
            app.open_overlays();
        }
    });
    app.tray.on_quit(|| {
        let _ = slint::quit_event_loop();
    });

    let ticks = Rc::new(Cell::new(0u32));
    let timer = slint::Timer::default();
    let weak = Rc::downgrade(&app);
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(1),
        move || {
            let Some(app) = weak.upgrade() else { return };
            app.tick();
            if !scripted {
                return;
            }
            let t = ticks.get() + 1;
            ticks.set(t);
            match t {
                5 => log_memory("idle (tray only)"),
                6 => app.open_overlays(),
                9 => log_memory("break overlay open"),
                10 => app.close_overlays(),
                14 => log_memory("after break #1"),
                15 => app.open_config(),
                17 => app.set_config_dark(true),
                18 => log_memory("config open (dark)"),
                19 => app.close_config(),
                23 => log_memory("after config"),
                24 | 27 | 30 => app.open_overlays(),
                25 | 28 | 31 => app.close_overlays(),
                35 => log_memory("after 3 more breaks"),
                36 => {
                    let _ = slint::quit_event_loop();
                }
                _ => {}
            }
        },
    );

    log_memory("startup");
    slint::run_event_loop_until_quit()
}
