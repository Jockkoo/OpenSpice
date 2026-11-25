use crate::analysis::{Analysis, Settings};
use crate::dock::{Dock, Layout as DockLayout, View};
use crate::plot::{BACK, Plot};
use crate::schematic::{Schematic, Tool};
use crate::simulation::{EXAMPLE, Run, simulate};
use eframe::egui::{
    self, Button, Color32, Context, FontId, Frame, Layout, Panel, ScrollArea, TextEdit, Ui,
};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

const CHROME: Color32 = Color32::from_gray(34);
const TOOLBAR: Color32 = Color32::from_gray(40);
const CHROME_EDGE: Color32 = Color32::from_gray(55);
const ACTIVE_TOOL: Color32 = Color32::from_rgb(24, 115, 181);
pub(crate) const TOOL_SIZE: egui::Vec2 = egui::vec2(32., 32.);

pub(crate) const DIALOG_FIELD_WIDTH: f32 = 140.;

#[derive(Clone, Copy)]
enum Icon {
    Save,
    Configure,
    Cursor,
    Select,
    Move,
    Delete,
    Copy,
    Run,
    Plus,
    Minus,
    Fit,
}

pub(crate) fn dialog<T>(
    ctx: &Context,
    id: &'static str,
    title: &str,
    body: impl FnOnce(&mut Ui) -> T,
) -> egui::ModalResponse<T> {
    dialog_with_min_height(ctx, egui::Id::unique(id), title, None, body)
}

pub(crate) fn dialog_with_min_height<T>(
    ctx: &Context,
    id: egui::Id,
    title: &str,
    min_height: Option<f32>,
    body: impl FnOnce(&mut Ui) -> T,
) -> egui::ModalResponse<T> {
    let frame = Frame::new()
        .fill(Color32::from_gray(24))
        .stroke(egui::Stroke::new(1., Color32::from_gray(49)))
        .inner_margin(15)
        .shadow(egui::epaint::Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: Color32::from_black_alpha(110),
        });
    egui::Modal::new(id)
        .frame(frame)
        .backdrop_color(Color32::from_black_alpha(150))
        .show(ctx, |ui| {
            ui.set_width(360.);
            if let Some(height) = min_height {
                let available_height = (ctx.content_rect().height() - 100.).max(100.);
                ui.set_min_height(height.min(available_height));
            }
            let style = ui.style_mut();
            style.spacing.item_spacing = egui::vec2(8., 6.);
            style.spacing.button_padding = egui::vec2(8., 4.);
            style.spacing.interact_size.y = 24.;
            style.visuals.window_fill = Color32::from_gray(24);
            style.visuals.menu_corner_radius = egui::CornerRadius::ZERO;
            style.visuals.extreme_bg_color = Color32::from_gray(31);
            style.visuals.widgets.noninteractive.fg_stroke.color = Color32::from_gray(177);
            for (widgets, fill, border) in [
                (&mut style.visuals.widgets.inactive, 34, 57),
                (&mut style.visuals.widgets.hovered, 47, 83),
                (&mut style.visuals.widgets.active, 49, 110),
                (&mut style.visuals.widgets.open, 42, 75),
            ] {
                widgets.corner_radius = egui::CornerRadius::ZERO;
                widgets.bg_fill = Color32::from_gray(fill);
                widgets.weak_bg_fill = widgets.bg_fill;
                widgets.bg_stroke = egui::Stroke::new(1., Color32::from_gray(border));
                widgets.expansion = 0.;
            }
            style.visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::ZERO;
            ui.label(
                egui::RichText::new(title)
                    .size(19.)
                    .strong()
                    .color(Color32::from_gray(238)),
            );
            ui.add_space(4.);
            let available_height = (ctx.content_rect().height() - 100.).max(100.);
            ScrollArea::vertical()
                .max_height(available_height)
                .auto_shrink([false, true])
                .show(ui, body)
                .inner
        })
}

pub(crate) fn dialog_field(ui: &mut Ui, label: &str, value: &mut String) {
    ui.label(label);
    ui.add(
        egui::TextEdit::singleline(value)
            .desired_width(DIALOG_FIELD_WIDTH)
            .margin(egui::Margin::symmetric(6, 3)),
    );
    ui.end_row();
}

pub(crate) fn dialog_actions(ui: &mut Ui, primary: &str, enabled: bool, cancel: bool) -> bool {
    ui.add_space(6.);
    ui.separator();
    ui.add_space(2.);
    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
        let button = Button::new(
            egui::RichText::new(primary)
                .strong()
                .color(Color32::from_gray(22)),
        )
        .fill(Color32::from_gray(239))
        .min_size(egui::vec2(70., 26.));
        let apply = ui.add_enabled(enabled, button).clicked();
        if apply
            || (cancel
                && ui
                    .add(Button::new("Cancel").min_size(egui::vec2(70., 26.)))
                    .clicked())
        {
            ui.close();
        }
        apply
    })
    .inner
}

pub(crate) fn canvas_zoom_factor(input: &egui::InputState) -> f64 {
    (f64::from(input.smooth_scroll_delta().y) * 0.007)
        .clamp(-2., 2.)
        .exp()
        * f64::from(input.zoom_delta())
}

pub(crate) fn toolbar_surface(ui: &Ui, response: &egui::Response, active: bool) -> Color32 {
    if !response.enabled() {
        return Color32::from_gray(115);
    }
    let fill = if active {
        ACTIVE_TOOL
    } else if response.is_pointer_button_down_on() {
        Color32::from_gray(66)
    } else if response.hovered() || response.has_focus() {
        Color32::from_gray(52)
    } else {
        return Color32::from_gray(206);
    };
    ui.painter().rect_filled(response.rect, 3, fill);
    Color32::WHITE
}

fn tool(ui: &mut Ui, enabled: bool, icon: Icon, active: bool, hint: &str) -> bool {
    let response = ui.add_enabled_ui(enabled, |ui| {
        let (_, rect) = ui.allocate_space(TOOL_SIZE);
        let response = ui.interact(rect, egui::Id::unique(hint), egui::Sense::click());
        let color = toolbar_surface(ui, &response, active);
        draw_icon(ui.painter(), rect.center(), icon, color);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, hint));
        response.on_hover_text(hint).clicked()
    });
    response.inner
}

fn draw_icon(p: &egui::Painter, center: egui::Pos2, icon: Icon, color: Color32) {
    use egui::{Pos2, Stroke, vec2};
    let origin = center - vec2(8., 8.);
    let point = |x, y| -> Pos2 { origin + vec2(x, y) };
    let pen = Stroke::new(1.25, color);
    let path = |coords: &[(f32, f32)]| {
        p.add(egui::Shape::line(
            coords.iter().map(|&(x, y)| point(x, y)).collect(),
            pen,
        ));
    };
    match icon {
        Icon::Save => {
            path(&[
                (1., 1.),
                (12., 1.),
                (15., 4.),
                (15., 15.),
                (1., 15.),
                (1., 1.),
            ]);
            path(&[(4., 1.), (4., 6.), (11., 6.), (11., 1.)]);
            path(&[(4., 15.), (4., 9.), (12., 9.), (12., 15.)]);
        }
        Icon::Run | Icon::Configure => {
            let cog = matches!(icon, Icon::Configure);
            if cog {
                let gear = (0..=32)
                    .map(|i| {
                        let angle = (i as f32 - 0.5) * std::f32::consts::TAU / 32.;
                        let radius = if i % 4 < 2 { 8.5 } else { 6.8 };
                        point(8. + radius * angle.cos(), 8. + radius * angle.sin())
                    })
                    .collect();
                p.add(egui::Shape::line(gear, pen));
            }
            let size = if cog { 0.52 } else { 1. };
            let triangle = [(-4.8, -7.), (7., 0.), (-4.8, 7.)]
                .map(|(x, y)| point(8. + size * x, 8. + size * y));
            p.add(egui::Shape::convex_polygon(
                triangle.to_vec(),
                Color32::TRANSPARENT,
                pen,
            ));
        }
        Icon::Cursor => path(&[
            (2., 0.),
            (14., 9.),
            (8., 9.),
            (11., 15.),
            (8., 16.),
            (5., 10.),
            (2., 14.),
            (2., 0.),
        ]),
        Icon::Select => {
            for (x, y, dx, dy) in [
                (1., 2., 1., 1.),
                (15., 2., -1., 1.),
                (1., 14., 1., -1.),
                (15., 14., -1., -1.),
            ] {
                path(&[(x + dx * 3., y), (x, y), (x, y + dy * 3.)]);
            }
            path(&[(6., 2.), (10., 2.)]);
            path(&[(6., 14.), (10., 14.)]);
        }
        Icon::Move => {
            for (dx, dy) in [(1., 0.), (-1., 0.), (0., 1.), (0., -1.)] {
                path(&[(8., 8.), (8. + dx * 8., 8. + dy * 8.)]);
                path(&[
                    (8. + dx * 5. + dy * 3., 8. + dy * 5. + dx * 3.),
                    (8. + dx * 8., 8. + dy * 8.),
                    (8. + dx * 5. - dy * 3., 8. + dy * 5. - dx * 3.),
                ]);
            }
        }
        Icon::Delete => {
            p.circle_stroke(point(3.5, 12.5), 2.5, pen);
            p.circle_stroke(point(11.5, 12.5), 2.5, pen);
            path(&[(5., 10.5), (14., 1.)]);
            path(&[(10., 10.5), (1., 1.)]);
        }
        Icon::Copy => {
            path(&[(2., 10.), (2., 1.), (11., 1.), (11., 4.)]);
            path(&[(5., 5.), (14., 5.), (14., 15.), (5., 15.), (5., 5.)]);
        }
        Icon::Plus | Icon::Minus => {
            p.circle_stroke(point(6., 6.), 5., pen);
            path(&[(10., 10.), (15., 15.)]);
            path(&[(3., 6.), (9., 6.)]);
            if matches!(icon, Icon::Plus) {
                path(&[(6., 3.), (6., 9.)]);
            }
        }
        Icon::Fit => {
            for (x, y, dx, dy) in [
                (1., 2., 1., 1.),
                (15., 2., -1., 1.),
                (1., 14., 1., -1.),
                (15., 14., -1., -1.),
            ] {
                path(&[(x + dx * 4., y), (x, y), (x, y + dy * 4.)]);
            }
        }
    }
}

pub(crate) fn schematic_tool_cursor(ui: &Ui, position: egui::Pos2, clip: egui::Rect, tool: Tool) {
    let icon = match tool {
        Tool::Move => Icon::Move,
        Tool::Erase => Icon::Delete,
        Tool::Copy => Icon::Copy,
        _ => return,
    };
    ui.ctx().set_cursor_icon(egui::CursorIcon::None);
    let painter = ui
        .ctx()
        .layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::unique("schematic tool cursor"),
        ))
        .with_clip_rect(clip);
    draw_icon(&painter, position, icon, Color32::from_gray(35));
}

enum State {
    Ready,
    Running(Receiver<Run>),
    Success,
    Failed(String),
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum WaveformLayout {
    #[default]
    Above,
    Right,
    Tab,
}

impl WaveformLayout {
    const STORAGE_KEY: &str = "waveform_layout";

    fn stored_value(self) -> &'static str {
        match self {
            Self::Above => "above",
            Self::Right => "right",
            Self::Tab => "tab",
        }
    }

    fn dock_layout(self) -> DockLayout {
        match self {
            Self::Above => DockLayout::Rows(View::Waveforms),
            Self::Right => DockLayout::Columns(View::Schematic),
            Self::Tab => DockLayout::Tabs,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Above => "Vertical split",
            Self::Right => "Horizontal split",
            Self::Tab => "New tab",
        }
    }
}

#[derive(Clone, PartialEq)]
struct Revision {
    text: String,
    geometry: crate::schematic::Geometry,
}

pub struct Workbench {
    schematic: crate::schematic::Schematic,
    committed: Revision,
    saved: Revision,
    path: Option<PathBuf>,
    file_error: Option<String>,
    settings: Option<Settings>,
    undo: Vec<Revision>,
    redo: Vec<Revision>,
    dock: Dock,
    netlist: String,
    state: State,
    plot: Option<Plot>,
    probe_reveal_at: Option<f64>,
    log: String,
    log_open: bool,
    edited: bool,
    waveform_layout: WaveformLayout,
    show_netlist: bool,
    show_toolbar: bool,
}

impl Workbench {
    pub fn new(ctx: &Context, storage: Option<&dyn eframe::Storage>) -> Self {
        let layout = storage.and_then(|storage| storage.get_string(WaveformLayout::STORAGE_KEY));
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "Noto Sans".into(),
            egui::FontData::from_static(include_bytes!("../assets/fonts/NotoSans-Regular.ttf"))
                .into(),
        );
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .insert(0, "Noto Sans".into());
        }
        ctx.set_fonts(fonts);
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = CHROME;
        visuals.window_fill = Color32::from_gray(31);
        visuals.window_stroke = egui::Stroke::new(1., CHROME_EDGE);
        visuals.extreme_bg_color = BACK;
        visuals.selection.bg_fill = ACTIVE_TOOL;
        visuals.selection.stroke = egui::Stroke::new(1., Color32::WHITE);
        visuals.window_corner_radius = egui::CornerRadius::ZERO;
        visuals.menu_corner_radius = egui::CornerRadius::same(4);
        for widget in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.corner_radius = egui::CornerRadius::same(3);
        }
        visuals.widgets.inactive.weak_bg_fill = Color32::from_gray(43);
        visuals.widgets.hovered.weak_bg_fill = Color32::from_gray(55);
        visuals.widgets.active.weak_bg_fill = ACTIVE_TOOL;
        visuals.widgets.open.weak_bg_fill = Color32::from_gray(54);
        ctx.set_visuals(visuals);
        ctx.style_mut_of(ctx.theme(), |style| {
            style.spacing.menu_margin = egui::Margin::same(5);
            style.spacing.menu_spacing = 3.;
        });
        let mut schematic = crate::schematic::Schematic::default();
        schematic.american_resistor = storage
            .and_then(|s| s.get_string("resistor_symbol"))
            .as_deref()
            != Some("european");
        schematic.import(EXAMPLE);
        Self {
            path: None,
            file_error: None,
            settings: None,
            saved: Revision {
                text: EXAMPLE.into(),
                geometry: schematic.geometry(),
            },
            committed: Revision {
                text: EXAMPLE.into(),
                geometry: schematic.geometry(),
            },
            undo: Vec::new(),
            redo: Vec::new(),
            schematic,
            dock: Dock::default(),
            netlist: EXAMPLE.into(),
            state: State::Ready,
            plot: None,
            probe_reveal_at: None,
            log: String::new(),
            log_open: false,
            edited: false,
            waveform_layout: match layout.as_deref() {
                Some("left" | "right") => WaveformLayout::Right,
                Some("tab") => WaveformLayout::Tab,
                _ => WaveformLayout::default(),
            },
            show_netlist: false,
            show_toolbar: true,
        }
    }

    fn write_document(&mut self, path: &Path) -> Result<(), String> {
        let metadata = ron::to_string(&self.committed.geometry).map_err(|e| e.to_string())?;
        let (title, body) = self
            .committed
            .text
            .split_once('\n')
            .unwrap_or((&self.committed.text, ""));
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        write!(temp, "{title}\n* OpenSpice layout v1 {metadata}\n{body}")
            .map_err(|e| e.to_string())?;
        temp.as_file().sync_all().map_err(|e| e.to_string())?;
        temp.persist(path).map_err(|e| e.to_string())?;
        self.path = Some(path.into());
        self.saved = self.committed.clone();
        Ok(())
    }

    fn open_document(&mut self, path: &Path) -> Result<(), String> {
        let file = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let mut text = file.clone();
        let american = self.schematic.american_resistor;
        let mut schematic = Schematic::default();
        schematic.american_resistor = american;
        if let Some((title, rest)) = file.split_once('\n')
            && let Some(meta) = rest.strip_prefix("* OpenSpice layout v1 ")
        {
            let (layout, body) = meta.split_once('\n').unwrap_or((meta, ""));
            text = format!("{title}\n{body}");
            crate::schematic::upgrade_pole_pins(&mut text);
            let geometry =
                ron::from_str(layout).map_err(|e| format!("Invalid schematic layout: {e}"))?;
            if !Schematic::matches_document(&geometry, &text) {
                return Err("Schematic layout does not match the netlist".into());
            }
            schematic.restore(geometry, &text);
        } else {
            crate::schematic::upgrade_pole_pins(&mut text);
            schematic.import(&text);
        }
        schematic.fit_pending = true;
        self.schematic = schematic;
        self.netlist = text;
        self.reset_document(Some(path.into()));
        Ok(())
    }

    fn reset_document(&mut self, path: Option<PathBuf>) {
        self.path = path;
        self.committed = Revision {
            text: self.netlist.clone(),
            geometry: self.schematic.geometry(),
        };
        self.saved = self.committed.clone();
        self.undo.clear();
        self.redo.clear();
        self.settings = None;
        self.dock = Dock::default();
        self.plot = None;
        self.schematic.clear_operating_point();
        self.probe_reveal_at = None;
        self.dock.close_waveform();
        self.state = State::Ready;
        self.log.clear();
        self.edited = false;
    }

    fn save_to(&mut self, choice: Option<PathBuf>) {
        if let Some(path) = choice
            && let Err(e) = self.write_document(&path)
        {
            self.file_error = Some(format!("Cannot save {}: {e}", path.display()));
        }
    }

    fn save_action(&mut self, save_as: bool) {
        let mut path = self.path.clone();
        if save_as || path.is_none() {
            let mut dialog = rfd::FileDialog::new()
                .set_title("Save schematic as")
                .add_filter("SPICE schematic", &["cir"]);
            if let Some(parent) = path.as_deref().and_then(Path::parent) {
                dialog = dialog.set_directory(parent);
            }
            path = dialog
                .set_file_name(
                    path.as_deref()
                        .and_then(Path::file_name)
                        .and_then(|s| s.to_str())
                        .unwrap_or("circuit.cir"),
                )
                .save_file();
        }
        self.save_to(path);
    }

    fn configure_simulation(&mut self) {
        self.schematic.cancel();
        self.settings = Some(Settings::read(&self.netlist));
    }

    fn start(&mut self, ctx: &Context) {
        if matches!(self.state, State::Running(_)) {
            return;
        }
        self.schematic.clear_operating_point();
        self.probe_reveal_at = None;
        self.log.clear();
        self.edited = false;
        let (tx, rx) = mpsc::channel();
        self.state = State::Running(rx);
        let netlist = self.netlist.clone();
        let ctx = ctx.clone();
        if let Err(e) = std::thread::Builder::new()
            .name("ngspice".into())
            .spawn(move || {
                let _ = tx.send(simulate(&netlist));
                ctx.request_repaint();
            })
        {
            self.fail(format!("Cannot start simulation worker: {e}"));
        }
    }

    fn fail(&mut self, reason: String) {
        self.state = State::Failed(reason);
        self.log_open = true;
    }

    fn poll(&mut self) {
        let State::Running(rx) = &self.state else {
            return;
        };
        match rx.try_recv() {
            Ok(run) => {
                self.log = run.log;
                match run.waveform {
                    Ok(data) => {
                        self.schematic.cancel();
                        if data.analysis == Analysis::Op {
                            self.plot = None;
                            self.dock.close_waveform();
                            if !self.edited {
                                let display = Settings::read(&self.netlist).op_display;
                                self.schematic.set_operating_point(data, display);
                            }
                        } else if let Some(plot) = &mut self.plot {
                            plot.refresh(data);
                        } else {
                            self.dock.arrange(self.waveform_layout.dock_layout());
                            self.dock.close_waveform();
                            self.plot = Some(Plot::new(data));
                        }
                        self.state = State::Success;
                    }
                    Err(reason) => self.fail(reason),
                }
            }
            Err(TryRecvError::Disconnected) => {
                self.fail("Simulation worker stopped unexpectedly".into())
            }
            Err(TryRecvError::Empty) => {}
        }
    }

    fn commit_edit(&mut self) {
        let next = Revision {
            text: self.netlist.clone(),
            geometry: self.schematic.geometry(),
        };
        if self.committed != next {
            self.undo.push(std::mem::replace(&mut self.committed, next));
            if self.undo.len() > 128 {
                self.undo.remove(0);
            }
            self.redo.clear();
            self.edited = true;
            self.schematic.clear_operating_point();
            self.probe_reveal_at = None;
        }
    }
    fn step_history(&mut self, redo: bool) {
        let (from, to) = if redo {
            (&mut self.redo, &mut self.undo)
        } else {
            (&mut self.undo, &mut self.redo)
        };
        if let Some(next) = from.pop() {
            self.netlist.clone_from(&next.text);
            self.schematic.restore(next.geometry.clone(), &next.text);
            to.push(std::mem::replace(&mut self.committed, next));
            self.edited = true;
            self.schematic.clear_operating_point();
            self.probe_reveal_at = None;
        }
    }

    fn load_example(&mut self) {
        let american = self.schematic.american_resistor;
        self.schematic = Default::default();
        self.schematic.american_resistor = american;
        self.dock = Dock::default();
        self.netlist = EXAMPLE.into();
        self.schematic.import(EXAMPLE);
        self.reset_document(None);
    }

    fn menu_bar(&mut self, ui: &mut Ui) {
        let ready = !matches!(self.state, State::Running(_));
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.add_enabled(ready, Button::new("Open…")).clicked() {
                    ui.close();
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("SPICE schematic", &["cir", "sp", "net"])
                        .pick_file()
                        && let Err(e) = self.open_document(&path)
                    {
                        self.file_error = Some(format!("Cannot open {}: {e}", path.display()));
                    }
                }
                for (save_as, label) in [
                    (false, "Save    Ctrl+S"),
                    (true, "Save As…    Ctrl+Shift+S"),
                ] {
                    if ui.button(label).clicked() {
                        ui.close();
                        self.save_action(save_as);
                    }
                }
                ui.separator();
                if ui.add_enabled(ready, Button::new("Load Example")).clicked() {
                    self.load_example();
                    ui.close();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                for (redo, label, enabled) in [
                    (false, "Undo   Ctrl+Z", !self.undo.is_empty()),
                    (true, "Redo   Ctrl+Shift+Z", !self.redo.is_empty()),
                ] {
                    if ui.add_enabled(enabled, Button::new(label)).clicked() {
                        self.step_history(redo);
                        ui.close();
                    }
                }
            });
            ui.menu_button("View", |ui| {
                let mut open = !self.dock.waveform_closed();
                if ui
                    .add_enabled(
                        self.plot.as_ref().is_some_and(Plot::has_traces),
                        egui::Checkbox::new(&mut open, "Waveforms"),
                    )
                    .changed()
                {
                    if open {
                        self.dock.arrange(self.waveform_layout.dock_layout());
                    } else {
                        self.dock.close_waveform();
                    }
                    ui.close();
                }

                ui.menu_button("Resistor symbol", |ui| {
                    for (american, label) in [(false, "European"), (true, "American")] {
                        if ui
                            .radio_value(&mut self.schematic.american_resistor, american, label)
                            .clicked()
                        {
                            ui.close();
                        }
                    }
                });
                ui.checkbox(&mut self.show_toolbar, "Toolbar");
                if ui.checkbox(&mut self.show_netlist, "Netlist").changed() {
                    ui.close();
                }
                ui.checkbox(&mut self.log_open, "ngspice output");
            });
            ui.menu_button("Simulate", |ui| {
                if ui
                    .add_enabled(ready, Button::new("Configure Simulation…"))
                    .clicked()
                {
                    self.configure_simulation();
                    ui.close();
                }
                if ui
                    .add_enabled(ready, Button::new("Run Simulation"))
                    .clicked()
                {
                    self.start(ui.ctx());
                    ui.close();
                }
            });
            ui.menu_button("Plot Settings", |ui| {
                ui.menu_button("Waveform layout", |ui| {
                    for layout in [
                        WaveformLayout::Above,
                        WaveformLayout::Right,
                        WaveformLayout::Tab,
                    ] {
                        if ui
                            .radio_value(&mut self.waveform_layout, layout, layout.label())
                            .clicked()
                        {
                            self.dock.arrange(layout.dock_layout());
                            if !self.plot.as_ref().is_some_and(Plot::has_traces) {
                                self.dock.close_waveform();
                            }
                            ui.close();
                        }
                    }
                });
                ui.separator();
                if ui
                    .add_enabled(self.plot.is_some(), Button::new("Reset view"))
                    .clicked()
                    && let Some(plot) = &mut self.plot
                {
                    plot.reset();
                    ui.close();
                }
                if let Some(plot) = &mut self.plot {
                    ui.separator();
                    plot.legend(ui);
                }
            });
            ui.menu_button("Tools", |ui| {
                if ui.button("SPICE error log").clicked() {
                    self.log_open = true;
                    ui.close();
                }
            });
            ui.menu_button("Keybinds", |ui| {
                ScrollArea::vertical().max_height(420.).show(ui, |ui| {
                    for (section, bindings) in [
                        (
                            "File and history",
                            &[
                                ("Ctrl+S", "Save"),
                                ("Ctrl+Shift+S", "Save As"),
                                ("Ctrl+Z", "Undo"),
                                ("Ctrl+Shift+Z / Ctrl+Y", "Redo"),
                            ][..],
                        ),
                        (
                            "Schematic",
                            &[
                                ("Esc", "Cancel and clear selection"),
                                ("S", "Select"),
                                ("M", "Move"),
                                ("Delete / Ctrl+X", "Delete"),
                                ("Ctrl+C", "Copy tool"),
                                ("Ctrl+R", "Rotate"),
                                ("W", "Wire"),
                                ("G", "Ground"),
                                ("P", "Vcc"),
                                ("V", "Voltage source"),
                                ("I", "Current source"),
                                ("R", "Resistor"),
                                ("L", "Inductor"),
                                ("C", "Capacitor"),
                                ("D", "Diode"),
                            ][..],
                        ),
                        (
                            "Waveform",
                            &[
                                ("Scroll", "Zoom both axes"),
                                ("Ctrl+Scroll", "Zoom X axis"),
                                ("Ctrl+Shift+Scroll", "Zoom Y axis"),
                                ("Delete", "Remove hovered trace"),
                                ("Left drag", "Box zoom"),
                                ("Middle or right drag", "Pan"),
                                ("Double-click", "Zoom to Fit"),
                            ][..],
                        ),
                    ] {
                        ui.strong(section);
                        egui::Grid::new(section).spacing([16., 4.]).show(ui, |ui| {
                            for (keys, action) in bindings {
                                ui.label(*keys);
                                ui.label(*action);
                                ui.end_row();
                            }
                        });
                        ui.add_space(6.);
                    }
                });
            });
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                let name = self
                    .path
                    .as_deref()
                    .and_then(Path::file_name)
                    .and_then(|v| v.to_str())
                    .unwrap_or("Untitled");
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(format!(
                            "{name}{}",
                            if self.saved != self.committed {
                                " *"
                            } else {
                                ""
                            }
                        ))
                        .small()
                        .color(Color32::from_gray(178)),
                    )
                    .truncate(),
                );
            });
        });
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(2., 2.);
            ui.set_min_height(TOOL_SIZE.y);
            if tool(ui, true, Icon::Save, false, "Save schematic (Ctrl+S)") {
                self.save_action(false);
            }
            ui.separator();
            let ready = !matches!(self.state, State::Running(_));
            if tool(ui, ready, Icon::Configure, false, "Configure Simulation") {
                self.configure_simulation();
            }
            if tool(ui, ready, Icon::Run, false, "Run the current netlist") {
                self.start(ui.ctx());
            }
            ui.add_space(4.);
            ui.separator();
            ui.add_space(4.);
            for (mode, icon, label) in [
                (Tool::Cursor, Icon::Cursor, "Cursor"),
                (Tool::Select, Icon::Select, "Select"),
                (Tool::Move, Icon::Move, "Move"),
                (Tool::Erase, Icon::Delete, "Delete"),
                (Tool::Copy, Icon::Copy, "Copy"),
            ] {
                if tool(ui, true, icon, self.schematic.tool == mode, label) {
                    self.schematic.cancel();
                    self.schematic.tool = mode;
                    self.dock.reveal_schematic();
                }
            }
            ui.add_space(4.);
            ui.separator();
            ui.add_space(4.);
            if self.schematic.picker(ui) {
                self.dock.reveal_schematic();
            }

            if ui.available_width() < 164. {
                ui.end_row();
            }
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                let zoom = format!("{:.0}%", self.schematic.zoom_percent());
                ui.menu_button(zoom, |ui| {
                    ui.set_min_width(116.);
                    for percent in [50., 100., 200.] {
                        if ui.button(format!("{percent:.0}%")).clicked() {
                            self.dock.reveal_schematic();
                            self.schematic.set_zoom_percent(percent);
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button("Zoom to fit").clicked() {
                        self.dock.reveal_schematic();
                        self.schematic.fit_pending = true;
                        ui.close();
                    }
                })
                .response
                .on_hover_text("Schematic zoom");
                for (icon, steps, hint) in [
                    (Icon::Fit, 0., "Zoom schematic to fit"),
                    (Icon::Plus, 1., "Zoom schematic in"),
                    (Icon::Minus, -1., "Zoom schematic out"),
                ] {
                    if tool(ui, true, icon, false, hint) {
                        self.dock.reveal_schematic();
                        if steps == 0. {
                            self.schematic.fit_pending = true;
                        } else {
                            self.schematic.zoom(steps, None);
                        }
                    }
                }
            });
        });
    }
}

impl eframe::App for Workbench {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(
            "resistor_symbol",
            if self.schematic.american_resistor {
                "american"
            } else {
                "european"
            }
            .into(),
        );
        storage.set_string(
            WaveformLayout::STORAGE_KEY,
            self.waveform_layout.stored_value().into(),
        );
    }

    fn logic(&mut self, _: &Context, _: &mut eframe::Frame) {
        self.poll();
    }

    fn ui(&mut self, ui: &mut Ui, _: &mut eframe::Frame) {
        self.workspace(ui);
    }
}

impl Workbench {
    fn workspace(&mut self, ui: &mut Ui) {
        if self.settings.is_none() && self.file_error.is_none() {
            for (mods, save_as) in [
                (egui::Modifiers::CTRL | egui::Modifiers::SHIFT, true),
                (egui::Modifiers::CTRL, false),
            ] {
                if ui.input_mut(|i| i.consume_key(mods, egui::Key::S)) {
                    self.save_action(save_as);
                    break;
                }
            }
        }
        if !ui.ctx().text_edit_focused() && !ui.input(|i| i.pointer.any_down()) {
            let redo = ui.input_mut(|i| {
                i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::Z)
                    || i.consume_key(egui::Modifiers::CTRL, egui::Key::Y)
            });
            if redo {
                self.step_history(true);
            } else if ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Z)) {
                self.step_history(false);
            }
        }
        if self.settings.is_none() && self.file_error.is_none() {
            if let Some(mode) = self.schematic.tool_shortcut(ui)
                && mode != Tool::Cursor
            {
                self.dock.reveal_schematic();
            }
        }
        let panel = Frame::new().fill(ui.visuals().panel_fill).inner_margin(6);
        let bar = Frame::new().inner_margin(egui::Margin::symmetric(8, 4));
        Panel::top("menus")
            .frame(
                Frame::new()
                    .fill(CHROME)
                    .inner_margin(egui::Margin::symmetric(8, 2)),
            )
            .show(ui, |ui| self.menu_bar(ui));
        if self.show_toolbar {
            Panel::top("toolbar")
                .frame(
                    Frame::new()
                        .fill(TOOLBAR)
                        .inner_margin(egui::Margin::symmetric(8, 3))
                        .stroke(egui::Stroke::new(1., CHROME_EDGE)),
                )
                .show(ui, |ui| self.toolbar(ui));
        }
        Panel::bottom("status")
            .frame(bar.fill(TOOLBAR))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.small(if matches!(self.schematic.tool, Tool::Place(_)) {
                        "Click the grid to place · Esc to cancel"
                    } else if self
                        .dock
                        .shows_waveforms(self.plot.as_ref().is_some_and(Plot::has_traces))
                    {
                        "Drag to box zoom   ·   Middle drag to pan   ·   Double-click to reset"
                    } else if self.schematic.has_operating_point() {
                        "Click a node for DC voltage or a component for DC current"
                    } else if self.plot.is_some()
                        && !self.edited
                        && self.schematic.tool == Tool::Cursor
                    {
                        "Click a wire for voltage or a component for current"
                    } else {
                        "Place components from the toolbar"
                    });
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        let (label, color) = match self.state {
                            State::Ready => ("Ready", Color32::LIGHT_GRAY),
                            State::Running(_) => ("Running…", Color32::LIGHT_BLUE),
                            State::Success if self.edited => {
                                ("Edited since run", Color32::LIGHT_YELLOW)
                            }
                            State::Success => ("Success", Color32::LIGHT_GREEN),
                            State::Failed(_) => ("Failed", Color32::LIGHT_RED),
                        };
                        ui.colored_label(color, label);
                    });
                });
            });
        if self.log_open {
            Panel::bottom("console").frame(panel).show(ui, |ui| {
                if ui.button("▾  ngspice output").clicked() {
                    self.log_open = false;
                    return;
                }
                ScrollArea::vertical().max_height(120.).show(ui, |ui| {
                    if let State::Failed(reason) = &self.state {
                        ui.colored_label(Color32::LIGHT_RED, reason);
                    }
                    if !self.log.is_empty() {
                        ui.monospace(&self.log);
                    } else if !matches!(self.state, State::Failed(_)) {
                        ui.weak("Waiting for simulator output.");
                    }
                });
            });
        }
        if self.show_netlist {
            let width = ui.available_width();
            Panel::right("netlist")
                .default_size(width * 0.26)
                .min_size(155.)
                .max_size((width * 0.35).max(155.))
                .resizable(true)
                .frame(panel)
                .show(ui, |ui| self.netlist_editor(ui));
        }

        egui::CentralPanel::default()
            .frame(panel.inner_margin(0))
            .show(ui, |ui| {
                let mut dock = std::mem::take(&mut self.dock);
                let mut probe = None;
                let probing =
                    self.plot.is_some() && !self.edited && matches!(self.state, State::Success);
                let had_traces = self.plot.as_ref().is_some_and(Plot::has_traces);
                let show_plot = self.plot.is_some() && !dock.waveform_closed();
                dock.show(ui, show_plot, |view, ui| match view {
                    View::Schematic => {
                        if self
                            .schematic
                            .show(ui, &mut self.netlist, probing.then_some(&mut probe))
                        {
                            self.commit_edit();
                        }
                    }
                    View::Waveforms => self.waveform_view(ui),
                });
                let now = ui.input(|input| input.time);
                if let Some((signal, isolate)) = probe
                    && let Some(plot) = &mut self.plot
                    && plot.probe(&signal, isolate)
                {
                    if isolate || dock.shows_waveforms(had_traces) {
                        self.probe_reveal_at = Some(now);
                    } else {
                        // Keep the schematic in place while egui waits for a second click.
                        let delay = ui
                            .ctx()
                            .options(|options| options.input_options.max_double_click_delay);
                        self.probe_reveal_at = Some(now + delay);
                    }
                }
                if self.plot.as_ref().is_some_and(Plot::has_traces) {
                    if let Some(deadline) = self.probe_reveal_at {
                        if now >= deadline {
                            self.probe_reveal_at = None;
                            dock.reveal_waveforms();
                            ui.ctx().request_repaint();
                        } else {
                            ui.ctx()
                                .request_repaint_after(std::time::Duration::from_secs_f64(
                                    deadline - now,
                                ));
                        }
                    }
                } else if had_traces {
                    dock.close_waveform();
                    self.probe_reveal_at = None;
                    ui.ctx().request_repaint();
                }
                self.dock = dock;
            });
        if let Some(mut settings) = self.settings.take() {
            let dialog = dialog(
                ui.ctx(),
                "simulation settings",
                "Configure Simulation",
                |ui| settings.show(ui),
            );
            if dialog.inner {
                if let Ok(deck) = settings.apply(&self.netlist) {
                    self.netlist = deck;
                    self.schematic.import(&self.netlist);
                    self.commit_edit();
                }
            } else if !dialog.should_close() {
                self.settings = Some(settings);
            }
        }
        if let Some(error) = &self.file_error {
            let modal = dialog(ui.ctx(), "file error", "Schematic file", |ui| {
                ui.label(error);
                dialog_actions(ui, "Close", true, false);
            });
            if modal.should_close() {
                self.file_error = None;
            }
        }
    }

    fn netlist_editor(&mut self, ui: &mut Ui) {
        ui.strong("Netlist");
        ui.separator();
        ui.add_space(2.);
        Frame::new()
            .fill(Color32::from_gray(44))
            .corner_radius(0)
            .inner_margin(10)
            .show(ui, |ui| {
                ui.set_min_height(ui.available_height());
                ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let edit = TextEdit::multiline(&mut self.netlist)
                            .font(FontId::monospace(14.))
                            .code_editor()
                            .frame(Frame::NONE)
                            .desired_width(f32::INFINITY);
                        if ui.add(edit).changed() {
                            crate::schematic::upgrade_pole_pins(&mut self.netlist);
                            self.schematic.import(&self.netlist);
                            self.commit_edit();
                        }
                    });
            });
    }

    fn waveform_view(&mut self, ui: &mut Ui) {
        if let Some(plot) = &mut self.plot {
            Frame::new()
                .fill(Color32::from_rgb(38, 48, 55))
                .inner_margin(4)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    plot.legend(ui);
                });
            if plot.has_traces() {
                plot.show(ui);
            } else {
                ui.label("Probe the schematic to add a trace.");
            }
        } else {
            let (r, p) = ui.allocate_painter(ui.available_size(), egui::Sense::hover());
            p.rect_filled(r.rect, 0, BACK);
            let message = match self.state {
                State::Running(_) => "Running ngspice…",
                State::Failed(_) => "Simulation failed. See ngspice output.",
                _ => "Run a simulation to display voltage traces.",
            };
            p.text(
                r.rect.center(),
                egui::Align2::CENTER_CENTER,
                message,
                FontId::proportional(15.),
                Color32::LIGHT_GRAY,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, FullOutput, Pos2, RawInput, Rect, vec2};
    use std::time::{Duration, Instant};

    fn frame(app: &mut Workbench, ctx: &Context, events: Vec<Event>) -> FullOutput {
        let mut output = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1140., 700.))),
                events,
                ..Default::default()
            },
            |ui| app.workspace(ui),
        );
        output.textures_delta.clear();
        output
    }

    fn locate(frame: &FullOutput, text: &str) -> Pos2 {
        frame
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Text(t) = &shape.shape
                    && t.galley.text().starts_with(text)
                {
                    let pos = t.pos + vec2(3., 5.);
                    assert!(shape.clip_rect.contains(pos), "{text} is clipped");
                    Some(pos)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| panic!("Missing {text}"))
    }

    fn has_label(output: &FullOutput, label: &str) -> bool {
        output.shapes.iter().any(
            |shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == label),
        )
    }

    fn click(app: &mut Workbench, ctx: &Context, pos: Pos2) {
        for pressed in [true, false] {
            frame(
                app,
                ctx,
                vec![
                    Event::PointerMoved(pos),
                    Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
    }

    fn drag(app: &mut Workbench, ctx: &Context, from: Pos2, to: Pos2) {
        frame(app, ctx, vec![Event::PointerMoved(from)]);
        frame(
            app,
            ctx,
            vec![Event::PointerButton {
                pos: from,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }],
        );
        frame(app, ctx, vec![Event::PointerMoved(to)]);
        frame(
            app,
            ctx,
            vec![Event::PointerButton {
                pos: to,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
    }

    fn finish_probe_delay(app: &mut Workbench, ctx: &Context) {
        assert!(app.probe_reveal_at.is_some());
        app.probe_reveal_at = Some(f64::NEG_INFINITY);
        frame(app, ctx, vec![]);
    }

    #[test]
    #[ignore = "requires ngspice on PATH"]
    fn schematic_probes_reopen_plot_and_show_voltage_and_current_readouts() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        let run = simulate(EXAMPLE);
        app.plot = Some(Plot::new(run.waveform.expect(&run.log)));
        app.dock.arrange(DockLayout::Rows(View::Waveforms));
        for (offset, trace) in [
            (vec2(14., 5.) * crate::schematic::GRID, "V(mid)"),
            (vec2(10., 5.) * crate::schematic::GRID, "I(r1)"),
        ] {
            app.dock.close_waveform();
            let output = frame(&mut app, &ctx, vec![]);
            let center = paper(&output).min + offset;
            let hover = frame(&mut app, &ctx, vec![Event::PointerMoved(center)]);
            assert_eq!(hover.platform_output.cursor_icon, egui::CursorIcon::None);
            click(&mut app, &ctx, center);
            assert!(app.dock.waveform_closed());
            finish_probe_delay(&mut app, &ctx);
            assert!(!app.dock.waveform_closed(), "Probe did not reveal {trace}");
        }
        frame(&mut app, &ctx, vec![]);
        let output = frame(
            &mut app,
            &ctx,
            vec![Event::PointerMoved(egui::pos2(470., 230.))],
        );
        let readout = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text().starts_with("x  ") => {
                    Some(text.galley.text())
                }
                _ => None,
            })
            .expect("missing corner readout");
        assert!(
            readout.contains("V(mid)") && readout.contains("I(r1)"),
            "{readout}"
        );
        for _ in 0..20 {
            frame(&mut app, &ctx, vec![]);
        }
        app.dock.close_waveform();
        let output = frame(&mut app, &ctx, vec![]);
        let resistor = paper(&output).min + vec2(10., 5.) * crate::schematic::GRID;
        click(&mut app, &ctx, resistor);
        assert!(app.dock.waveform_closed());
        click(&mut app, &ctx, resistor);
        assert!(!app.dock.waveform_closed());
        let output = frame(
            &mut app,
            &ctx,
            vec![Event::PointerMoved(egui::pos2(5., 450.))],
        );
        assert!(!has_label(&output, "Component properties"));
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(t) if t.galley.text().starts_with('C') && t.galley.text().ends_with("V(mid)")
        )));
        let point = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Path(line) if line.stroke.width == 1.8 && line.points.len() > 20 => {
                    line.points
                        .iter()
                        .copied()
                        .find(|&p| shape.clip_rect.shrink(12.).contains(p))
                }
                _ => None,
            })
            .expect("missing plotted current");
        frame(
            &mut app,
            &ctx,
            vec![
                Event::PointerMoved(point),
                Event::Key {
                    key: egui::Key::Delete,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                },
            ],
        );
        assert!(app.dock.waveform_closed());
        assert!(app.netlist.contains("R1 "));
        assert!(!app.plot.as_ref().unwrap().has_traces());
        let output = frame(&mut app, &ctx, vec![]);
        click(
            &mut app,
            &ctx,
            paper(&output).min + vec2(10., 5.) * crate::schematic::GRID,
        );
        assert!(app.plot.as_ref().unwrap().has_traces());
    }

    #[test]
    fn rotated_placement_preview_keeps_labels_clear_and_uses_default_value() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        frame(&mut app, &ctx, vec![]);
        click_tool(&mut app, &ctx, "Choose component");
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, locate(&output, "Inductor"));
        let output = frame(&mut app, &ctx, vec![]);
        assert!(!has_label(&output, "Value"));
        let center = paper(&output).min + vec2(35., 15.) * crate::schematic::GRID;
        let output = frame(
            &mut app,
            &ctx,
            vec![
                Event::PointerMoved(center),
                Event::Key {
                    key: egui::Key::R,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::CTRL,
                },
            ],
        );
        let symbol = Rect::from_center_size(center, vec2(28., 4. * crate::schematic::GRID));
        for label in ["L2", "1m"] {
            let bounds = output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.visual_bounding_rect())
                }
                _ => None,
            });
            assert!(!symbol.intersects(bounds.expect(label)));
        }
        click(&mut app, &ctx, center);
        assert!(
            app.netlist
                .lines()
                .any(|line| line.starts_with("L2 ") && line.ends_with(" 1m"))
        );
    }

    #[test]
    fn place_repeated_resistors_and_cancel() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        frame(&mut app, &ctx, vec![]);
        frame(&mut app, &ctx, vec![]);
        click_tool(&mut app, &ctx, "Choose component");
        let output = frame(&mut app, &ctx, vec![]);
        drag(
            &mut app,
            &ctx,
            locate(&output, "Resistor"),
            egui::pos2(410., 340.),
        );
        assert!(app.netlist.contains("R2 n"));
        let output = frame(&mut app, &ctx, vec![]);
        let target = paper(&output).min + vec2(140., 160.);
        click(&mut app, &ctx, target);
        click(&mut app, &ctx, target);
        click(&mut app, &ctx, target + vec2(160., 0.));
        assert!(app.netlist.contains("R2 n"));
        assert!(app.netlist.contains("R3 n"));
        assert!(app.netlist.contains("R4 n"));
        assert!(!app.netlist.contains("R5 "));
        frame(
            &mut app,
            &ctx,
            vec![Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        click(&mut app, &ctx, target + vec2(320., 0.));
        assert!(app.netlist.contains("R4 n"));
        assert!(!app.netlist.contains("R5 "));
        let placed = app.schematic.geometry();
        let count = app.undo.len();
        app.step_history(false);
        assert_eq!(app.undo.len(), count - 1);
        assert!(!app.netlist.contains("R4 "));
        assert!(placed != app.schematic.geometry());
        app.step_history(true);
        assert!(app.netlist.contains("R4 n") && placed == app.schematic.geometry());
    }

    #[test]
    fn right_click_reference_and_apply_properties() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        let pos = locate(&output, "R1");
        for pressed in [true, false] {
            frame(
                &mut app,
                &ctx,
                vec![
                    Event::PointerMoved(pos),
                    Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Secondary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        let output = frame(&mut app, &ctx, vec![]);
        locate(&output, "Component properties");
        click(&mut app, &ctx, locate(&output, "Apply"));
        let output = frame(&mut app, &ctx, vec![]);
        assert!(!has_label(&output, "Component properties"));
        assert!(app.netlist.contains("R1 in mid 47"));
    }

    #[test]
    fn change_diode_type_and_reopen() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        app.netlist = "Diode test\nX1 a 0 os_diode\n.op\n.end\n".into();
        app.schematic = Default::default();
        app.schematic.import(&app.netlist);
        app.schematic.fit_pending = true;
        frame(&mut app, &ctx, vec![]);
        let initial = frame(&mut app, &ctx, vec![]);
        let pos = locate(&initial, "X1");
        for pressed in [true, false] {
            frame(
                &mut app,
                &ctx,
                vec![
                    Event::PointerMoved(pos),
                    Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Secondary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        let popup = frame(&mut app, &ctx, vec![]);
        let label = locate(&popup, "Diode type");
        let field = popup
            .shapes
            .iter()
            .find_map(|shape| {
                let egui::Shape::Text(text) = &shape.shape else {
                    return None;
                };
                let center = text.pos + text.galley.size() / 2.;
                (text.galley.text() == "Small-signal diode"
                    && center.x > label.x
                    && (center.y - label.y).abs() < 10.)
                    .then_some(center)
            })
            .expect("The diode type field is beside its label");
        click(&mut app, &ctx, field);
        let choices = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, locate(&choices, "Schottky"));
        let popup = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, locate(&popup, "Apply"));
        assert!(app.netlist.contains("X1 a 0 os_schottky"));
        assert!(app.netlist.contains(".subckt os_schottky"));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("diode.cir");
        app.write_document(&path).unwrap();
        app.open_document(&path).unwrap();
        let output = frame(&mut app, &ctx, vec![]);
        assert!(has_label(&output, "Schottky"));
    }

    #[test]
    fn editor_run_failure_and_reload() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, locate(&output, "View"));
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, locate(&output, "Netlist"));
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, locate(&output, "RLC low-pass"));
        frame(&mut app, &ctx, vec![Event::Text("edited ".into())]);
        assert_ne!(app.netlist, EXAMPLE);
        app.netlist = "invalid test circuit\nRbad\n.tran 1u 1m\n.end\n".into();
        click(
            &mut app,
            &ctx,
            ctx.read_response(egui::Id::unique("Run the current netlist"))
                .unwrap()
                .rect
                .center(),
        );
        assert!(matches!(app.state, State::Running(_)));
        let deadline = Instant::now() + Duration::from_secs(10);
        while matches!(app.state, State::Running(_)) && Instant::now() < deadline {
            app.poll();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(matches!(app.state, State::Failed(_)) && app.log_open && app.plot.is_none());
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, locate(&output, "File"));
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, locate(&output, "Load Example"));
        assert_eq!(app.netlist, EXAMPLE);
    }

    #[test]
    fn displays_worker_result_and_clears_on_rerun() {
        for layout in [
            WaveformLayout::Above,
            WaveformLayout::Right,
            WaveformLayout::Tab,
        ] {
            let ctx = Context::default();
            let mut app = Workbench::new(&ctx, None);
            assert!(app.waveform_layout == WaveformLayout::Above && !app.show_netlist);
            app.waveform_layout = layout;
            let (tx, rx) = mpsc::channel();
            app.state = State::Running(rx);
            let mut raw = b"Plotname: Transient Analysis\nFlags: real\nNo. Variables: 3\nNo. Points: 2\nVariables:\n0 time time\n1 v(in) voltage\n2 v(out) voltage\nBinary:\n".to_vec();
            for value in [0.0_f64, 0., 0., 0.001, 5., 3.16] {
                raw.extend(value.to_ne_bytes());
            }
            tx.send(Run {
                waveform: crate::waveform::parse_raw(&raw),
                log: "fixture".into(),
            })
            .unwrap();
            app.poll();
            let output = frame(&mut app, &ctx, vec![]);
            assert!(app.dock.waveform_closed());
            assert!(!has_label(&output, "Waveforms"));
            click(
                &mut app,
                &ctx,
                paper(&output).min + vec2(6., 5.) * crate::schematic::GRID,
            );
            finish_probe_delay(&mut app, &ctx);
            let output = frame(&mut app, &ctx, vec![]);
            for label in ["Success", "C1   V(in)", "Timebase"] {
                locate(&output, label);
            }
            match layout {
                WaveformLayout::Above => {
                    assert!(locate(&output, "Waveforms").y < locate(&output, "Schematic").y);
                }
                WaveformLayout::Right => {
                    let waveform = locate(&output, "Waveforms");
                    let schematic = locate(&output, "Schematic");
                    assert!(waveform.x > schematic.x);
                    let divider = egui::pos2(waveform.x - 16., waveform.y + 130.);
                    drag(&mut app, &ctx, divider, divider + vec2(90., 0.));
                    let output = frame(&mut app, &ctx, vec![]);
                    assert!(locate(&output, "Waveforms").x > waveform.x + 50.);

                    click(
                        &mut app,
                        &ctx,
                        ctx.read_response(egui::Id::unique(("pane size", "Schematic")))
                            .unwrap()
                            .rect
                            .center(),
                    );
                    let output = frame(&mut app, &ctx, vec![]);
                    assert!(!has_label(&output, "Waveforms"));
                    frame(&mut app, &ctx, vec![]);
                    click(
                        &mut app,
                        &ctx,
                        ctx.read_response(egui::Id::unique(("pane size", "Schematic")))
                            .unwrap()
                            .rect
                            .center(),
                    );

                    let output = frame(&mut app, &ctx, vec![]);
                    drag(
                        &mut app,
                        &ctx,
                        locate(&output, "Waveforms"),
                        egui::pos2(40., 350.),
                    );
                    let output = frame(&mut app, &ctx, vec![]);
                    assert!(locate(&output, "Waveforms").x < locate(&output, "Schematic").x);
                    frame(&mut app, &ctx, vec![]);
                    click(
                        &mut app,
                        &ctx,
                        ctx.read_response(egui::Id::unique(("pane size", "Waveforms")))
                            .unwrap()
                            .rect
                            .center(),
                    );
                    let output = frame(&mut app, &ctx, vec![]);
                    assert!(!has_label(&output, "Schematic"));
                    frame(&mut app, &ctx, vec![]);
                    locate(&output, "Timebase");
                    click(
                        &mut app,
                        &ctx,
                        ctx.read_response(egui::Id::unique(("pane size", "Waveforms")))
                            .unwrap()
                            .rect
                            .center(),
                    );

                    let output = frame(&mut app, &ctx, vec![]);
                    drag(
                        &mut app,
                        &ctx,
                        locate(&output, "Waveforms"),
                        egui::pos2(570., 350.),
                    );
                    let output = frame(&mut app, &ctx, vec![]);
                    assert!(locate(&output, "Waveforms").x > locate(&output, "Schematic").x);
                    locate(&output, "Timebase");
                }
                WaveformLayout::Tab => {
                    click(&mut app, &ctx, locate(&output, "Schematic"));
                    let output = frame(&mut app, &ctx, vec![]);
                    locate(&output, "R1");
                    click(&mut app, &ctx, locate(&output, "Waveforms"));
                    locate(&frame(&mut app, &ctx, vec![]), "Timebase");
                }
            }
            let output = frame(&mut app, &ctx, vec![]);
            click(&mut app, &ctx, locate(&output, "×"));
            let output = frame(&mut app, &ctx, vec![]);
            assert!(app.dock.waveform_closed() && app.plot.is_some());
            assert!(!has_label(&output, "Waveforms"));
            locate(&output, "Schematic");
            click(
                &mut app,
                &ctx,
                ctx.read_response(egui::Id::unique("Run the current netlist"))
                    .unwrap()
                    .rect
                    .center(),
            );
            assert!(
                app.plot.is_none() && app.log.is_empty() && matches!(app.state, State::Running(_))
            );
        }
    }

    fn click_tool(app: &mut Workbench, ctx: &Context, hint: &str) {
        let pos = ctx
            .read_response(egui::Id::unique(hint))
            .unwrap()
            .rect
            .center();
        click(app, ctx, pos);
    }

    #[test]
    fn saves_edits_cancels_and_reopens_wired_layout() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.cir");
        let second = dir.path().join("second.cir");
        app.save_to(Some(first.clone()));
        let output = frame(&mut app, &ctx, vec![]);
        let origin = paper(&output).min;
        let resistor = origin + vec2(10., 5.) * crate::schematic::GRID;
        click_tool(&mut app, &ctx, "Move");
        click(&mut app, &ctx, resistor);
        click(&mut app, &ctx, resistor + vec2(0., 32.));
        app.netlist = app.netlist.replace("AC 1", "AC 2 45");
        app.commit_edit();
        assert!(app.saved != app.committed);
        app.save_to(None);
        assert_eq!(app.path.as_ref(), Some(&first));
        assert!(app.saved != app.committed);
        app.save_to(Some(dir.path().into()));
        assert!(app.file_error.take().is_some());
        assert_eq!(app.path.as_ref(), Some(&first));
        app.save_to(Some(second.clone()));
        assert!(app.saved == app.committed);
        assert!(!std::fs::read_to_string(&first).unwrap().contains("AC 2"));
        app.netlist = app.netlist.replace("AC 2 45", "AC 3 30");
        app.commit_edit();
        click_tool(&mut app, &ctx, "Save schematic (Ctrl+S)");
        let expected = app.committed.clone();
        app.load_example();
        app.open_document(&second).unwrap();
        assert!(app.saved == expected && app.schematic.geometry() == expected.geometry);
        assert!(app.netlist.contains("AC 3 30"));
    }

    #[test]
    fn power_toolbar_places_presets_and_opens_custom_voltage_editor() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        frame(&mut app, &ctx, vec![]);
        for (label, x) in [("12 V", 12.), ("Vcc", 19.)] {
            click_tool(&mut app, &ctx, "VCC power");
            let output = frame(&mut app, &ctx, vec![]);
            assert!(has_label(&output, "3.3 V") && has_label(&output, "5 V"));
            click(&mut app, &ctx, locate(&output, label));
            let output = frame(&mut app, &ctx, vec![]);
            click(
                &mut app,
                &ctx,
                paper(&output).min + vec2(x, 13.) * crate::schematic::GRID,
            );
        }
        assert!(app.netlist.contains("os_vcc_12 0 DC 12"));
        let output = frame(&mut app, &ctx, vec![]);
        let pos = locate(&output, "Vcc");
        for pressed in [true, false] {
            frame(
                &mut app,
                &ctx,
                vec![
                    Event::PointerMoved(pos),
                    Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Secondary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        let output = frame(&mut app, &ctx, vec![]);
        assert!(has_label(&output, "DC voltage (V)"));
    }

    #[test]
    fn transistor_toolbar_dropdowns_place_the_selected_variants() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        frame(&mut app, &ctx, vec![]);
        for (toolbar, variant, model, x) in [
            ("BJT", "PNP", "os_pnp", 12.),
            ("MOSFET", "NMOS", "os_nmos", 18.),
        ] {
            click_tool(&mut app, &ctx, toolbar);
            let output = frame(&mut app, &ctx, vec![]);
            click(&mut app, &ctx, locate(&output, variant));
            let output = frame(&mut app, &ctx, vec![]);
            let target = paper(&output).min + vec2(x, 15.) * crate::schematic::GRID;
            click(&mut app, &ctx, target);
            assert!(
                app.netlist
                    .lines()
                    .any(|line| line.starts_with('X') && line.ends_with(model))
            );
        }
    }

    #[test]
    fn copy_previews_single_parts_and_places_connected_groups() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        frame(&mut app, &ctx, vec![]);
        let origin = paper(&frame(&mut app, &ctx, vec![])).min;
        let grid = crate::schematic::GRID;
        let resistor = origin + vec2(10., 5.) * grid;
        let inductor = origin + vec2(18., 5.) * grid;
        let initial = app.schematic.geometry();

        click_tool(&mut app, &ctx, "Copy");
        click(&mut app, &ctx, resistor);
        let destination = resistor + vec2(0., 7.) * grid;
        let output = frame(&mut app, &ctx, vec![Event::PointerMoved(destination)]);
        assert_eq!(output.platform_output.cursor_icon, egui::CursorIcon::None);
        assert!(app.netlist == EXAMPLE && app.schematic.geometry() == initial);
        click(&mut app, &ctx, destination);
        assert!(
            app.netlist
                .lines()
                .any(|line| line.starts_with("R2 ") && line.ends_with(" 47"))
        );

        app.step_history(false);
        assert!(app.netlist == EXAMPLE && app.schematic.geometry() == initial);
        click_tool(&mut app, &ctx, "Copy");
        let release = inductor + vec2(1., 1.) * grid;
        drag(&mut app, &ctx, resistor - vec2(1., 1.) * grid, release);
        assert!(app.netlist == EXAMPLE && app.schematic.geometry() == initial);
        click(&mut app, &ctx, release + vec2(0., 7.) * grid);

        let nodes = |name: &str| {
            app.netlist
                .lines()
                .filter_map(|line| line.strip_prefix(name))
                .next()
                .unwrap()
                .split_whitespace()
                .collect::<Vec<_>>()
        };
        let resistor_copy = nodes("R2 ");
        let inductor_copy = nodes("L2 ");
        assert_eq!(resistor_copy[1], inductor_copy[0]);
        assert_ne!(resistor_copy[1], "mid");
        assert_eq!(nodes("R1 "), ["in", "mid", "47"]);
        assert_eq!(nodes("L1 "), ["mid", "out", "10m"]);
    }

    #[test]
    fn marquee_uses_pointer_coordinates_at_both_edges() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        frame(&mut app, &ctx, vec![]);
        let origin = paper(&frame(&mut app, &ctx, vec![])).min;
        let resistor = origin + vec2(10., 5.) * crate::schematic::GRID;
        click_tool(&mut app, &ctx, "Select");
        for (left, right, selected) in [(-25., -2., false), (2., 25., false), (-2., 2., true)] {
            drag(
                &mut app,
                &ctx,
                resistor + vec2(left, -21.),
                resistor + vec2(right, 21.),
            );
            frame(
                &mut app,
                &ctx,
                vec![
                    Event::PointerMoved(origin + vec2(500., 300.)),
                    Event::Key {
                        key: egui::Key::Delete,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: Default::default(),
                    },
                ],
            );
            assert_eq!(!app.netlist.contains("R1 "), selected);
            frame(
                &mut app,
                &ctx,
                vec![Event::Key {
                    key: egui::Key::Delete,
                    physical_key: None,
                    pressed: false,
                    repeat: false,
                    modifiers: Default::default(),
                }],
            );
        }
    }

    #[test]
    fn shift_click_toggles_parts_and_wires() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        frame(&mut app, &ctx, vec![]);
        let origin = paper(&frame(&mut app, &ctx, vec![])).min;
        click_tool(&mut app, &ctx, "Select");
        let resistor = origin + vec2(10., 5.) * crate::schematic::GRID;
        click(&mut app, &ctx, resistor);
        let inductor = origin + vec2(18., 5.) * crate::schematic::GRID;
        let wire = origin + vec2(14., 5.) * crate::schematic::GRID;
        for pos in [inductor, wire, inductor] {
            for pressed in [true, false] {
                frame(
                    &mut app,
                    &ctx,
                    vec![
                        Event::ModifiersChanged(egui::Modifiers::SHIFT),
                        Event::PointerMoved(pos),
                        Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::SHIFT,
                        },
                    ],
                );
            }
        }
        click_tool(&mut app, &ctx, "Delete");
        click(&mut app, &ctx, wire);
        assert!(!app.netlist.contains("R1 ") && app.netlist.contains("L1 "));
    }

    fn paper(output: &FullOutput) -> Rect {
        output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Rect(r) if r.fill == Color32::WHITE && r.rect.width() > 100. => {
                    Some(r.rect)
                }
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn pick_place_cancel_group_delete_pan_and_zoom() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        let origin = paper(&output).min;
        let resistor = origin + vec2(10., 5.) * crate::schematic::GRID;
        let initial = app.schematic.geometry();
        click_tool(&mut app, &ctx, "Move");
        click(&mut app, &ctx, resistor);
        frame(
            &mut app,
            &ctx,
            vec![Event::PointerMoved(resistor + vec2(32., 32.))],
        );
        assert!(app.schematic.geometry() != initial);
        frame(
            &mut app,
            &ctx,
            vec![Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        assert!(app.schematic.geometry() == initial);
        click_tool(&mut app, &ctx, "Move");
        click(&mut app, &ctx, resistor);
        let moved = resistor + vec2(0., 56.);
        click(&mut app, &ctx, moved);
        assert!(app.schematic.geometry() != initial);
        assert!(!app.netlist.contains("R1 in mid 47"));
        assert!(app.netlist.contains("L1 mid out 10m"));
        click_tool(&mut app, &ctx, "Delete");
        drag(
            &mut app,
            &ctx,
            resistor - vec2(35., 20.),
            moved + vec2(165., 20.),
        );
        assert!(!app.netlist.contains("R1 ") && !app.netlist.contains("L1 "));
        assert!(app.netlist.contains("V1 ") && app.netlist.contains("C1 "));
        app.step_history(false);
        let before = locate(&frame(&mut app, &ctx, vec![]), "R1");
        for (pos, pressed) in [(moved, true), (moved + vec2(48., 48.), false)] {
            frame(&mut app, &ctx, vec![Event::PointerMoved(pos)]);
            frame(
                &mut app,
                &ctx,
                vec![Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Middle,
                    pressed,
                    modifiers: Default::default(),
                }],
            );
        }
        let panned = locate(&frame(&mut app, &ctx, vec![]), "R1");
        assert!(panned.distance(before + vec2(48., 48.)) < 1.);
        click_tool(&mut app, &ctx, "Zoom schematic in");
        let enlarged = locate(&frame(&mut app, &ctx, vec![]), "R1");
        assert!(enlarged.distance(panned) > 1.);
        click_tool(&mut app, &ctx, "Zoom schematic out");
        assert!(locate(&frame(&mut app, &ctx, vec![]), "R1").distance(panned) < 1.);
        app.dock.close_waveform();
        click_tool(&mut app, &ctx, "Zoom schematic to fit");
        let fit = frame(&mut app, &ctx, vec![]);
        for name in ["R1", "L1", "C1", "V1"] {
            assert!(paper(&fit).shrink(4.).contains(locate(&fit, name)));
        }
    }

    #[test]
    #[ignore = "requires ngspice on PATH"]
    fn operating_point_automatically_labels_voltages_and_currents() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        app.netlist = "DC load\nV1 supply 0 3.3\nR1 supply 0 1k\n.op\n.end\n".into();
        app.schematic.import(&app.netlist);
        app.configure_simulation();
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, locate(&output, "Show all node voltages"));
        let output = frame(&mut app, &ctx, vec![]);
        click(
            &mut app,
            &ctx,
            locate(&output, "Show all component currents"),
        );
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, locate(&output, "Apply"));

        let flags = Settings::read(&app.netlist).op_display;
        assert!(flags.voltages && flags.currents);
        let (sender, receiver) = mpsc::channel();
        sender.send(simulate(&app.netlist)).unwrap();
        app.state = State::Running(receiver);
        app.edited = false;
        app.poll();
        let output = frame(&mut app, &ctx, vec![]);
        assert!(!has_label(&output, "0 V"));
        for annotation in ["3.3 V", "3.3 mA", "-3.3 mA"] {
            locate(&output, annotation);
        }
        assert!(app.dock.waveform_closed() && app.plot.is_none());
    }

    #[test]
    #[ignore = "requires ngspice on PATH"]
    fn configuration_runs_each_analysis() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        for (section, readout) in [
            ("Transient Analysis", "Timebase"),
            ("AC Analysis", "Frequency"),
            ("DC Operating Point", "0 V"),
        ] {
            frame(&mut app, &ctx, vec![]);
            click_tool(&mut app, &ctx, "Configure Simulation");
            let dialog = frame(&mut app, &ctx, vec![]);
            click(&mut app, &ctx, locate(&dialog, section));
            let dialog = frame(&mut app, &ctx, vec![]);
            click(&mut app, &ctx, locate(&dialog, "Apply"));
            assert!(app.settings.is_none(), "Apply did not close {section}");
            assert!(!matches!(app.state, State::Running(_)));
            frame(&mut app, &ctx, vec![]);
            click_tool(&mut app, &ctx, "Run the current netlist");
            let deadline = Instant::now() + Duration::from_secs(5);
            while matches!(app.state, State::Running(_)) && Instant::now() < deadline {
                app.poll();
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(matches!(app.state, State::Success), "{}", app.log);
            let output = frame(&mut app, &ctx, vec![]);
            assert!(!has_label(&output, "Waveforms"));
            click(
                &mut app,
                &ctx,
                paper(&output).min + vec2(6., 5.) * crate::schematic::GRID,
            );
            if section == "DC Operating Point" {
                let output = frame(&mut app, &ctx, vec![]);
                assert!(app.dock.waveform_closed() && app.plot.is_none());
                locate(&output, readout);
                click(
                    &mut app,
                    &ctx,
                    paper(&output).min + vec2(10., 5.) * crate::schematic::GRID,
                );
                let output = frame(&mut app, &ctx, vec![]);
                locate(&output, "0 A");
                assert!(!has_label(&output, "Waveforms"));
            } else {
                finish_probe_delay(&mut app, &ctx);
                let output = frame(&mut app, &ctx, vec![]);
                locate(&output, readout);
            }
        }
    }
}
