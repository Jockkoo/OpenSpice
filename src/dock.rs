use eframe::egui::{self, Button, Color32, CursorIcon, Rect, Sense, Stroke, StrokeKind, Ui};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Schematic,
    Waveforms,
}

impl View {
    fn title(self) -> &'static str {
        match self {
            Self::Schematic => "Schematic",
            Self::Waveforms => "Waveforms",
        }
    }

    fn other(self) -> Self {
        match self {
            Self::Schematic => Self::Waveforms,
            Self::Waveforms => Self::Schematic,
        }
    }
}

#[derive(Clone, Copy)]
pub enum Layout {
    Rows(View),
    Columns(View),
    Tabs,
}

pub struct Dock {
    waveform_closed: bool,
    layout: Layout,
    active: View,
    maximized: Option<View>,
    dragging: Option<View>,
    row_fraction: f32,
    column_fraction: f32,
}

impl Default for Dock {
    fn default() -> Self {
        Self {
            waveform_closed: false,
            layout: Layout::Tabs,
            active: View::Schematic,
            maximized: None,
            dragging: None,
            row_fraction: 0.58,
            column_fraction: 0.43,
        }
    }
}

impl Dock {
    pub fn arrange(&mut self, layout: Layout) {
        self.waveform_closed = false;
        self.layout = layout;
        self.active = View::Waveforms;
        self.maximized = None;
        self.dragging = None;
    }

    pub fn waveform_closed(&self) -> bool {
        self.waveform_closed
    }
    pub fn close_waveform(&mut self) {
        self.waveform_closed = true;
        self.active = View::Schematic;
        self.maximized = None;
        self.dragging = None;
    }

    pub fn reveal_schematic(&mut self) {
        self.active = View::Schematic;
        if self.maximized == Some(View::Waveforms) {
            self.maximized = None;
        }
    }

    pub fn reveal_waveforms(&mut self) {
        self.waveform_closed = false;
        self.active = View::Waveforms;
        if self.maximized == Some(View::Schematic) {
            self.maximized = None;
        }
    }

    pub fn shows_waveforms(&self, has_plot: bool) -> bool {
        if self.waveform_closed {
            return false;
        }
        if let Some(view) = self.maximized.filter(|_| has_plot) {
            return view == View::Waveforms;
        }
        if matches!(self.layout, Layout::Tabs) {
            return self.active == View::Waveforms;
        }
        has_plot
    }

    pub fn show(&mut self, ui: &mut Ui, has_plot: bool, mut content: impl FnMut(View, &mut Ui)) {
        let area = ui.available_rect_before_wrap();
        if self.waveform_closed {
            self.pane(ui, area, &[View::Schematic], false, &mut content);
            return;
        }
        let split = match (self.layout, has_plot, self.maximized) {
            (Layout::Rows(first), true, None) => Some((first, true)),
            (Layout::Columns(first), true, None) => Some((first, false)),
            _ => None,
        };

        if let Some((first, rows)) = split {
            let fraction = if rows {
                &mut self.row_fraction
            } else {
                &mut self.column_fraction
            };
            let extent = if rows { area.height() } else { area.width() };
            let length = (extent - 6.).max(1.);
            let minimum = if rows { 110. } else { 180. };
            let limit = (minimum / length).min(0.45);
            let offset = length * fraction.clamp(limit, 1. - limit);
            let (first_rect, divider, second_rect) = if rows {
                let y = area.top() + offset;
                let divider = Rect::from_x_y_ranges(area.x_range(), y..=y + 6.);
                (
                    Rect::from_min_max(area.min, divider.right_top()),
                    divider,
                    Rect::from_min_max(divider.left_bottom(), area.max),
                )
            } else {
                let x = area.left() + offset;
                let divider = Rect::from_x_y_ranges(x..=x + 6., area.y_range());
                (
                    Rect::from_min_max(area.min, divider.left_bottom()),
                    divider,
                    Rect::from_min_max(divider.right_top(), area.max),
                )
            };
            let handle = ui
                .interact(divider, ui.scope_id().with("pane divider"), Sense::drag())
                .on_hover_and_drag_cursor(if rows {
                    CursorIcon::ResizeVertical
                } else {
                    CursorIcon::ResizeHorizontal
                });
            if handle.dragged() {
                let delta = handle.drag_delta();
                *fraction = ((offset + if rows { delta.y } else { delta.x }) / length)
                    .clamp(limit, 1. - limit);
            }
            if handle.hovered() || handle.dragged() {
                ui.painter()
                    .rect_filled(divider.shrink(1.), 2, Color32::from_gray(85));
            }
            self.pane(ui, first_rect, &[first], true, &mut content);
            self.pane(ui, second_rect, &[first.other()], true, &mut content);
        } else if let Some(view) = self.maximized.filter(|_| has_plot) {
            self.pane(ui, area, &[view], true, &mut content);
        } else {
            let tabs = [View::Schematic, View::Waveforms];
            let count = if has_plot || matches!(self.layout, Layout::Tabs) {
                2
            } else {
                1
            };
            self.pane(ui, area, &tabs[..count], has_plot, &mut content);
        }

        if let Some(view) = self.dragging {
            let (pointer, down, released, escape) = ui.input(|input| {
                (
                    input.pointer.latest_pos(),
                    input.pointer.primary_down(),
                    input.pointer.button_released(egui::PointerButton::Primary),
                    input.key_pressed(egui::Key::Escape),
                )
            });
            let target = pointer.filter(|&point| area.contains(point)).map(|point| {
                let left = (point.x - area.left()) / area.width();
                let top = (point.y - area.top()) / area.height();
                let (distance, layout, preview) = [
                    (
                        left,
                        Layout::Columns(view),
                        Rect::from_min_max(area.min, area.center_bottom()),
                    ),
                    (
                        1. - left,
                        Layout::Columns(view.other()),
                        Rect::from_min_max(area.center_top(), area.max),
                    ),
                    (
                        top,
                        Layout::Rows(view),
                        Rect::from_min_max(area.min, area.right_center()),
                    ),
                    (
                        1. - top,
                        Layout::Rows(view.other()),
                        Rect::from_min_max(area.left_center(), area.max),
                    ),
                ]
                .into_iter()
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .unwrap();
                if distance < 0.25 {
                    (layout, preview)
                } else {
                    (Layout::Tabs, area.shrink2(area.size() * 0.25))
                }
            });
            if down && !escape {
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            }
            if !escape && let Some((layout, preview)) = target {
                let color = ui.visuals().selection.stroke.color;
                ui.painter()
                    .rect_filled(preview, 0, color.gamma_multiply(0.22));
                ui.painter()
                    .rect_stroke(preview, 0, Stroke::new(2., color), StrokeKind::Inside);
                if released {
                    self.arrange(layout);
                    self.active = view;
                }
            }
            if !down || escape {
                self.dragging = None;
            }
        }
    }

    fn pane(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        tabs: &[View],
        has_plot: bool,
        content: &mut impl FnMut(View, &mut Ui),
    ) {
        let single = tabs.len() == 1;
        let mut active = if single { tabs[0] } else { self.active };
        let content_scope = ui.scope_id().with("pane content");
        let mut pane = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(tabs[0].title())
                .max_rect(rect.shrink(6.)),
        );
        pane.set_clip_rect(ui.clip_rect().intersect(rect));
        let radius = crate::ui::PANE_RADIUS;
        pane.painter().rect(
            rect,
            radius,
            pane.visuals().window_fill,
            pane.visuals().window_stroke,
            StrokeKind::Inside,
        );
        pane.horizontal(|ui| {
            for &view in tabs {
                let sense = if has_plot {
                    Sense::click_and_drag()
                } else {
                    Sense::click()
                };
                let title = egui::RichText::new(view.title());
                let title = if view == active {
                    title.strong()
                } else {
                    title.weak()
                };
                let mut response = ui.add(Button::new(title).frame(false).sense(sense));
                if !single && view == active {
                    let rect = response.rect.shrink2(egui::vec2(5., 1.));
                    ui.painter().line_segment(
                        [rect.left_bottom(), rect.right_bottom()],
                        Stroke::new(1., ui.visuals().weak_text_color()),
                    );
                }
                if has_plot {
                    response = response
                        .on_hover_cursor(CursorIcon::Grab)
                        .on_hover_text("Drag to dock this tab");
                }
                if response.clicked() && !single {
                    active = view;
                    self.active = view;
                }
                if response.drag_started_by(egui::PointerButton::Primary) {
                    self.dragging = Some(view);
                }
                if view == View::Waveforms
                    && ui
                        .small_button("×")
                        .on_hover_text("Close waveforms")
                        .clicked()
                {
                    self.close_waveform();
                    ui.ctx().request_repaint();
                }
            }
            if single && has_plot {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if self.maximized.is_some() {
                        "Restore"
                    } else {
                        "Maximize"
                    };
                    let size = egui::vec2(25., 20.);
                    let (_, rect) = ui.allocate_space(size);
                    let response = ui.interact(
                        rect,
                        egui::Id::unique(("pane size", active.title())),
                        Sense::click(),
                    );
                    let style = ui.style().interact(&response);
                    ui.painter().rect_filled(rect, 3, style.bg_fill);
                    for x in [-1., 1.] {
                        for y in [-1., 1.] {
                            let direction = egui::vec2(x, y);
                            let restoring = self.maximized.is_some();
                            let corner =
                                rect.center() + direction * if restoring { 2. } else { 6. };
                            let arm = if restoring { 4. } else { -4. };
                            ui.painter().add(egui::Shape::line(
                                vec![
                                    corner + egui::vec2(x * arm, 0.),
                                    corner,
                                    corner + egui::vec2(0., y * arm),
                                ],
                                style.fg_stroke,
                            ));
                        }
                    }
                    response
                        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
                    if response.on_hover_text(label).clicked() {
                        self.maximized = if self.maximized.is_some() {
                            None
                        } else {
                            Some(active)
                        };
                    }
                });
            }
        });
        pane.add_space(2.);
        // Keep widget IDs tied to the content when it changes dock position.
        pane.scope_builder(
            egui::UiBuilder::new().scope_id(content_scope.with(active.title())),
            |ui| content(active, ui),
        );
    }
}
