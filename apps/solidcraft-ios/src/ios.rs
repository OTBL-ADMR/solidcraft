//! The iPadOS host: the window, the GPU, settings and the Documents folder.

use std::rc::Rc;
use std::time::Instant;

use solidcraft_engine::Session;
use solidcraft_ui_egui::{Services, SolveApp};

use crate::files::{Ask, Library, Picker};
use crate::prefs_writer::{Due, PrefsWriter};

/// The scene's depth buffer: 32-bit float with a stencil where the device has it (every Apple GPU
/// that runs iPadOS 15), else 24 bits. Drawn offscreen; the window's pass has no depth.
const FLOAT_DEPTH_BITS: u8 = 32;
const DEPTH_BITS: u8 = 24;
/// The section cap marks where it shows in the stencil.
const STENCIL_BITS: u8 = 8;
const FLOAT_DEPTH_STENCIL: eframe::wgpu::Features = eframe::wgpu::Features::DEPTH32FLOAT_STENCIL8;

pub fn run() {
    // The kernel's fix set, named to solidcore-ops before any call runs on it.
    solidcraft_engine::kernel::start();
    let library = Library::documents();
    if let Some(lib) = &library
        && let Err(e) = lib.ensure()
    {
        log::warn!("SolidCraft: could not make {}: {e}", lib.root.display());
    }
    // Library/Application Support/SolidCraft inside the app's container (`recovery::data_dir`).
    let settings = solidcraft_engine::recovery::data_dir();
    if let Some(dir) = &settings
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        log::warn!("SolidCraft: could not make {}: {e}", dir.display());
    }
    let options = eframe::NativeOptions {
        // The window's pass (egui's) has no depth and one sample: the viewport draws its scene
        // offscreen with its own depth and MSAA, then copies it in.
        depth_buffer: 0,
        stencil_buffer: 0,
        multisampling: 1,
        persistence_path: settings.map(|d| d.join("app.ron")),
        wgpu_options: wgpu_options(),
        ..Default::default()
    };
    let result = eframe::run_native(
        "SolidCraft",
        options,
        Box::new(move |cc| {
            solidcraft_ui_egui::fonts::setup_bundled_fonts(&cc.egui_ctx);
            let picker = Rc::new(Picker::default());
            let services = services(&picker, library.is_some());
            let mut app = SolveApp::new(Session::default(), services);
            let stored = cc.storage.and_then(|s| solidcraft_ui_egui::prefs::stored(|k| s.get_string(k)));
            if let Some(p) = &stored {
                app.load_prefs(p);
            }
            app.apply_preferences(&cc.egui_ctx);
            if let Some(rs) = &cc.wgpu_render_state {
                install_viewport(&mut app, rs);
            }
            // The empty design takes Units of new designs (#1205).
            solidcraft_ui_egui::prefs::apply_new_design(&mut app);
            if app.preferences.home_on_launch(false, false, false) {
                let rev = app.session.revision;
                app.home.show(rev);
            }
            let mut prefs_writer = PrefsWriter::default();
            if let Ok(prefs) = app.try_prefs() {
                prefs_writer.read(prefs);
            }
            Ok(Box::new(Shell { app, picker, library, prefs_writer }))
        }),
    );
    if let Err(e) = result {
        log::error!("SolidCraft could not start: {e}");
    }
}

/// Metal only, one frame in flight (as the desktop app: two queued frames add a frame of latency
/// to every orbit), and the device features the viewport uses where the GPU has them.
fn wgpu_options() -> eframe::egui_wgpu::WgpuConfiguration {
    let mut options = eframe::egui_wgpu::WgpuConfiguration::default().with_surface_config(eframe::egui_wgpu::SurfaceConfig::LOW_LATENCY);
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_setup {
        setup.instance_descriptor.backends = eframe::wgpu::Backends::METAL;
        let base = setup.device_descriptor.clone();
        setup.device_descriptor = std::sync::Arc::new(move |adapter| {
            let mut d = base(adapter);
            // Indirect draws that start at an instance: instanced meshes are drawn a call per pipeline.
            d.required_features |= adapter.features() & (FLOAT_DEPTH_STENCIL | eframe::wgpu::Features::INDIRECT_FIRST_INSTANCE);
            d
        });
    }
    options
}

/// The 3D view on the GPU, its scene offscreen with its own depth, stencil and the saved
/// anti-aliasing, as the desktop app draws it; then Home's pictures, drawn by the same device.
/// Run once at start: the shader compile happens before the first frame.
fn install_viewport(app: &mut SolveApp, rs: &eframe::egui_wgpu::RenderState) {
    let backend = rs.adapter.get_info().backend;
    let depth_bits = if rs.device.features().contains(FLOAT_DEPTH_STENCIL) { FLOAT_DEPTH_BITS } else { DEPTH_BITS };
    let depth = eframe::egui_wgpu::depth_format_from_bits(depth_bits, STENCIL_BITS);
    let supported = eframe::egui_wgpu::supported_msaa_samples(&rs.adapter, &rs.device, rs.target_format, depth);
    let requested = solidcraft_ui_egui::gpu::scene_samples(backend, app.preferences.msaa, &supported);
    let drawn = app.set_wgpu_split(rs, depth_bits, STENCIL_BITS, requested);
    app.msaa_supported = solidcraft_ui_egui::gpu::scene_sample_choices(backend, supported);
    app.services.graphics_backend = Some("metal");
    solidcraft_ui_egui::gpu::install_picture_renderer(rs);
    log::info!("SolidCraft graphics: {backend:?}, MSAA {drawn}x (saved {}x)", app.preferences.msaa);
}

/// Open and Save go through the in-app windows over the Documents folder. Without that folder
/// (no home: never inside an app container) there is nowhere to open from or save to.
fn services(picker: &Rc<Picker>, has_library: bool) -> Services {
    if !has_library {
        return Services::default();
    }
    let (opener, saver) = (picker.clone(), picker.clone());
    Services {
        pick_open: Some(Box::new(move |exts: &[&str]| opener.ask(Ask::Open { exts: exts.iter().map(|e| e.to_string()).collect() }))),
        pick_save: Some(Box::new(move |name: &str, exts: &[&str]| {
            saver.ask(Ask::Save { name: name.to_string(), exts: exts.iter().map(|e| e.to_string()).collect() })
        })),
        ..Default::default()
    }
}

struct Shell {
    app: SolveApp,
    picker: Rc<Picker>,
    library: Option<Library>,
    prefs_writer: PrefsWriter,
}

impl eframe::App for Shell {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.app.logic(ctx);
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        solidcraft_ui_egui::window_chrome::release_stale_input(ctx, raw);
        self.app.raw_input_hook(raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Inside the safe area: clear of the status bar, the camera housing and the home indicator.
        let content = ui.ctx().content_rect();
        ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| self.app.ui(ui));
        if let Some(library) = &self.library {
            self.picker.ui(library, ui.ctx());
        }
        self.write_prefs(ui.ctx(), frame);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        match self.app.try_prefs() {
            Ok(prefs) => {
                storage.set_string(solidcraft_ui_egui::prefs::STORAGE_KEY, prefs.clone());
                self.prefs_writer.wrote(prefs, Instant::now());
            }
            Err(reason) => log::warn!("Settings were not written: {reason}"),
        }
    }
}

impl Shell {
    /// Changed settings reach the disk within [`PrefsWriter::INTERVAL`].
    /// Only compared after input, or when a waiting change's time has come: building the settings
    /// every frame of an orbit would cost for nothing.
    fn write_prefs(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let now = Instant::now();
        if !self.prefs_writer.worth_checking(ctx.input(|i| !i.events.is_empty()), now) {
            return;
        }
        let Ok(prefs) = self.app.try_prefs() else { return };
        match self.prefs_writer.due(&prefs, now) {
            Due::Nothing => {}
            Due::Wait(wait) => ctx.request_repaint_after(wait),
            Due::Write => {
                if let Some(storage) = frame.storage_mut() {
                    storage.set_string(solidcraft_ui_egui::prefs::STORAGE_KEY, prefs.clone());
                    storage.flush();
                    self.prefs_writer.wrote(prefs, now);
                }
            }
        }
    }
}
