//! Files on the iPad: the app's Documents folder, which the Files app shows as
//! On My iPad › SolidCraft (`UIFileSharingEnabled` and `LSSupportsOpeningDocumentsInPlace` in
//! `packaging/ios/Info.plist`). Open lists the files there; Save asks for a name and writes there.
//! A STEP file from anywhere else reaches the app by being copied into that folder in Files.
//!
//! The pickers are in-app windows, not UIKit's document picker: the shared UI asks for a path and
//! waits on a future ([`FilePickerFuture`]), and these windows answer it. Nothing here is iOS-only,
//! so it builds and is tested on every system.

use std::cell::RefCell;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use solidcraft_ui_egui::FilePickerFuture;
use solidcraft_ui_egui::ui_kit;

/// The folder designs are kept in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Library {
    pub root: PathBuf,
}

/// A file in the library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub size: u64,
}

impl Library {
    /// `$HOME/Documents`: inside an iOS app's container, the folder Files shows. None without a home.
    pub fn documents() -> Option<Self> {
        let home = std::env::var_os("HOME").filter(|h| !h.is_empty())?;
        Some(Self { root: PathBuf::from(home).join("Documents") })
    }

    /// The files whose extension is one of `exts` (any case), by name; every file for `&[]`.
    /// Folders, hidden files and names that are not text are left out. A folder that can't be read
    /// lists nothing.
    pub fn list(&self, exts: &[&str]) -> Vec<Entry> {
        let Ok(dir) = std::fs::read_dir(&self.root) else { return Vec::new() };
        let mut entries: Vec<Entry> = dir
            .filter_map(Result::ok)
            .filter_map(|e| {
                let meta = e.metadata().ok().filter(std::fs::Metadata::is_file)?;
                let name = e.file_name().into_string().ok().filter(|n| !n.starts_with('.'))?;
                has_extension(&name, exts).then_some(Entry { name, size: meta.len() })
            })
            .collect();
        entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.name.cmp(&b.name)));
        entries
    }

    /// The path a file of this name is saved to: the name's last component only, with the first of
    /// `exts` added when it has none of them. None for a name with nothing usable in it.
    pub fn save_path(&self, name: &str, exts: &[&str]) -> Option<PathBuf> {
        let name = Path::new(name.trim()).file_name()?.to_str()?.trim();
        if name.is_empty() || name.starts_with('.') {
            return None;
        }
        let name = match exts.first() {
            Some(first) if !has_extension(name, exts) => format!("{name}.{first}"),
            _ => name.to_string(),
        };
        Some(self.root.join(name))
    }

    pub fn path_of(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    /// Make the folder, so the Files app shows it before anything was saved.
    pub fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)
    }
}

/// Whether `name` ends in one of `exts` (any case); always for `&[]`.
fn has_extension(name: &str, exts: &[&str]) -> bool {
    if exts.is_empty() {
        return true;
    }
    Path::new(name).extension().and_then(|e| e.to_str()).is_some_and(|e| exts.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// What the shared UI asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    /// A file to open, of one of these kinds (`[]`: anything the app opens).
    Open { exts: Vec<String> },
    /// A name to save under, starting from `name`.
    Save { name: String, exts: Vec<String> },
}

#[derive(Default)]
struct Slot {
    answer: Option<Option<String>>,
    waker: Option<Waker>,
}

/// The answer the shared UI waits on.
struct Answer(Rc<RefCell<Slot>>);

impl Future for Answer {
    type Output = Option<String>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<String>> {
        let mut slot = self.0.borrow_mut();
        match slot.answer.take() {
            Some(answer) => Poll::Ready(answer),
            None => {
                slot.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

struct Waiting {
    ask: Ask,
    /// The name being typed (Save).
    name: String,
    slot: Rc<RefCell<Slot>>,
}

impl Waiting {
    fn answer(self, path: Option<String>) {
        let mut slot = self.slot.borrow_mut();
        slot.answer = Some(path);
        if let Some(waker) = slot.waker.take() {
            waker.wake();
        }
    }
}

/// The question on screen, if any. One at a time: a new one cancels the last.
#[derive(Default)]
pub struct Picker {
    waiting: RefCell<Option<Waiting>>,
}

impl Picker {
    pub fn ask(&self, ask: Ask) -> FilePickerFuture {
        let slot = Rc::new(RefCell::new(Slot::default()));
        let name = match &ask {
            Ask::Save { name, .. } => name.clone(),
            Ask::Open { .. } => String::new(),
        };
        if let Some(previous) = self.waiting.replace(Some(Waiting { ask, name, slot: slot.clone() })) {
            previous.answer(None);
        }
        Box::pin(Answer(slot))
    }

    pub fn asking(&self) -> Option<Ask> {
        self.waiting.borrow().as_ref().map(|w| w.ask.clone())
    }

    /// Answer the question on screen (`None`: cancelled).
    pub fn answer(&self, path: Option<String>) {
        if let Some(waiting) = self.waiting.take() {
            waiting.answer(path);
        }
    }

    /// The Open or Save window, while one is asked for.
    pub fn ui(&self, library: &Library, ctx: &egui::Context) {
        let Some(ask) = self.asking() else { return };
        let answer = match &ask {
            Ask::Open { exts } => open_window(library, exts, ctx),
            Ask::Save { exts, .. } => {
                let mut name = self.waiting.borrow().as_ref().map(|w| w.name.clone()).unwrap_or_default();
                let answer = save_window(library, &mut name, exts, ctx);
                if let Some(w) = self.waiting.borrow_mut().as_mut() {
                    w.name = name;
                }
                answer
            }
        };
        if let Some(path) = answer {
            self.answer(path);
        }
    }
}

/// `Some(answer)` once the user chose a file or cancelled.
fn open_window(library: &Library, exts: &[String], ctx: &egui::Context) -> Option<Option<String>> {
    let exts: Vec<&str> = exts.iter().map(String::as_str).collect();
    let entries = library.list(&openable(&exts));
    let mut chosen: Option<Option<String>> = None;
    let closed = ui_kit::Window::new(egui::Id::unique("sc_ios_open"), "folder", "Open from this iPad").cards(4.0).show_body(ctx, |ui| {
        ui.weak("Files in On My iPad › SolidCraft. To open a file kept elsewhere, copy it into that folder in the Files app.");
        ui.separator();
        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
            ui_kit::name_list(ui, "ios_open_files", &["Name", "Size", ""], "No files to open here yet.", |list| {
                for e in &entries {
                    list.row(&e.name, |row| {
                        row.name(&e.name);
                        row.cell(|ui| ui.label(size_text(e.size)));
                        row.cell(|ui| {
                            if ui_kit::button(ui, "Open", ui_kit::Button::Small).clicked() {
                                chosen = Some(Some(library.path_of(&e.name).to_string_lossy().into_owned()));
                            }
                        });
                    });
                }
            });
        });
        ui.separator();
        if ui_kit::button(ui, "Cancel", ui_kit::Button::Normal).clicked() {
            chosen = Some(None);
        }
    });
    if closed && chosen.is_none() {
        chosen = Some(None);
    }
    chosen
}

/// `Some(answer)` once the user saved or cancelled.
fn save_window(library: &Library, name: &mut String, exts: &[String], ctx: &egui::Context) -> Option<Option<String>> {
    let exts: Vec<&str> = exts.iter().map(String::as_str).collect();
    let target = library.save_path(name, &exts);
    let mut chosen: Option<Option<String>> = None;
    let closed = ui_kit::Window::new(egui::Id::unique("sc_ios_save"), "save", "Save to this iPad").show_body(ctx, |ui| {
        ui.label("Name");
        let field = ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
        let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        match &target {
            Some(path) if path.exists() => {
                ui.weak("A file of this name is there: saving replaces it.");
            }
            Some(_) => {
                ui.weak("Saved in On My iPad › SolidCraft.");
            }
            None => {
                ui.weak("Type a name.");
            }
        }
        ui.horizontal(|ui| {
            let save = ui_kit::button(ui, "Save", ui_kit::Button::Enabled(target.is_some())).clicked();
            if (save || entered)
                && let Some(path) = &target
            {
                chosen = Some(Some(path.to_string_lossy().into_owned()));
            }
            if ui_kit::button(ui, "Cancel", ui_kit::Button::Normal).clicked() {
                chosen = Some(None);
            }
        });
    });
    if closed && chosen.is_none() {
        chosen = Some(None);
    }
    chosen
}

/// The kinds Open lists: those asked for, else designs, CAD files, meshes and drawings.
fn openable<'a>(asked: &[&'a str]) -> Vec<&'a str> {
    if !asked.is_empty() {
        return asked.to_vec();
    }
    let mut all = solidcraft_engine::io::formats::open_extensions(true);
    all.extend(["dxf", "svg"]);
    all
}

fn size_text(bytes: u64) -> String {
    match bytes {
        b if b < 1024 => format!("{b} B"),
        b if b < 1024 * 1024 => format!("{:.1} KB", b as f64 / 1024.0),
        b => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library(tag: &str) -> Library {
        let root = std::env::temp_dir().join(format!("solidcraft-ios-files-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let library = Library { root };
        assert!(library.ensure().is_ok());
        library
    }

    fn poll(future: &mut FilePickerFuture) -> Poll<Option<String>> {
        future.as_mut().poll(&mut Context::from_waker(Waker::noop()))
    }

    #[test]
    fn list_keeps_files_of_the_kinds_asked_for_by_name() {
        let lib = library("list");
        for name in ["b.STEP", "a.step", "c.stl", ".hidden.step"] {
            assert!(std::fs::write(lib.path_of(name), b"x").is_ok());
        }
        assert!(std::fs::create_dir_all(lib.path_of("folder.step")).is_ok());
        let names: Vec<String> = lib.list(&["step"]).into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["a.step", "b.STEP"]);
        assert_eq!(lib.list(&[]).len(), 3);
        let _ = std::fs::remove_dir_all(&lib.root);
    }

    #[test]
    fn list_of_a_missing_folder_is_empty() {
        let lib = Library { root: std::env::temp_dir().join("solidcraft-ios-files-none/nothing/here") };
        assert!(lib.list(&[]).is_empty());
    }

    #[test]
    fn save_path_keeps_the_name_inside_the_folder_and_adds_the_extension() {
        let lib = Library { root: PathBuf::from("/docs") };
        assert_eq!(lib.save_path("Bracket", &["solidcraft", "json"]), Some(PathBuf::from("/docs/Bracket.solidcraft")));
        assert_eq!(lib.save_path("plate.JSON", &["solidcraft", "json"]), Some(PathBuf::from("/docs/plate.JSON")));
        assert_eq!(lib.save_path("../../etc/passwd", &["stl"]), Some(PathBuf::from("/docs/passwd.stl")));
        assert_eq!(lib.save_path("  part.stl  ", &["stl"]), Some(PathBuf::from("/docs/part.stl")));
        assert_eq!(lib.save_path("anything", &[]), Some(PathBuf::from("/docs/anything")));
        for bad in ["", "   ", "..", "/", ".hidden"] {
            assert_eq!(lib.save_path(bad, &["stl"]), None, "{bad:?}");
        }
    }

    #[test]
    fn an_answer_reaches_the_waiting_future_and_a_new_question_cancels_the_last() {
        let picker = Picker::default();
        let mut first = picker.ask(Ask::Open { exts: vec![] });
        assert_eq!(poll(&mut first), Poll::Pending);
        let mut second = picker.ask(Ask::Save { name: "part".into(), exts: vec!["stl".into()] });
        assert_eq!(poll(&mut first), Poll::Ready(None));
        assert_eq!(picker.asking(), Some(Ask::Save { name: "part".into(), exts: vec!["stl".into()] }));
        picker.answer(Some("/docs/part.stl".into()));
        assert_eq!(poll(&mut second), Poll::Ready(Some("/docs/part.stl".into())));
        assert_eq!(picker.asking(), None);
        // Nothing is waiting: an answer now does nothing.
        picker.answer(None);
    }

    #[test]
    fn open_lists_every_kind_the_app_reads_when_none_is_asked_for() {
        let all = openable(&[]);
        assert!(all.contains(&"dxf") && all.contains(&"svg"));
        assert!(all.iter().any(|e| e.eq_ignore_ascii_case("step")));
        assert_eq!(openable(&["png"]), ["png"]);
    }

    #[test]
    fn sizes_read_in_the_nearest_unit() {
        assert_eq!(size_text(512), "512 B");
        assert_eq!(size_text(2048), "2.0 KB");
        assert_eq!(size_text(3 * 1024 * 1024), "3.0 MB");
    }
}
