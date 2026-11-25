use crate::analysis::{Analysis, Settings};
use crate::dock::{Dock, Layout as DockLayout, View};
use crate::plot::{BACK, Plot};
use crate::schematic::{Schematic, Tool};
#[cfg(test)]
use crate::simulation::simulate;
use crate::simulation::{EXAMPLE, Run};
use eframe::egui::{
    self, Button, Color32, Context, FontId, Frame, Layout, Panel, ScrollArea, TextEdit, Ui,
};
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::task::{Poll, Wake, Waker};

const DIRECTIVE_START: &str = "* OpenSpice directives begin\n";
const DIRECTIVE_END: &str = "* OpenSpice directives end\n";

fn directive_block(deck: &str) -> Option<std::ops::Range<usize>> {
    let start = deck.find(DIRECTIVE_START)?;
    let end = deck[start + DIRECTIVE_START.len()..].find(DIRECTIVE_END)?;
    Some(start..start + DIRECTIVE_START.len() + end + DIRECTIVE_END.len())
}

fn directive_layout(
    ui: &Ui,
    buffer: &dyn egui::TextBuffer,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    for line in buffer.as_str().split_inclusive('\n') {
        let mut comment = line.trim_start().starts_with('*');
        for token in line.split_inclusive(char::is_whitespace) {
            let word = token.trim();
            comment |= word.starts_with('$') || word.starts_with(';');
            let color = if comment {
                Color32::from_rgb(134, 182, 146)
            } else if word.starts_with('.') || word == "+" {
                Color32::from_rgb(138, 195, 235)
            } else if word
                .trim_start_matches(['-', '+'])
                .starts_with(|c: char| c.is_ascii_digit())
            {
                Color32::from_rgb(236, 191, 134)
            } else {
                ui.visuals().text_color()
            };
            job.append(
                token,
                0.,
                egui::TextFormat::simple(FontId::monospace(14.), color),
            );
        }
    }
    job.wrap.max_width = width;
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

const CHROME: Color32 = Color32::from_rgb(22, 23, 26);
const TOOLBAR: Color32 = Color32::from_rgb(36, 38, 42);
pub(crate) const PANE_RADIUS: u8 = 6;
const CHROME_EDGE: Color32 = Color32::from_rgb(54, 57, 63);
const FIELD: Color32 = Color32::from_rgb(26, 27, 30);
const HOVER: Color32 = Color32::from_rgb(52, 56, 63);
const CYAN: Color32 = Color32::from_rgb(143, 183, 207);
const ACTIVE_TOOL: Color32 = Color32::from_rgb(56, 70, 84);
pub(crate) const TOOL_SIZE: egui::Vec2 = egui::vec2(36., 36.);

pub(crate) const DIALOG_FIELD_WIDTH: f32 = 140.;

#[derive(Clone, Copy)]
enum Icon {
    Save,
    Configure,
    Directives,
    Cursor,
    Select,
    Move,
    Delete,
    Copy,
    Comment,
    Run,
    Pause,
    Stop,
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
        .fill(TOOLBAR)
        .corner_radius(PANE_RADIUS)
        .stroke(egui::Stroke::new(1., CHROME_EDGE))
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
            ui.label(
                egui::RichText::new(title)
                    .size(16.)
                    .strong()
                    .color(Color32::from_gray(238)),
            );
            ui.separator();
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
        let button = Button::new(egui::RichText::new(primary).strong().color(Color32::WHITE))
            .fill(ACTIVE_TOOL)
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
        ACTIVE_TOOL
    } else if response.hovered() || response.has_focus() {
        HOVER
    } else {
        return Color32::from_gray(206);
    };
    ui.painter().rect_filled(response.rect, 3, fill);
    if active {
        ui.painter().line_segment(
            [
                response.rect.left_bottom() + egui::vec2(4., -1.),
                response.rect.right_bottom() + egui::vec2(-4., -1.),
            ],
            egui::Stroke::new(2., CYAN),
        );
    }
    Color32::WHITE
}

fn ribbon_group(ui: &mut Ui, label: &str, tools: impl FnOnce(&mut Ui)) {
    let start = ui.next_widget_position();
    tools(ui);
    ui.painter().text(
        egui::pos2(start.x + 2., ui.min_rect().top() - 2.),
        egui::Align2::LEFT_BOTTOM,
        label,
        FontId::proportional(10.),
        Color32::from_rgb(173, 190, 205),
    );
}

fn tool(ui: &mut Ui, enabled: bool, icon: Icon, active: bool, hint: &str) -> bool {
    let response = ui.add_enabled_ui(enabled, |ui| {
        let (_, rect) = ui.allocate_space(TOOL_SIZE);
        let response = ui.interact(rect, egui::Id::unique(hint), egui::Sense::click());
        let color = toolbar_surface(ui, &response, active);
        draw_icon(ui.painter(), rect.center(), icon, color, 1.25);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, hint));
        response.on_hover_text(hint).clicked()
    });
    response.inner
}

const PLAY_COLORS: [Color32; 3] = [
    Color32::from_rgb(96, 208, 122),
    Color32::from_rgb(44, 128, 73),
    Color32::from_rgb(187, 249, 176),
];
pub(crate) const CURRENT_ARROW_COLORS: [Color32; 3] = [
    Color32::from_rgb(75, 160, 243),
    Color32::from_rgb(34, 89, 163),
    Color32::from_rgb(180, 224, 255),
];

pub(crate) fn paint_play_triangle(
    painter: &egui::Painter,
    center: egui::Pos2,
    direction: egui::Vec2,
    size: f32,
    outline_width: f32,
    tail_length: f32,
    [fill, outline, highlight]: [Color32; 3],
) {
    let perpendicular = egui::vec2(-direction.y, direction.x);
    let point = |x, y| center + size * (direction * x + perpendicular * y);
    let points = |vertices: &[(f32, f32)]| vertices.iter().map(|&(x, y)| point(x, y)).collect();
    let head = [(-4.8, -7.), (7., 0.), (-4.8, 7.)];
    let border = egui::Stroke::new(outline_width, outline);
    if tail_length > 0. {
        let back = -4.8 - tail_length;
        let tail = [(back, -1.3), (-4.4, -1.3), (-4.4, 1.3), (back, 1.3)];
        painter.add(egui::Shape::convex_polygon(
            points(&tail),
            fill,
            egui::Stroke::NONE,
        ));
        painter.add(egui::Shape::convex_polygon(
            points(&head),
            fill,
            egui::Stroke::NONE,
        ));
        let perimeter = [
            head[0],
            head[1],
            head[2],
            (-4.8, 1.3),
            (back, 1.3),
            (back, -1.3),
            (-4.8, -1.3),
        ];
        painter.add(egui::Shape::closed_line(points(&perimeter), border));
        painter.add(egui::Shape::convex_polygon(
            points(&[(-3.5, -5.), (4., -0.5), (-3.5, -0.5)]),
            highlight,
            egui::Stroke::NONE,
        ));
    } else {
        let corners = [point(-5., -7.), point(7., 0.), point(-5., 7.)];
        let mut rounded = Vec::with_capacity(15);
        for (index, &corner) in corners.iter().enumerate() {
            let start = corner.lerp(corners[(index + 2) % 3], 0.13);
            let end = corner.lerp(corners[(index + 1) % 3], 0.13);
            for step in 0..=4 {
                let t = step as f32 / 4.;
                rounded.push(start.lerp(corner, t).lerp(corner.lerp(end, t), t));
            }
        }
        painter.add(egui::Shape::convex_polygon(rounded, fill, border));
        painter.line_segment(
            [point(-3.1, -3.4), point(-3.1, 3.4)],
            egui::Stroke::new(1.2 * size, highlight),
        );
    }
}

fn draw_icon(p: &egui::Painter, center: egui::Pos2, icon: Icon, color: Color32, scale: f32) {
    use egui::{Pos2, Stroke, vec2};
    let point = |x, y| -> Pos2 { center + vec2(x - 8., y - 8.) * scale };
    let pen = Stroke::new(1.25 * scale, color);
    let path = |coords: &[(f32, f32)]| {
        p.add(egui::Shape::line(
            coords.iter().map(|&(x, y)| point(x, y)).collect(),
            pen,
        ));
    };
    let polygon = |coords: &[(f32, f32)], fill, outline| {
        p.add(egui::Shape::convex_polygon(
            coords.iter().map(|&(x, y)| point(x, y)).collect(),
            fill,
            outline,
        ));
    };
    match icon {
        Icon::Save => {
            let slate = Color32::from_rgb(81, 84, 102);
            let silver = Color32::from_rgb(238, 235, 230);
            polygon(
                &[(0., 0.), (12., 0.), (16., 4.), (16., 16.), (0., 16.)],
                slate,
                Stroke::new(1., Color32::from_rgb(128, 132, 153)),
            );
            p.rect_filled(
                egui::Rect::from_min_max(point(4., 0.), point(12., 6.)),
                1,
                silver,
            );
            p.rect_filled(
                egui::Rect::from_min_max(point(9., 1.), point(11., 5.)),
                0,
                slate,
            );
            p.rect_filled(
                egui::Rect::from_min_max(point(3., 8.), point(13., 16.)),
                1,
                silver,
            );
            p.rect_filled(
                egui::Rect::from_min_max(point(3., 13.), point(13., 16.)),
                0,
                Color32::from_rgb(143, 190, 205),
            );
        }
        Icon::Configure => {
            let fill = Color32::from_rgb(133, 185, 224);
            let rim = Color32::from_rgb(48, 91, 131);
            let light = Color32::from_rgb(201, 231, 248);
            let gear = |hub: Pos2, radius: f32, phase: f32| {
                let radius = radius * scale;
                let perimeter: Vec<_> = (0..8)
                    .flat_map(|tooth| {
                        [(0.02, 0.75), (0.24, 1.), (0.68, 1.), (0.9, 0.75)].map(
                            |(offset, depth)| {
                                let angle =
                                    (tooth as f32 + offset + phase) * std::f32::consts::TAU / 8.;
                                hub + vec2(angle.cos(), angle.sin()) * (radius * depth)
                            },
                        )
                    })
                    .collect();
                let mut mesh = egui::Mesh::default();
                mesh.colored_vertex(hub, fill);
                for &vertex in &perimeter {
                    mesh.colored_vertex(vertex, fill);
                }
                let count = perimeter.len() as u32;
                for i in 0..count {
                    mesh.add_triangle(0, i + 1, (i + 1) % count + 1);
                }
                p.add(egui::Shape::mesh(mesh));
                p.add(egui::Shape::closed_line(perimeter, Stroke::new(scale, rim)));
                p.circle(hub, radius * 0.42, light, Stroke::NONE);
                p.circle(hub, radius * 0.31, FIELD, Stroke::new(0.65 * scale, rim));
            };
            gear(point(3.2, 12.5), 5.8, 0.5);
            gear(point(11., 4.3), 7.3, 0.);
            paint_play_triangle(
                p,
                point(14.7, 14.3),
                vec2(1., 0.),
                0.7 * scale,
                scale,
                0.,
                PLAY_COLORS,
            );
        }
        Icon::Pause => {
            for x in [1., 10.] {
                polygon(
                    &[(x, 1.), (x + 5., 1.), (x + 5., 16.), (x, 16.)],
                    Color32::from_rgb(245, 191, 86),
                    Stroke::new(scale, Color32::from_rgb(163, 110, 48)),
                );
                p.line_segment(
                    [point(x + 1., 3.), point(x + 4., 3.)],
                    Stroke::new(scale, Color32::LIGHT_YELLOW),
                );
            }
        }
        Icon::Stop => {
            polygon(
                &[
                    (4., 0.),
                    (12., 0.),
                    (16., 4.),
                    (16., 12.),
                    (12., 16.),
                    (4., 16.),
                    (0., 12.),
                    (0., 4.),
                ],
                Color32::from_rgb(225, 83, 86),
                Stroke::new(scale, Color32::from_rgb(145, 52, 60)),
            );
            p.line_segment(
                [point(4., 3.), point(12., 3.)],
                Stroke::new(scale, Color32::LIGHT_RED),
            );
        }
        Icon::Run => {
            paint_play_triangle(
                p,
                center,
                vec2(1., 0.),
                1.12 * scale,
                scale,
                0.,
                PLAY_COLORS,
            );
        }
        Icon::Directives => {
            let edge = Color32::from_rgb(151, 117, 59);
            polygon(
                &[(-2., 0.), (12., 0.), (18., 6.), (18., 16.), (-2., 16.)],
                Color32::from_rgb(255, 235, 186),
                Stroke::new(scale, edge),
            );
            polygon(
                &[(12., 0.), (18., 6.), (12., 6.)],
                Color32::from_rgb(245, 204, 110),
                Stroke::new(scale, edge),
            );
            p.text(
                point(8., 10.),
                egui::Align2::CENTER_CENTER,
                ".dir",
                FontId::monospace(8. * scale),
                Color32::from_rgb(59, 45, 28),
            );
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
            p.circle_stroke(point(3.5, 12.5), 2.5 * scale, pen);
            p.circle_stroke(point(11.5, 12.5), 2.5 * scale, pen);
            path(&[(5., 10.5), (14., 1.)]);
            path(&[(10., 10.5), (1., 1.)]);
        }
        Icon::Copy => {
            path(&[(2., 10.), (2., 1.), (11., 1.), (11., 4.)]);
            path(&[(5., 5.), (14., 5.), (14., 15.), (5., 15.), (5., 5.)]);
        }
        Icon::Comment => {
            let [green, rim, highlight] = PLAY_COLORS;
            polygon(
                &[
                    (0., 2.),
                    (2., 0.),
                    (14., 0.),
                    (16., 2.),
                    (16., 10.),
                    (14., 12.),
                    (2., 12.),
                    (0., 10.),
                ],
                green,
                Stroke::NONE,
            );
            polygon(&[(4., 11.), (10., 11.), (4., 16.)], green, Stroke::NONE);
            p.add(egui::Shape::closed_line(
                [
                    (0., 2.),
                    (2., 0.),
                    (14., 0.),
                    (16., 2.),
                    (16., 10.),
                    (14., 12.),
                    (8.8, 12.),
                    (4., 16.),
                    (4., 12.),
                    (2., 12.),
                    (0., 10.),
                ]
                .map(|(x, y)| point(x, y))
                .to_vec(),
                Stroke::new(scale, rim),
            ));
            p.line_segment(
                [point(2.5, 1.5), point(12.5, 1.5)],
                Stroke::new(scale, highlight),
            );
            let letter = Stroke::new(1.6 * scale, Color32::from_rgb(22, 60, 40));
            p.line_segment([point(5., 4.), point(11., 4.)], letter);
            p.line_segment([point(8., 4.), point(8., 9.)], letter);
        }
        Icon::Plus | Icon::Minus => {
            p.circle_stroke(point(6., 6.), 5. * scale, pen);
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

pub(crate) fn schematic_tool_cursor(
    ui: &Ui,
    position: egui::Pos2,
    clip: egui::Rect,
    tool: Tool,
    light: bool,
) {
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
    draw_icon(
        &painter,
        position,
        icon,
        Color32::from_gray(if light { 35 } else { 228 }),
        1.,
    );
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

#[derive(Clone, Copy)]
enum FileAction {
    Open,
    SaveAs,
}

enum FilePicker {
    Requested(FileAction),
    Waiting(FileAction, Pin<Box<dyn Future<Output = Option<PathBuf>>>>),
}

struct RepaintOnWake(Context);

impl Wake for RepaintOnWake {
    fn wake(self: Arc<Self>) {
        self.0.request_repaint();
    }
}

pub struct Workbench {
    schematic: crate::schematic::Schematic,
    committed: Revision,
    saved: Revision,
    path: Option<PathBuf>,
    file_error: Option<String>,
    file_picker: Option<FilePicker>,
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
    control: Arc<crate::simulation::Control>,
    progress: f64,
    updates: Option<Receiver<(f64, Option<crate::waveform::Waveform>)>>,
    live_plot: bool,
    show_directives: bool,
    show_toolbar: bool,
    current_hover: Option<String>,
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
        visuals.window_fill = TOOLBAR;
        visuals.window_stroke = egui::Stroke::new(1., CHROME_EDGE);
        visuals.extreme_bg_color = FIELD;
        visuals.selection.bg_fill = ACTIVE_TOOL;
        visuals.selection.stroke = egui::Stroke::new(1., Color32::WHITE);
        visuals.window_corner_radius = egui::CornerRadius::same(PANE_RADIUS);
        visuals.menu_corner_radius = egui::CornerRadius::same(PANE_RADIUS);
        for (widget, fill) in [
            (&mut visuals.widgets.noninteractive, TOOLBAR),
            (&mut visuals.widgets.inactive, CHROME),
            (&mut visuals.widgets.hovered, HOVER),
            (&mut visuals.widgets.active, ACTIVE_TOOL),
            (&mut visuals.widgets.open, HOVER),
        ] {
            widget.corner_radius = egui::CornerRadius::same(3);
            widget.bg_fill = fill;
            widget.weak_bg_fill = fill;
            widget.bg_stroke = egui::Stroke::new(1., CHROME_EDGE);
            widget.expansion = 0.;
        }
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
        schematic.light_canvas = storage
            .and_then(|s| s.get_string("light_schematic"))
            .as_deref()
            != Some("false");
        schematic.import(EXAMPLE);
        schematic.center_pending = true;
        Self {
            path: None,
            file_error: None,
            file_picker: None,
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
            control: Default::default(),
            progress: 0.,
            updates: None,
            live_plot: false,
            show_directives: false,
            show_toolbar: true,
            current_hover: None,
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
        self.open_document_text(&file, Some(path.into()))
    }

    fn open_document_text(&mut self, file: &str, path: Option<PathBuf>) -> Result<(), String> {
        let mut text = file.to_owned();
        let american = self.schematic.american_resistor;
        let light_canvas = self.schematic.light_canvas;
        let mut schematic = Schematic::default();
        schematic.american_resistor = american;
        schematic.light_canvas = light_canvas;
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
        self.reset_document(path);
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

    fn request_file_picker(&mut self, action: FileAction, ctx: &Context) {
        if self.file_picker.is_none() {
            self.file_picker = Some(FilePicker::Requested(action));
            ctx.request_repaint();
        }
    }

    fn save_action(&mut self, save_as: bool, ctx: &Context) {
        if self.file_picker.is_some() {
            return;
        }
        if !save_as && let Some(path) = self.path.clone() {
            self.save_to(Some(path));
        } else {
            self.request_file_picker(FileAction::SaveAs, ctx);
        }
    }

    fn poll_file_picker(&mut self, ctx: &Context, frame: &eframe::Frame) {
        let Some(picker) = self.file_picker.take() else {
            return;
        };
        let (action, mut future) = match picker {
            FilePicker::Waiting(action, future) => (action, future),
            FilePicker::Requested(action) => {
                let dialog = rfd::AsyncFileDialog::new().set_parent(frame);
                let future: Pin<Box<dyn Future<Output = Option<PathBuf>>>> = match action {
                    FileAction::Open => {
                        let pick = dialog
                            .add_filter("SPICE schematic", &["cir", "sp", "net"])
                            .pick_file();
                        Box::pin(async move { pick.await.map(|file| file.path().to_owned()) })
                    }
                    FileAction::SaveAs => {
                        let mut dialog = dialog
                            .set_title("Save schematic as")
                            .add_filter("SPICE schematic", &["cir"]);
                        if let Some(parent) = self.path.as_deref().and_then(Path::parent) {
                            dialog = dialog.set_directory(parent);
                        }
                        let filename = self
                            .path
                            .as_deref()
                            .and_then(Path::file_name)
                            .and_then(|name| name.to_str())
                            .unwrap_or("circuit.cir");
                        let save = dialog.set_file_name(filename).save_file();
                        Box::pin(async move { save.await.map(|file| file.path().to_owned()) })
                    }
                };
                (action, future)
            }
        };
        let waker = Waker::from(Arc::new(RepaintOnWake(ctx.clone())));
        match future
            .as_mut()
            .poll(&mut std::task::Context::from_waker(&waker))
        {
            Poll::Pending => self.file_picker = Some(FilePicker::Waiting(action, future)),
            Poll::Ready(None) => {}
            Poll::Ready(Some(path)) => match action {
                FileAction::Open => {
                    if let Err(e) = self.open_document(&path) {
                        self.file_error = Some(format!("Cannot open {}: {e}", path.display()));
                    }
                }
                FileAction::SaveAs => self.save_to(Some(path)),
            },
        }
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
        self.control = Default::default();
        self.control
            .live
            .store(self.live_plot, std::sync::atomic::Ordering::Relaxed);
        let control = self.control.clone();
        self.progress = 0.;
        let (updates, receiver) = mpsc::sync_channel(1);
        self.updates = Some(receiver);
        self.state = State::Running(rx);
        let netlist = self.netlist.clone();
        let ctx = ctx.clone();
        if let Err(e) = std::thread::Builder::new()
            .name("ngspice".into())
            .spawn(move || {
                let _ = tx.send(crate::simulation::controlled(
                    &netlist,
                    &control,
                    |progress, wave| {
                        let _ = updates.try_send((progress, wave));
                        ctx.request_repaint();
                    },
                ));
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
        if let Some((progress, wave)) = self.updates.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.progress = progress;
            if let Some(data) = wave.filter(|w| w.analysis != Analysis::Op) {
                if let Some(plot) = &mut self.plot {
                    plot.refresh(data);
                } else {
                    self.plot = Some(Plot::new(data));
                }
            }
        }
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
                        if self.control.mode.load(std::sync::atomic::Ordering::Relaxed) != 2 {
                            self.progress = 1.;
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
            let circuit_changed = self.committed.text != next.text
                || !self.committed.geometry.same_circuit(&next.geometry);
            self.undo.push(std::mem::replace(&mut self.committed, next));
            if self.undo.len() > 128 {
                self.undo.remove(0);
            }
            self.redo.clear();
            if circuit_changed {
                self.edited = true;
                self.schematic.clear_operating_point();
                self.probe_reveal_at = None;
            }
        }
    }
    fn step_history(&mut self, redo: bool) {
        let (from, to) = if redo {
            (&mut self.redo, &mut self.undo)
        } else {
            (&mut self.undo, &mut self.redo)
        };
        if let Some(next) = from.pop() {
            let circuit_changed = self.committed.text != next.text
                || !self.committed.geometry.same_circuit(&next.geometry);
            self.netlist.clone_from(&next.text);
            self.schematic.restore(next.geometry.clone(), &next.text);
            to.push(std::mem::replace(&mut self.committed, next));
            if circuit_changed {
                self.edited = true;
                self.schematic.clear_operating_point();
                self.probe_reveal_at = None;
            }
        }
    }

    fn load_example(&mut self) {
        let american = self.schematic.american_resistor;
        let light_canvas = self.schematic.light_canvas;
        self.schematic = Default::default();
        self.schematic.american_resistor = american;
        self.schematic.light_canvas = light_canvas;
        self.dock = Dock::default();
        self.netlist = EXAMPLE.into();
        self.schematic.import(EXAMPLE);
        self.schematic.center_pending = true;
        self.reset_document(None);
    }

    fn menu_bar(&mut self, ui: &mut Ui) {
        let ready = !matches!(self.state, State::Running(_));
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                let file_ready = self.file_picker.is_none();
                if ui
                    .add_enabled(ready && file_ready, Button::new("Open…"))
                    .clicked()
                {
                    ui.close();
                    self.request_file_picker(FileAction::Open, ui.ctx());
                }
                for (save_as, label) in [
                    (false, "Save    Ctrl+S"),
                    (true, "Save As…    Ctrl+Shift+S"),
                ] {
                    if ui.add_enabled(file_ready, Button::new(label)).clicked() {
                        ui.close();
                        self.save_action(save_as, ui.ctx());
                    }
                }
                ui.separator();
                ui.add_enabled_ui(ready && file_ready, |ui| {
                    ui.menu_button("Load Examples", |ui| {
                        if ui.button("RLC circuit").clicked() {
                            self.load_example();
                            ui.close();
                        }
                        for (name, document) in [
                            (
                                "Full bridge rectifier",
                                include_str!("../examples/full-bridge-rectifier.cir"),
                            ),
                            (
                                "Class A amplifier with a transformer and MOSFETs",
                                include_str!(
                                    "../examples/class-a-transformer-mosfet-amplifier.cir"
                                ),
                            ),
                        ] {
                            if ui.button(name).clicked() {
                                if let Err(error) = self.open_document_text(document, None) {
                                    self.file_error = Some(error);
                                }
                                ui.close();
                            }
                        }
                    });
                });
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
                if ui
                    .checkbox(&mut self.schematic.light_canvas, "Light schematic")
                    .changed()
                {
                    ui.close();
                }
                ui.checkbox(&mut self.show_toolbar, "Toolbar");
                if ui
                    .checkbox(&mut self.show_directives, "SPICE directives")
                    .changed()
                {
                    ui.close();
                }
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
                if ui
                    .checkbox(&mut self.live_plot, "Plot during simulation")
                    .changed()
                {
                    self.control
                        .live
                        .store(self.live_plot, std::sync::atomic::Ordering::Relaxed);
                }
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
                                ("Ctrl+E", "Mirror horizontally"),
                                ("Ctrl+Shift+E", "Mirror vertically"),
                                ("W", "Wire"),
                                ("T", "Add comment"),
                                (".", "Toggle SPICE directives"),
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
            ribbon_group(ui, "FILE", |ui| {
                if tool(
                    ui,
                    self.file_picker.is_none(),
                    Icon::Save,
                    false,
                    "Save schematic (Ctrl+S)",
                ) {
                    self.save_action(false, ui.ctx());
                }
            });
            ui.separator();
            ribbon_group(ui, "SIMULATION", |ui| {
                let ready = !matches!(self.state, State::Running(_));
                if tool(ui, ready, Icon::Configure, false, "Configure Simulation") {
                    self.configure_simulation();
                }
                use std::sync::atomic::Ordering::Relaxed;
                let mode = self.control.mode.load(Relaxed);
                if tool(
                    ui,
                    ready || mode == 1,
                    Icon::Run,
                    false,
                    "Run the current netlist",
                ) {
                    if ready {
                        self.start(ui.ctx());
                    } else {
                        self.control.mode.store(0, Relaxed);
                    }
                }
                if tool(
                    ui,
                    !ready && mode == 0,
                    Icon::Pause,
                    false,
                    "Pause simulation",
                ) {
                    self.control.mode.store(1, Relaxed);
                }
                if tool(
                    ui,
                    !ready && mode != 2,
                    Icon::Stop,
                    false,
                    "Stop simulation",
                ) {
                    self.control.mode.store(2, Relaxed);
                }
                if tool(
                    ui,
                    true,
                    Icon::Directives,
                    self.show_directives,
                    "SPICE directives (.)",
                ) {
                    self.show_directives = !self.show_directives;
                }
            });
            ui.add_space(4.);
            ui.separator();
            ui.add_space(4.);
            ribbon_group(ui, "EDIT", |ui| {
                for (mode, icon, label) in [
                    (Tool::Cursor, Icon::Cursor, "Cursor"),
                    (Tool::Select, Icon::Select, "Select"),
                    (Tool::Move, Icon::Move, "Move"),
                    (Tool::Erase, Icon::Delete, "Delete"),
                    (Tool::Copy, Icon::Copy, "Copy"),
                    (Tool::Comment, Icon::Comment, "Add comment (T)"),
                ] {
                    if tool(ui, true, icon, self.schematic.tool == mode, label) {
                        self.schematic.cancel();
                        self.schematic.tool = mode;
                        self.dock.reveal_schematic();
                    }
                }
            });
            ui.add_space(4.);
            ui.separator();
            ui.add_space(4.);
            ribbon_group(ui, "PLACEMENT", |ui| {
                if self.schematic.picker(ui) {
                    self.dock.reveal_schematic();
                }
            });

            let zoom_width = ui.available_width().max(164.);
            ui.allocate_ui_with_layout(
                egui::vec2(zoom_width, TOOL_SIZE.y),
                Layout::right_to_left(egui::Align::Center),
                |ui| {
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
                },
            );
        });
    }
}

impl eframe::App for Workbench {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string("light_schematic", self.schematic.light_canvas.to_string());
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

    fn logic(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        self.poll();
        self.poll_file_picker(ctx, frame);
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
                    self.save_action(save_as, ui.ctx());
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
            if !ui.ctx().text_edit_focused()
                && !ui.ctx().memory(|memory| memory.top_modal_layer().is_some())
                && !egui::Popup::is_any_open(ui.ctx())
                && ui.input_mut(|input| {
                    input.modifiers == egui::Modifiers::NONE
                        && input.consume_key(egui::Modifiers::NONE, egui::Key::Period)
                })
            {
                self.show_directives = !self.show_directives;
            }
            if let Some(mode) = self.schematic.tool_shortcut(ui)
                && mode != Tool::Cursor
            {
                self.dock.reveal_schematic();
            }
        }
        ui.painter()
            .rect_filled(ui.available_rect_before_wrap(), 0, CHROME);
        let pane_frame = Frame::new()
            .fill(TOOLBAR)
            .stroke(egui::Stroke::new(1., CHROME_EDGE))
            .corner_radius(PANE_RADIUS)
            .inner_margin(6)
            .outer_margin(egui::Margin {
                left: 4,
                top: 2,
                right: 4,
                bottom: 2,
            });
        let bar = Frame::new().inner_margin(egui::Margin::symmetric(8, 4));
        Panel::top("menus")
            .show_separator_line(false)
            .frame(
                Frame::new()
                    .fill(CHROME)
                    .inner_margin(egui::Margin::symmetric(8, 2)),
            )
            .show(ui, |ui| self.menu_bar(ui));
        if self.show_toolbar {
            Panel::top("toolbar")
                .show_separator_line(false)
                .frame(
                    Frame::new()
                        .fill(TOOLBAR)
                        .corner_radius(PANE_RADIUS)
                        .outer_margin(egui::Margin::symmetric(4, 2))
                        .inner_margin(egui::Margin {
                            left: 8,
                            right: 8,
                            top: 16,
                            bottom: 3,
                        })
                        .stroke(egui::Stroke::new(1., CHROME_EDGE)),
                )
                .show(ui, |ui| self.toolbar(ui));
        }
        Panel::bottom("status")
            .show_separator_line(false)
            .frame(bar.fill(CHROME))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.small(if self.schematic.writing_comment() {
                        "Enter to place · Shift+Enter for a new line · Esc to cancel"
                    } else if self.schematic.tool == Tool::Comment {
                        "Click the schematic to add a comment · Esc to cancel"
                    } else if matches!(self.schematic.tool, Tool::Place(_)) {
                        "Click the grid to place · Esc to cancel"
                    } else if self
                        .dock
                        .shows_waveforms(self.plot.as_ref().is_some_and(Plot::has_traces))
                    {
                        "Drag to box zoom   ·   Middle drag to pan   ·   Double-click to reset"
                    } else if self.schematic.has_operating_point() {
                        "Click for DC voltage or current · Drag between nodes to measure the difference"
                    } else if self.plot.is_some()
                        && !self.edited
                        && self.schematic.tool == Tool::Cursor
                    {
                        "Click for voltage or current · Drag between nodes to measure the difference"
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
                        let mode = self.control.mode.load(std::sync::atomic::Ordering::Relaxed);
                        let label = match (&self.state, mode) {
                            (State::Running(_),1) => "Paused",
                            (State::Running(_),2) => "Stopping",
                            (State::Success,2) => "Stopped",
                            _ => label,
                        };
                        if matches!(self.state,State::Running(_) | State::Success) { ui.label(format!("{:.0}%",self.progress*100.)); }
                        ui.colored_label(color, label);
                    });
                });
            });
        if self.log_open {
            let max_height = (ui.available_height() * 0.65).max(72.);
            Panel::bottom("console")
                .show_separator_line(false)
                .default_size(145.)
                .min_size(72.)
                .max_size(max_height)
                .resizable(true)
                .frame(pane_frame)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.strong("ngspice output");
                        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .small_button("×")
                                .on_hover_text("Close ngspice output")
                                .clicked()
                            {
                                self.log_open = false;
                            }
                        });
                    });
                    ui.separator();
                    ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
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
        if self.show_directives {
            Panel::left("directives pane")
                .resizable(true)
                .show_separator_line(false)
                .default_size(310.)
                .min_size(180.)
                .frame(pane_frame)
                .show(ui, |ui| self.directives_editor(ui));
        }
        if self.schematic.editor_open() || self.schematic.selector_open {
            Panel::right("component editor")
                .resizable(true)
                .default_size(350.)
                .min_size(300.)
                .frame(pane_frame)
                .show(ui, |ui| {
                    if self.schematic.selector_open {
                        let height = ui.available_height()
                            * if self.schematic.editor_open() {
                                0.45
                            } else {
                                1.0
                            };
                        ui.allocate_ui(egui::vec2(ui.available_width(), height), |ui| {
                            self.schematic.component_selector(ui)
                        });
                        ui.separator();
                    }
                    if self.schematic.component_editor(ui, &mut self.netlist) {
                        self.commit_edit();
                    }
                });
        }
        if self.show_netlist {
            let width = ui.available_width();
            Panel::right("netlist")
                .show_separator_line(false)
                .default_size(width * 0.26)
                .min_size(155.)
                .max_size((width * 0.35).max(155.))
                .resizable(true)
                .frame(pane_frame)
                .show(ui, |ui| self.netlist_editor(ui));
        }

        egui::CentralPanel::default()
            .frame(Frame::new().inner_margin(egui::Margin::symmetric(4, 2)))
            .show(ui, |ui| {
                let mut dock = std::mem::take(&mut self.dock);
                let mut probe = None;
                let probing = self.plot.is_some()
                    && !self.edited
                    && matches!(self.state, State::Success | State::Running(_));
                let had_traces = self.plot.as_ref().is_some_and(Plot::has_traces);
                let show_plot = self.plot.is_some() && !dock.waveform_closed();
                let previous_hover = self.current_hover.take();
                let mut current_hover = None;
                dock.show(ui, show_plot, |view, ui| match view {
                    View::Schematic => {
                        if self.schematic.show(
                            ui,
                            &mut self.netlist,
                            probing.then_some(&mut probe),
                            if show_plot && probing {
                                previous_hover.as_deref()
                            } else {
                                None
                            },
                        ) {
                            self.commit_edit();
                        }
                    }
                    View::Waveforms => current_hover = self.waveform_view(ui),
                });
                if current_hover != previous_hover {
                    ui.ctx().request_repaint();
                }
                self.current_hover = current_hover;
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

    fn directives_editor(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.strong("SPICE directives");
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("×")
                    .on_hover_text("Close directives")
                    .clicked()
                {
                    self.show_directives = false;
                }
            });
        });
        ui.separator();
        let range = directive_block(&self.netlist);
        let mut draft = range
            .clone()
            .map(|range| {
                self.netlist[range]
                    .strip_prefix(DIRECTIVE_START)
                    .unwrap()
                    .strip_suffix(DIRECTIVE_END)
                    .unwrap()
                    .strip_suffix('\n')
                    .unwrap_or("")
                    .to_owned()
            })
            .unwrap_or_default();
        let height = ui.available_height();
        let changed = ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add(
                    TextEdit::multiline(&mut draft)
                        .code_editor()
                        .hint_text(".param gain=10")
                        .desired_width(f32::INFINITY)
                        .min_size(egui::vec2(0., (height - 8.).max(24.)))
                        .layouter(&mut directive_layout),
                )
                .changed()
            })
            .inner;
        if changed {
            let replace = range.unwrap_or_else(|| {
                let offset = self
                    .netlist
                    .split_inclusive('\n')
                    .take_while(|line| !line.trim().eq_ignore_ascii_case(".end"))
                    .map(str::len)
                    .sum();
                offset..offset
            });
            let mut block = String::new();
            if !draft.is_empty() {
                if replace.start > 0 && !self.netlist[..replace.start].ends_with('\n') {
                    block.push('\n');
                }
                block.push_str(DIRECTIVE_START);
                block.push_str(&draft);
                block.push('\n');
                block.push_str(DIRECTIVE_END);
            }
            self.netlist.replace_range(replace, &block);
            self.schematic.import(&self.netlist);
            self.commit_edit();
        }
    }

    fn netlist_editor(&mut self, ui: &mut Ui) {
        ui.strong("Netlist");
        ui.separator();
        ui.add_space(2.);
        Frame::new()
            .fill(FIELD)
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

    fn waveform_view(&mut self, ui: &mut Ui) -> Option<String> {
        if let Some(plot) = &mut self.plot {
            Frame::new().inner_margin(4).show(ui, |ui| {
                plot.legend(ui);
            });
            if plot.has_traces() {
                return plot.show(ui);
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
        None
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
    fn pending_file_picker_allows_frames_and_applies_the_result_once() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        let framework = eframe::Frame::_new_kittest();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("picked.cir");

        for (action, choice) in [
            (FileAction::SaveAs, None),
            (FileAction::SaveAs, Some(path.clone())),
            (FileAction::Open, Some(path.clone())),
        ] {
            let mut waiting = true;
            app.file_picker = Some(FilePicker::Waiting(
                action,
                Box::pin(std::future::poll_fn(move |_ctx| {
                    if std::mem::take(&mut waiting) {
                        Poll::Pending
                    } else {
                        Poll::Ready(choice.clone())
                    }
                })),
            ));
            let before = app.path.clone();
            app.poll_file_picker(&ctx, &framework);
            assert!(matches!(app.file_picker, Some(FilePicker::Waiting(..))));
            assert_eq!(app.path, before);
            let output = frame(&mut app, &ctx, vec![]);
            locate(&output, "R1");
            app.poll_file_picker(&ctx, &framework);
            assert!(app.file_picker.is_none());
            assert!(app.file_error.is_none());
        }

        assert_eq!(app.path.as_deref(), Some(path.as_path()));
        assert!(std::fs::read_to_string(&path).unwrap().contains("R1"));
    }

    #[test]
    fn hovering_negative_current_draws_the_reference_arrow_on_the_schematic() {
        use crate::waveform::{Quantity, Trace, Waveform};
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        app.plot = Some(Plot::new(Waveform {
            analysis: Analysis::Transient,
            logarithmic_frequency: false,
            time: vec![0., 1.],
            traces: vec![Trace {
                name: "I(r1)".into(),
                quantity: Quantity::Current,
                samples: vec![-0.001; 2],
                phasors: None,
            }],
        }));
        app.state = State::Success;
        app.plot.as_mut().unwrap().probe("I(r1)", false);
        app.dock.arrange(DockLayout::Rows(View::Waveforms));
        let output = frame(&mut app, &ctx, vec![]);
        let point = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Path(path) if path.stroke.width == 1.8 && path.points.len() == 2 => {
                    Some(path.points[0].lerp(path.points[1], 0.5))
                }
                _ => None,
            })
            .unwrap();
        frame(&mut app, &ctx, vec![Event::PointerMoved(point)]);
        assert_eq!(app.current_hover.as_deref(), Some("I(r1)"));

        let output = frame(&mut app, &ctx, vec![]);
        assert!(output.shapes.iter().any(|shape| match &shape.shape {
            egui::Shape::Path(path)
                if path.fill == CURRENT_ARROW_COLORS[0] && path.points.len() == 3 =>
            {
                let [a, tip, b] = [path.points[0], path.points[1], path.points[2]];
                tip.x > a.x && tip.x > b.x && paper(&output).contains(tip)
            }
            _ => false,
        }));
        frame(&mut app, &ctx, vec![Event::PointerGone]);
        assert!(app.current_hover.is_none());

        let output = frame(&mut app, &ctx, vec![]);
        let resistor = paper(&output).min
            + app
                .schematic
                .grid_offset(vec2(10., 5.) * crate::schematic::GRID);
        for pressed in [true, false] {
            frame(
                &mut app,
                &ctx,
                vec![
                    Event::ModifiersChanged(egui::Modifiers::CTRL),
                    Event::PointerMoved(resistor),
                    Event::PointerButton {
                        pos: resistor,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::CTRL,
                    },
                ],
            );
        }
        let output = frame(&mut app, &ctx, vec![]);
        locate(&output, "C2   -I(R1)");
    }

    #[test]
    fn voltage_probe_drag_plots_the_difference_without_moving_the_circuit() {
        use crate::waveform::{Quantity, Trace, Waveform};

        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        app.plot = Some(Plot::new(Waveform {
            analysis: Analysis::Transient,
            logarithmic_frequency: false,
            time: vec![0., 1.],
            traces: [("V(in)", 5.), ("V(mid)", 2.)]
                .map(|(name, value)| Trace {
                    name: name.into(),
                    quantity: Quantity::Voltage,
                    samples: vec![0., value],
                    phasors: None,
                })
                .into(),
        }));
        app.state = State::Success;
        app.dock.close_waveform();
        let output = frame(&mut app, &ctx, vec![]);
        let start = paper(&output).min
            + app
                .schematic
                .grid_offset(vec2(6., 5.) * crate::schematic::GRID);
        let end = paper(&output).min
            + app
                .schematic
                .grid_offset(vec2(14., 5.) * crate::schematic::GRID);
        let before = app.schematic.geometry();
        drag(&mut app, &ctx, start, end);
        assert!(before == app.schematic.geometry());
        finish_probe_delay(&mut app, &ctx);
        let output = frame(&mut app, &ctx, vec![]);
        let legend = locate(&output, "C3   V(in,mid)");
        for pressed in [true, false] {
            frame(
                &mut app,
                &ctx,
                vec![
                    Event::PointerMoved(legend),
                    Event::PointerButton {
                        pos: legend,
                        button: egui::PointerButton::Secondary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        let output = frame(&mut app, &ctx, vec![]);
        for label in ["Peak to peak", "RMS", "Mean", "Minimum", "Maximum"] {
            locate(&output, label);
        }
        click(&mut app, &ctx, locate(&output, "RMS"));
        let output = frame(&mut app, &ctx, vec![]);
        locate(&output, "RMS 1.73 V");
        let refreshed = Waveform {
            analysis: Analysis::Transient,
            logarithmic_frequency: false,
            time: vec![0., 1.],
            traces: [("V(in)", 8.), ("V(mid)", 2.)]
                .map(|(name, value)| Trace {
                    name: name.into(),
                    quantity: Quantity::Voltage,
                    samples: vec![0., value],
                    phasors: None,
                })
                .into(),
        };
        app.plot.as_mut().unwrap().refresh(refreshed);
        let output = frame(&mut app, &ctx, vec![]);
        locate(&output, "RMS 3.46 V");
        assert!(app.plot.as_mut().unwrap().probe("V(in)", false));
        let output = frame(&mut app, &ctx, vec![]);
        locate(&output, "C1   V(in)");
        locate(&output, "RMS 3.46 V");
        assert!(!has_label(&output, "RMS 4.62 V"));
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
            let center = paper(&output).min + app.schematic.grid_offset(offset);
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
        let resistor = paper(&output).min
            + app
                .schematic
                .grid_offset(vec2(10., 5.) * crate::schematic::GRID);
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
        let point = paper(&output).min
            + app
                .schematic
                .grid_offset(vec2(10., 5.) * crate::schematic::GRID);
        click(&mut app, &ctx, point);
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
    fn custom_transformer_places_and_reopens_with_winding_dots() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        frame(&mut app, &ctx, vec![]);
        click_tool(&mut app, &ctx, "Choose component");
        let output = frame(&mut app, &ctx, vec![]);
        let first = locate(&output, "Resistor");
        frame(
            &mut app,
            &ctx,
            vec![
                Event::PointerMoved(first),
                Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: vec2(0., -500.),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                    modifiers: Default::default(),
                },
            ],
        );
        for _ in 0..20 {
            frame(&mut app, &ctx, vec![]);
        }
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, locate(&output, "Custom transformer"));
        let output = frame(&mut app, &ctx, vec![]);
        locate(&output, "Primary windings");
        locate(&output, "Secondary windings");
        let place = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "Place" => {
                    Some(text.pos + text.galley.size() / 2.)
                }
                _ => None,
            })
            .unwrap();
        click(&mut app, &ctx, place);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, paper(&output).center() + vec2(250., 0.));
        assert!(app.netlist.contains("os_xfmr_multi_1_2 ratio=1"));
        assert!(app.netlist.contains(".subckt os_xfmr_multi_1_2"));

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("transformer.cir");
        app.write_document(&path).unwrap();
        app.open_document(&path).unwrap();
        let output = frame(&mut app, &ctx, vec![]);
        locate(&output, "X1");
        let radius = 2. * app.schematic.zoom_percent() / 100.;
        let dots = output
            .shapes
            .iter()
            .filter(|shape| {
                matches!(&shape.shape,
                    egui::Shape::Circle(circle) if (circle.radius - radius).abs() < 0.01
                )
            })
            .count();
        assert_eq!(dots, 3);
    }

    #[test]
    fn edit_legacy_pmos_model_parameters_and_reopen() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        app.netlist = "PMOS test\nX1 d g 0 os_pmos\n.op\n.end\n".into();
        app.schematic.import(&app.netlist);
        let output = frame(&mut app, &ctx, vec![]);
        let pos = locate(&output, "X1");
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
        click(&mut app, &ctx, locate(&output, "-1"));
        let output = frame(
            &mut app,
            &ctx,
            vec![
                Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                },
                Event::Text("-1.5".into()),
            ],
        );
        click(&mut app, &ctx, locate(&output, "Apply"));
        assert!(
            app.netlist
                .contains("X1 d g 0 os_pmos_adjust vto=-1.5 kp=1m")
        );
        assert!(app.netlist.contains(".subckt os_pmos_adjust"));
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("pmos.cir");
        app.write_document(&path).unwrap();
        app.open_document(&path).unwrap();
        assert!(app.netlist.contains("vto=-1.5"));
        locate(&frame(&mut app, &ctx, vec![]), "X1");
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
        click(&mut app, &ctx, locate(&output, "Load Examples"));
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, locate(&output, "RLC circuit"));
        assert_eq!(app.netlist, EXAMPLE);
    }

    #[test]
    fn displays_worker_result_and_preserves_plot_on_rerun() {
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
            let point = paper(&output).min
                + app
                    .schematic
                    .grid_offset(vec2(6., 5.) * crate::schematic::GRID);
            click(&mut app, &ctx, point);
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
                app.plot.is_some() && app.log.is_empty() && matches!(app.state, State::Running(_))
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
        let origin =
            paper(&frame(&mut app, &ctx, vec![])).min + app.schematic.grid_offset(vec2(0., 0.));
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
        let resistor = origin
            + app
                .schematic
                .grid_offset(vec2(10., 5.) * crate::schematic::GRID);
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
        let origin =
            paper(&frame(&mut app, &ctx, vec![])).min + app.schematic.grid_offset(vec2(0., 0.));
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
                egui::Shape::Rect(r)
                    if (r.fill == crate::schematic::PAPER || r.fill == Color32::WHITE)
                        && r.rect.width() > 100. =>
                {
                    Some(r.rect)
                }
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn directives_edit_save_reopen_and_undo() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        let original = app.netlist.clone();
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, locate(&output, "View"));
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, locate(&output, "SPICE directives"));
        let output = frame(&mut app, &ctx, vec![]);
        assert!(locate(&output, "SPICE directives").x < paper(&output).left());
        let period = || Event::Key {
            key: egui::Key::Period,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        };
        frame(&mut app, &ctx, vec![period()]);
        assert!(!app.show_directives);
        click_tool(&mut app, &ctx, "SPICE directives (.)");
        assert!(app.show_directives);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, locate(&output, ".param gain=10"));
        frame(&mut app, &ctx, vec![period()]);
        assert!(app.show_directives);
        let directives = ".param scale=2\n.options reltol=0.001";
        frame(&mut app, &ctx, vec![Event::Paste(directives.into())]);
        assert!(app.netlist.contains(directives));
        assert!(app.netlist.find(directives).unwrap() < app.netlist.find(".end").unwrap());
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("directives.cir");
        app.write_document(&path).unwrap();
        app.step_history(false);
        assert_eq!(app.netlist, original);
        app.open_document(&path).unwrap();
        assert!(app.netlist[directive_block(&app.netlist).unwrap()].contains(directives));
    }

    #[test]
    fn comment_can_be_added_moved_saved_and_deleted() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        let output = frame(&mut app, &ctx, vec![]);
        let anchor = paper(&output).center() + vec2(0., 126.);
        frame(
            &mut app,
            &ctx,
            vec![Event::Key {
                key: egui::Key::T,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        assert!(app.schematic.tool == Tool::Comment);
        click(&mut app, &ctx, anchor);
        frame(
            &mut app,
            &ctx,
            vec![
                Event::Text("Check the rise time".into()),
                Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::SHIFT,
                },
                Event::Text("Compare both traces".into()),
            ],
        );
        let output = frame(&mut app, &ctx, vec![]);
        assert!(!has_label(&output, "Schematic comment"));
        assert!(locate(&output, "Check the rise time").distance(anchor) < 20.);
        assert!(has_label(
            &output,
            "Check the rise time\nCompare both traces"
        ));
        let extra = " and make sure this deliberately long sentence stays on the same line";
        frame(&mut app, &ctx, vec![Event::Text(extra.into())]);
        let contents = format!("Check the rise time\nCompare both traces{extra}");
        let output = frame(&mut app, &ctx, vec![]);
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.text() == contents && text.galley.rows.len() == 2
        )));
        frame(
            &mut app,
            &ctx,
            vec![Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );

        let output = frame(&mut app, &ctx, vec![]);
        let original = locate(&output, "Check the rise time");
        assert!(original.distance(anchor) < 20.);
        click_tool(&mut app, &ctx, "Move");
        click(&mut app, &ctx, original);
        let delta = vec2(56., 28.);
        click(&mut app, &ctx, original + delta);
        let output = frame(&mut app, &ctx, vec![]);
        assert!(locate(&output, "Check the rise time").distance(original + delta) < 1.);
        assert_eq!(app.netlist, EXAMPLE);
        assert!(!app.edited);

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("comments.cir");
        app.write_document(&path).unwrap();
        app.open_document(&path).unwrap();
        let output = frame(&mut app, &ctx, vec![]);
        let comment = locate(&output, "Check the rise time");
        click_tool(&mut app, &ctx, "Delete");
        click(&mut app, &ctx, comment);
        assert!(!has_label(&frame(&mut app, &ctx, vec![]), &contents));
        assert_eq!(app.netlist, EXAMPLE);
        app.step_history(false);
        assert!(has_label(&frame(&mut app, &ctx, vec![]), &contents));
    }

    #[test]
    fn schematic_centers_on_startup_and_fits_when_waveforms_open() {
        use crate::waveform::{Quantity, Trace, Waveform};

        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        let output = frame(&mut app, &ctx, vec![]);
        let canvas = paper(&output);
        let resistor = locate(&output, "R1");
        assert!((resistor.x - canvas.center().x).abs() < 130.);
        assert!((resistor.y - canvas.center().y).abs() < 90.);
        assert!((app.schematic.zoom_percent() - 100.).abs() < 0.1);

        app.schematic.zoom(5., None);
        let zoomed = app.schematic.zoom_percent();
        app.plot = Some(Plot::new(Waveform {
            analysis: Analysis::Transient,
            logarithmic_frequency: false,
            time: vec![0., 1.],
            traces: vec![Trace {
                name: "v(out)".into(),
                quantity: Quantity::Voltage,
                samples: vec![0., 1.],
                phasors: None,
            }],
        }));
        app.dock.arrange(DockLayout::Rows(View::Waveforms));
        let output = frame(&mut app, &ctx, vec![]);
        let split_canvas = paper(&output);
        assert!(split_canvas.height() < canvas.height());
        assert!(app.schematic.zoom_percent() < zoomed);
        for name in ["V1", "R1", "L1", "C1"] {
            assert!(split_canvas.shrink(12.).contains(locate(&output, name)));
        }

        app.dock.close_waveform();
        let output = frame(&mut app, &ctx, vec![]);
        assert_eq!(paper(&output), canvas);
        let resistor = locate(&output, "R1");
        assert!((resistor.y - canvas.center().y).abs() < 100.);
    }

    #[test]
    fn light_schematic_keeps_the_canvas_in_place() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        let output = frame(&mut app, &ctx, vec![]);
        let bounds = paper(&output);
        assert!(app.schematic.light_canvas);
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Rect(r) if r.fill == Color32::WHITE && r.rect == bounds
        )));
        click(&mut app, &ctx, locate(&output, "View"));
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, locate(&output, "Light schematic"));

        let output = frame(&mut app, &ctx, vec![]);
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Rect(r) if r.fill == crate::schematic::PAPER && r.rect == bounds
        )));
        app.load_example();
        assert!(!app.schematic.light_canvas);
    }

    #[test]
    fn dragging_console_edge_resizes_the_output_and_schematic() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        app.log_open = true;
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        let before = paper(&output);
        let title = locate(&output, "ngspice output");
        let console = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Rect(r) if r.fill == TOOLBAR && r.rect.contains(title) => Some(r.rect),
                _ => None,
            })
            .unwrap();
        let edge = Pos2::new(console.center().x, console.top() - 3.);
        drag(&mut app, &ctx, edge, edge - vec2(0., 65.));
        let output = frame(&mut app, &ctx, vec![]);
        assert!(paper(&output).height() < before.height() - 40.);
        assert!(locate(&output, "ngspice output").y < title.y - 40.);
    }

    #[test]
    fn keyboard_delete_keeps_the_schematic_visible_in_the_deletion_frame() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        let output = frame(&mut app, &ctx, vec![]);
        let canvas = paper(&output);
        let resistor = canvas.min
            + app
                .schematic
                .grid_offset(vec2(10., 5.) * crate::schematic::GRID);
        frame(&mut app, &ctx, vec![Event::PointerMoved(resistor)]);
        let deletion_frame = frame(
            &mut app,
            &ctx,
            vec![Event::Key {
                key: egui::Key::Delete,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );

        assert!(!app.netlist.lines().any(|line| line.starts_with("R1 ")));
        assert_eq!(paper(&deletion_frame), canvas);
        for reference in ["V1", "L1", "C1"] {
            locate(&deletion_frame, reference);
        }
        let updated = frame(&mut app, &ctx, vec![]);
        assert!(!has_label(&updated, "R1"));
    }

    #[test]
    fn moving_a_component_keeps_stationary_labels_fixed() {
        for use_move_tool in [false, true] {
            let ctx = Context::default();
            let mut app = Workbench::new(&ctx, None);
            let output = frame(&mut app, &ctx, vec![]);
            let canvas = paper(&output);
            let start = canvas.min
                + app
                    .schematic
                    .grid_offset(vec2(10., 5.) * crate::schematic::GRID);
            let fixed = ["V1", "L1", "C1"].map(|label| locate(&output, label));
            let destination = start + vec2(28., 70.);
            let button = |pos, pressed| Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            if use_move_tool {
                click_tool(&mut app, &ctx, "Move");
                click(&mut app, &ctx, start);
            } else {
                frame(&mut app, &ctx, vec![Event::PointerMoved(start)]);
                frame(&mut app, &ctx, vec![button(start, true)]);
            }

            for pos in [start + vec2(0., 28.), destination] {
                frame(&mut app, &ctx, vec![Event::PointerMoved(pos)]);
                let preview = frame(&mut app, &ctx, vec![]);
                assert_eq!(
                    fixed,
                    ["V1", "L1", "C1"].map(|label| locate(&preview, label))
                );
            }
            if use_move_tool {
                click(&mut app, &ctx, destination);
            } else {
                frame(&mut app, &ctx, vec![button(destination, false)]);
            }
            let after = frame(&mut app, &ctx, vec![]);
            assert_eq!(fixed, ["V1", "L1", "C1"].map(|label| locate(&after, label)));
            assert_eq!(
                locate(&after, "R1") - locate(&output, "R1"),
                destination - start,
            );
        }
    }

    #[test]
    fn component_labels_move_and_delete_independently_and_rename_restores_them() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        let first = frame(&mut app, &ctx, vec![]);
        let original = app.schematic.geometry();
        let start = locate(&first, "R1");
        let delta = vec2(0., -3. * crate::schematic::GRID);
        drag(&mut app, &ctx, start, start + delta);
        let output = frame(&mut app, &ctx, vec![]);
        assert_eq!(locate(&output, "R1"), start + delta);
        assert_eq!(locate(&output, "47"), locate(&first, "47") + delta);
        assert_eq!(locate(&output, "L1"), locate(&first, "L1"));

        click_tool(&mut app, &ctx, "Move");
        let picked_up = start + delta;
        let destination = picked_up + vec2(2. * crate::schematic::GRID, 0.);
        click(&mut app, &ctx, picked_up);
        click(&mut app, &ctx, destination);
        let output = frame(&mut app, &ctx, vec![]);
        assert_eq!(locate(&output, "R1"), destination);
        let moved = app.schematic.geometry();
        assert!(moved != original && moved.same_circuit(&original));
        assert_eq!(app.netlist, EXAMPLE);
        assert!(!app.edited);

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("labels.cir");
        app.write_document(&path).unwrap();
        app.open_document(&path).unwrap();
        assert!(app.schematic.geometry() == moved);

        let output = frame(&mut app, &ctx, vec![]);
        click_tool(&mut app, &ctx, "Delete");
        click(&mut app, &ctx, locate(&output, "R1"));
        let output = frame(&mut app, &ctx, vec![]);
        assert!(!has_label(&output, "R1") && !has_label(&output, "47"));
        assert!(app.schematic.geometry().same_circuit(&original));
        assert_eq!(app.netlist, EXAMPLE);
        app.step_history(false);
        assert!(has_label(&frame(&mut app, &ctx, vec![]), "R1"));
        app.step_history(true);
        app.write_document(&path).unwrap();
        app.open_document(&path).unwrap();
        let output = frame(&mut app, &ctx, vec![]);
        assert!(!has_label(&output, "R1"));

        let resistor = paper(&output).min
            + app
                .schematic
                .grid_offset(vec2(10., 5.) * crate::schematic::GRID);
        for pressed in [true, false] {
            frame(
                &mut app,
                &ctx,
                vec![
                    Event::PointerMoved(resistor),
                    Event::PointerButton {
                        pos: resistor,
                        button: egui::PointerButton::Secondary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, locate(&output, "Edit component"));
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, locate(&output, "R1"));
        let output = frame(
            &mut app,
            &ctx,
            vec![
                Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                },
                Event::Text("R10".into()),
            ],
        );
        click(&mut app, &ctx, locate(&output, "Apply"));
        let output = frame(&mut app, &ctx, vec![]);
        assert!(has_label(&output, "R10") && has_label(&output, "47"));
        assert!(app.netlist.contains("R10 in mid 47"));
    }

    #[test]
    fn pick_place_cancel_group_delete_pan_and_zoom() {
        let ctx = Context::default();
        let mut app = Workbench::new(&ctx, None);
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        let origin = paper(&output).min + app.schematic.grid_offset(vec2(0., 0.));
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
        let voltage = locate(&output, "3.3 V");
        let current = locate(&output, "3.3 mA");
        let highlighted_net = frame(&mut app, &ctx, vec![Event::PointerMoved(voltage)]);
        assert!(
            highlighted_net
                .shapes
                .iter()
                .any(|shape| matches!(&shape.shape,
                    egui::Shape::Path(path) if path.points.len() == 2 && path.stroke.width > 2.5
                ))
        );
        let highlighted_component = frame(&mut app, &ctx, vec![Event::PointerMoved(current)]);
        let paths = |output: &FullOutput| {
            output
                .shapes
                .iter()
                .filter(|shape| matches!(&shape.shape, egui::Shape::Path(_)))
                .count()
        };
        assert!(paths(&highlighted_component) > paths(&output));
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
            let point = paper(&output).min
                + app
                    .schematic
                    .grid_offset(vec2(6., 5.) * crate::schematic::GRID);
            click(&mut app, &ctx, point);
            if section == "DC Operating Point" {
                let output = frame(&mut app, &ctx, vec![]);
                assert!(app.dock.waveform_closed() && app.plot.is_none());
                locate(&output, readout);
                let point = paper(&output).min
                    + app
                        .schematic
                        .grid_offset(vec2(10., 5.) * crate::schematic::GRID);
                click(&mut app, &ctx, point);
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
