use crate::waveform::{Quantity, Trace, Waveform};
use eframe::egui::{self, Align2, Color32, FontId, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};

pub const BACK: Color32 = Color32::from_rgb(5, 8, 10);
const GRID: Color32 = Color32::from_rgb(55, 65, 69);
const LABEL: Color32 = Color32::from_rgb(176, 190, 196);
const CURSOR: Color32 = Color32::from_rgb(61, 217, 214);
const COLORS: [Color32; 5] = [
    Color32::from_rgb(255, 222, 53),
    Color32::from_rgb(103, 236, 85),
    Color32::from_rgb(255, 149, 63),
    Color32::from_rgb(102, 168, 255),
    Color32::from_rgb(235, 136, 223),
];

#[derive(Clone, Copy, PartialEq)]
struct Range {
    low: f64,
    span: f64,
}
impl Range {
    fn from_limits(low: f64, high: f64, padding: f64) -> Self {
        let span = high - low;
        let pad = if span == 0.0 {
            (low.abs() * 0.05).max(0.001)
        } else {
            span * padding
        };
        Self {
            low: low - pad,
            span: span + 2.0 * pad,
        }
    }
    fn fit_divisions(self, divisions: u32, center: bool) -> Self {
        let count = f64::from(divisions);
        let raw = self.span / count;
        let decade = 10_f64.powf(raw.log10().floor());
        let step = [1., 2., 2.5, 5., 10.]
            .into_iter()
            .map(|multiple| multiple * decade)
            .find(|step| *step >= raw)
            .unwrap_or(raw);
        let span = count * step;
        let slack = (span - self.span).max(0.);
        let target = if center {
            self.low - slack / 2.
        } else {
            self.low
        };
        // Align the grid to whole divisions when the samples still fit.
        let aligned = (target / step).round() * step;
        Self {
            low: aligned.clamp(self.low - slack, self.low),
            span,
        }
    }

    fn fraction(self, value: f64) -> f32 {
        ((value - self.low) / self.span) as f32
    }
    fn at(self, f: f32) -> f64 {
        self.low + self.span * f as f64
    }
    fn zoom(&mut self, anchor: f32, factor: f64) {
        if factor == 1. {
            return;
        }
        let span = self.span / factor;
        if span.is_finite() && span > self.at(anchor).abs().max(1e-100) * 1e-14 {
            self.low = self.at(anchor) - f64::from(anchor) * span;
            self.span = span;
        }
    }

    fn zoom_to_interval(&mut self, start: f32, end: f32) {
        let low = self.at(start);
        let span = self.span * f64::from(end - start);
        if span.is_finite() && span > low.abs().max(1e-100) * 1e-14 {
            *self = Self { low, span };
        }
    }
}

// Summarize blocks of raw samples so finding the peaks of a pixel column stays
// cheap even when a transient has millions of nonuniformly spaced time steps.
const BLOCK: usize = 32;

#[derive(Clone, Copy)]
struct Peaks {
    low: usize,
    high: usize,
}

impl Peaks {
    fn merge(self, other: Self, samples: &[f64]) -> Self {
        Self {
            low: if samples[other.low] < samples[self.low] {
                other.low
            } else {
                self.low
            },
            high: if samples[other.high] > samples[self.high] {
                other.high
            } else {
                self.high
            },
        }
    }
}

#[derive(Default)]
struct TraceDisplay {
    levels: Vec<Vec<Peaks>>,
    view: Option<(Range, usize)>,
    indices: Vec<usize>,
}

impl TraceDisplay {
    fn build_levels(&mut self, samples: &[f64]) {
        let mut blocks = Vec::new();
        for start in (0..samples.len()).step_by(BLOCK) {
            let peaks = (start + 1..(start + BLOCK).min(samples.len())).fold(
                Peaks {
                    low: start,
                    high: start,
                },
                |p, i| p.merge(Peaks { low: i, high: i }, samples),
            );
            blocks.push(peaks);
        }
        while blocks.len() > 1 {
            let parents = blocks
                .chunks(2)
                .map(|pair| pair[0].merge(*pair.last().unwrap(), samples))
                .collect();
            self.levels.push(blocks);
            blocks = parents;
        }
        self.levels.push(blocks);
    }

    fn peaks(&self, samples: &[f64], mut i: usize, end: usize) -> Peaks {
        let mut result = Peaks { low: i, high: i };
        while i < end {
            if i.is_multiple_of(BLOCK) && end - i >= BLOCK {
                let block = i / BLOCK;
                let level = block.trailing_zeros().min(((end - i) / BLOCK).ilog2());
                result = result.merge(self.levels[level as usize][block >> level], samples);
                i += BLOCK << level;
            } else {
                result = result.merge(Peaks { low: i, high: i }, samples);
                i += 1;
            }
        }
        result
    }

    fn update(
        &mut self,
        time: &[f64],
        samples: &[f64],
        x: Range,
        columns: usize,
        start: usize,
        end: usize,
    ) {
        if self.view == Some((x, columns)) {
            return;
        }
        self.view = Some((x, columns));
        self.indices.clear();
        if end - start <= columns * 4 {
            self.indices.extend(start..end);
            return;
        }
        if self.levels.is_empty() {
            self.build_levels(samples);
        }
        let mut first = start;
        for column in 1..=columns {
            let edge = x.low + x.span * column as f64 / columns as f64;
            let next = if column == columns {
                end
            } else {
                first + time[first..end].partition_point(|&t| t < edge)
            };
            if next > first {
                let peaks = self.peaks(samples, first, next);
                let indices = [
                    first,
                    peaks.low.min(peaks.high),
                    peaks.low.max(peaks.high),
                    next - 1,
                ];
                for index in indices {
                    if self.indices.last() != Some(&index) {
                        self.indices.push(index);
                    }
                }
            }
            first = next;
        }
    }
}

pub struct Plot {
    wave: Waveform,
    visible: Vec<bool>,
    displays: Vec<TraceDisplay>,
    grid_dash: Option<egui::TextureHandle>,
    x: Range,
    y: Range,
    current: Range,
    sample: Option<usize>,
    zoom_anchor: Option<egui::Pos2>,
}

impl Plot {
    pub fn new(mut wave: Waveform) -> Self {
        if wave.logarithmic_frequency {
            for frequency in &mut wave.time {
                *frequency = frequency.log10();
            }
        }
        let visible = vec![false; wave.traces.len()];
        let unit = Range {
            low: 0.0,
            span: 1.0,
        };
        let mut plot = Self {
            displays: (0..visible.len())
                .map(|_| TraceDisplay::default())
                .collect(),
            grid_dash: None,
            wave,
            visible,
            x: unit,
            y: unit,
            current: unit,
            sample: None,
            zoom_anchor: None,
        };
        plot.reset();
        plot
    }

    pub fn probe(&mut self, name: &str, isolate: bool) -> bool {
        let Some(index) = self.trace_index(name) else {
            return false;
        };
        if isolate {
            self.visible.fill(false);
        }
        if !self.visible[index] {
            self.visible[index] = true;
            self.fit_vertical();
        }
        true
    }

    pub fn refresh(&mut self, wave: Waveform) {
        let mut next = Self::new(wave);
        for (trace, &visible) in self.wave.traces.iter().zip(&self.visible) {
            if visible && let Some(index) = next.trace_index(&trace.name) {
                next.visible[index] = true;
            }
        }
        next.fit_vertical();
        next.grid_dash = self.grid_dash.take();
        *self = next;
    }

    fn trace_index(&mut self, name: &str) -> Option<usize> {
        if name == "V(0)" && !self.wave.traces.iter().any(|trace| trace.name == name) {
            self.wave.traces.push(Trace {
                name: name.into(),
                quantity: Quantity::Voltage,
                samples: vec![0.; self.wave.time.len()],
            });
            self.visible.push(false);
            self.displays.push(TraceDisplay::default());
        }
        self.wave
            .traces
            .iter()
            .position(|trace| trace.name.eq_ignore_ascii_case(name))
    }

    pub fn has_traces(&self) -> bool {
        self.visible.contains(&true)
    }

    pub fn reset(&mut self) {
        let time = &self.wave.time;
        self.x = Range::from_limits(time[0], time[time.len() - 1], 0.0).fit_divisions(10, false);
        self.fit_vertical();
    }

    fn fit_vertical(&mut self) {
        for (quantity, range) in [
            (Quantity::Voltage, &mut self.y),
            (Quantity::Current, &mut self.current),
        ] {
            let mut min = f64::INFINITY;
            let mut max = f64::NEG_INFINITY;
            for (trace, &visible) in self.wave.traces.iter().zip(&self.visible) {
                if visible && trace.quantity == quantity {
                    for &value in &trace.samples {
                        min = min.min(value);
                        max = max.max(value);
                    }
                }
            }
            if min <= max {
                *range = Range::from_limits(min, max, 0.08).fit_divisions(8, true);
            }
        }
    }

    fn shows(&self, quantity: Quantity) -> bool {
        self.wave
            .traces
            .iter()
            .zip(&self.visible)
            .any(|(trace, &visible)| visible && trace.quantity == quantity)
    }

    pub fn legend(&mut self, ui: &mut Ui) {
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            for (i, t) in self.wave.traces.iter().enumerate() {
                let color = COLORS[i % COLORS.len()];
                if !self.visible[i] {
                    continue;
                }
                let text = egui::RichText::new(format!("C{}   {}", i + 1, t.name)).color(BACK);
                let button = egui::Button::new(text)
                    .fill(color)
                    .stroke(Stroke::new(1., color));
                if ui
                    .add(button)
                    .on_hover_text(format!("Remove {}", t.name))
                    .clicked()
                {
                    self.visible[i] = false;
                    changed = true;
                }
            }
        });
        if changed {
            self.reset();
        }
    }

    pub fn show(&mut self, ui: &mut Ui) {
        let (response, p) = ui.allocate_painter(
            ui.available_size().max(vec2(1., 1.)),
            Sense::click_and_drag(),
        );
        let outer = response.rect;
        p.rect_filled(outer, 0, BACK);
        let voltage_visible = self.shows(Quantity::Voltage);
        let current_visible = self.shows(Quantity::Current);
        let mixed = voltage_visible && current_visible;
        let r = Rect::from_min_max(
            outer.min + vec2(64., 32.),
            outer.max - vec2(if mixed { 64. } else { 24. }, 26.),
        );
        if r.width() < 20. || r.height() < 20. {
            return;
        }
        use egui::PointerButton::{Middle, Primary, Secondary};
        if response.dragged_by(Middle) || response.dragged_by(Secondary) {
            let delta = response.drag_delta();
            self.x.low -= self.x.span * f64::from(delta.x / r.width());
            self.y.low += self.y.span * f64::from(delta.y / r.height());
            self.current.low += self.current.span * f64::from(delta.y / r.height());
        }
        if response.drag_started_by(Primary) {
            self.zoom_anchor = ui
                .input(|input| input.pointer.press_origin())
                .filter(|&pt| r.contains(pt));
        }
        let zoom_box =
            self.zoom_anchor
                .zip(response.interact_pointer_pos())
                .map(|(anchor, end)| {
                    Rect::from_two_pos(anchor.clamp(r.min, r.max), end.clamp(r.min, r.max))
                });
        if response.drag_stopped_by(Primary) {
            self.zoom_anchor = None;
            if let Some(selected) =
                zoom_box.filter(|rect| rect.width() >= 3. && rect.height() >= 3.)
            {
                self.x.zoom_to_interval(
                    (selected.left() - r.left()) / r.width(),
                    (selected.right() - r.left()) / r.width(),
                );
                let bottom = (r.bottom() - selected.bottom()) / r.height();
                let top = (r.bottom() - selected.top()) / r.height();
                self.y.zoom_to_interval(bottom, top);
                self.current.zoom_to_interval(bottom, top);
            }
        } else if !ui.input(|input| input.pointer.primary_down()) {
            self.zoom_anchor = None;
        }
        let cursor = response
            .hover_pos()
            .filter(|pt| r.contains(*pt) && self.zoom_anchor.is_none());
        if let Some(pos) = cursor {
            let (zoom, modifiers) =
                ui.input(|input| (crate::ui::canvas_zoom_factor(input), input.modifiers));
            if !modifiers.ctrl || !modifiers.shift {
                self.x.zoom((pos.x - r.left()) / r.width(), zoom);
            }
            if !modifiers.ctrl || modifiers.shift {
                let anchor = (r.bottom() - pos.y) / r.height();
                self.y.zoom(anchor, zoom);
                self.current.zoom(anchor, zoom);
            }
        }
        let mut fit = response.double_clicked();
        response.context_menu(|ui| {
            if ui.button("Zoom to Fit").clicked() {
                fit = true;
                ui.close();
            }
        });
        if fit {
            self.reset();
        }
        let font = FontId::proportional(11.);
        let ac = self.wave.analysis == crate::analysis::Analysis::Ac;
        let logarithmic = self.wave.logarithmic_frequency;
        let (ts, tu) = if logarithmic {
            (1., "dec".into())
        } else {
            scale(self.x.span / 10., if ac { "Hz" } else { "s" })
        };
        let axis_label = |x: f64| {
            if logarithmic {
                let hz = 10_f64.powf(x);
                let (factor, unit) = scale(hz, "Hz");
                format!("{} {unit}", format_tick(hz * factor, hz * factor))
            } else {
                format!("{} {tu}", format_tick(x * ts, self.x.span * ts))
            }
        };
        let (left_range, left_unit) = if current_visible && !voltage_visible {
            (self.current, "A")
        } else {
            (self.y, "V")
        };
        let (vs, vu) = scale(left_range.span / 8., left_unit);
        let (amps, amp_unit) = scale(self.current.span / 8., "A");
        let trace_range = |quantity| {
            if quantity == Quantity::Current {
                self.current
            } else {
                self.y
            }
        };
        let readout = |span: f64, div: f64, factor: f64, unit: &str| {
            format!(
                "{} {unit}/div",
                format_tick(span / div * factor, span / div * factor)
            )
        };
        p.text(
            pos2(r.left(), outer.top() + 6.),
            Align2::LEFT_TOP,
            format!(
                "{}   {}      {}   {}",
                if ac { "Frequency" } else { "Timebase" },
                readout(self.x.span, 10., ts, &tu),
                if ac { "Magnitude" } else { "Vertical" },
                readout(left_range.span, 8., vs, &vu)
            ),
            font.clone(),
            LABEL,
        );
        let grid_pen = Stroke::new(1., GRID);
        // Repeat the same dash along each graticule line in a single quad.
        let dash = self
            .grid_dash
            .get_or_insert_with(|| {
                let mut tile = egui::ColorImage::filled([5, 5], Color32::TRANSPARENT);
                for i in 0..2 {
                    tile[(i, 0)] = GRID;
                    tile[(0, i)] = GRID;
                }
                ui.ctx().load_texture(
                    "scope graticule",
                    tile,
                    egui::TextureOptions::NEAREST_REPEAT,
                )
            })
            .id();
        for index in 0..=10 {
            let f = index as f32 / 10.;
            let x = r.left() + r.width() * f;
            p.image(
                dash,
                Rect::from_min_max(pos2(x - 0.5, r.top()), pos2(x + 0.5, r.bottom())),
                Rect::from_min_max(pos2(0.1, 0.), pos2(0.1, r.height() / 5.)),
                Color32::WHITE,
            );
            if index % 2 == 0 || r.width() > 550. {
                p.text(
                    pos2(x, r.bottom() + 4.),
                    Align2::CENTER_TOP,
                    axis_label(self.x.at(f)),
                    font.clone(),
                    LABEL,
                );
            }
        }
        for index in 0..=8 {
            let f = index as f32 / 8.;
            let y = r.bottom() - r.height() * f;
            p.image(
                dash,
                Rect::from_min_max(pos2(r.left(), y - 0.5), pos2(r.right(), y + 0.5)),
                Rect::from_min_max(pos2(0., 0.1), pos2(r.width() / 5., 0.1)),
                Color32::WHITE,
            );
            if index % 2 == 0 || r.height() > 185. {
                p.text(
                    pos2(r.left() - 7., y),
                    Align2::RIGHT_CENTER,
                    format!(
                        "{} {vu}",
                        format_tick(left_range.at(f) * vs, left_range.span * vs)
                    ),
                    font.clone(),
                    LABEL,
                );
                if mixed {
                    p.text(
                        pos2(r.right() + 7., y),
                        Align2::LEFT_CENTER,
                        format!(
                            "{} {amp_unit}",
                            format_tick(self.current.at(f) * amps, self.current.span * amps)
                        ),
                        font.clone(),
                        LABEL,
                    );
                }
            }
        }
        // Five subdivisions per major division on the center crosshair.
        let tick_pen = Stroke::new(1., Color32::from_gray(110));
        for index in 0..=50 {
            let x = r.left() + r.width() * index as f32 / 50.;
            p.vline(x, r.center().y - 2.5..=r.center().y + 2.5, tick_pen);
        }
        for index in 0..=40 {
            let y = r.top() + r.height() * index as f32 / 40.;
            p.hline(r.center().x - 2.5..=r.center().x + 2.5, y, tick_pen);
        }
        p.rect_stroke(r, 0., grid_pen, egui::StrokeKind::Inside);
        let graph = p.with_clip_rect(r.intersect(p.clip_rect()));
        let start = self
            .wave
            .time
            .partition_point(|&t| t < self.x.low)
            .saturating_sub(1);
        let end =
            (self.wave.time.partition_point(|&t| t <= self.x.at(1.)) + 1).min(self.wave.time.len());
        let paths: Vec<_> = self
            .wave
            .traces
            .iter()
            .enumerate()
            .filter(|(i, _)| self.visible[*i])
            .map(|(i, trace)| {
                let range = trace_range(trace.quantity);
                let display = &mut self.displays[i];
                let columns = (r.width() * ui.ctx().pixels_per_point()).ceil().max(1.) as usize;
                display.update(&self.wave.time, &trace.samples, self.x, columns, start, end);
                let points: Vec<_> = display
                    .indices
                    .iter()
                    .map(|&j| {
                        pos2(
                            r.left() + r.width() * self.x.fraction(self.wave.time[j]),
                            r.bottom() - r.height() * range.fraction(trace.samples[j]),
                        )
                    })
                    .collect();
                (i, points)
            })
            .collect();
        let hovered = cursor.and_then(|pos| {
            paths
                .iter()
                .filter_map(|(i, points)| {
                    let distance = if let [p] = points.as_slice() {
                        pos.distance(*p)
                    } else {
                        let left = points
                            .partition_point(|p| p.x < pos.x - 6.)
                            .saturating_sub(1);
                        let right =
                            (points.partition_point(|p| p.x <= pos.x + 6.) + 1).min(points.len());
                        points[left..right]
                            .windows(2)
                            .map(|pair| {
                                let delta = pair[1] - pair[0];
                                let t = ((pos - pair[0]).dot(delta)
                                    / delta.length_sq().max(f32::EPSILON))
                                .clamp(0., 1.);
                                pos.distance(pair[0] + delta * t)
                            })
                            .fold(f32::INFINITY, f32::min)
                    };
                    (distance <= 6.).then_some((*i, distance))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        });
        if let Some(i) = hovered
            && !ui.ctx().text_edit_focused()
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Delete))
        {
            self.visible[i] = false;
            self.fit_vertical();
            ui.ctx().request_repaint();
            return;
        }
        for (i, points) in paths {
            let color = COLORS[i % COLORS.len()];
            if let [point] = points.as_slice() {
                graph.circle_filled(*point, 2., color);
            } else {
                graph.add(Shape::line(
                    points,
                    Stroke::new(if hovered == Some(i) { 3. } else { 1.8 }, color),
                ));
            }
        }
        if self.zoom_anchor.is_some()
            && let Some(selected) = zoom_box
        {
            graph.rect_filled(selected, 0., CURSOR.gamma_multiply(0.15));
            graph.rect_stroke(
                selected,
                0.,
                Stroke::new(1., CURSOR),
                egui::StrokeKind::Inside,
            );
        }
        if let Some(pos) = cursor {
            let t = self.x.at((pos.x - r.left()) / r.width());
            let after = self.wave.time.partition_point(|&sample| sample < t);
            let i = after.min(self.wave.time.len() - 1);
            let i = if i > 0 && (t - self.wave.time[i - 1]).abs() < (t - self.wave.time[i]).abs() {
                i - 1
            } else {
                i
            };
            let x = r.left() + r.width() * self.x.fraction(self.wave.time[i]);
            graph.vline(x, r.y_range(), Stroke::new(1., CURSOR));
            graph.add(Shape::convex_polygon(
                vec![
                    pos2(x - 5., r.top()),
                    pos2(x + 5., r.top()),
                    pos2(x, r.top() + 7.),
                ],
                CURSOR,
                Stroke::NONE,
            ));
            for (channel, trace) in self.wave.traces.iter().enumerate() {
                if self.visible[channel] {
                    let y = r.bottom()
                        - r.height() * trace_range(trace.quantity).fraction(trace.samples[i]);
                    graph.circle_stroke(
                        pos2(x, y),
                        3.5,
                        Stroke::new(1.5, COLORS[channel % COLORS.len()]),
                    );
                }
            }
            self.sample = Some(i);
        }
        if let Some(i) = self.sample {
            let mut label = format!("x  {}", axis_label(self.wave.time[i]));
            for (trace, &visible) in self.wave.traces.iter().zip(&self.visible) {
                if visible {
                    let value = trace.samples[i];
                    let (factor, unit) =
                        scale(trace_range(trace.quantity).span / 8., trace.quantity.unit());
                    label.push_str(&format!(
                        "\ny  {}  {} {unit}",
                        trace.name,
                        format_tick(
                            value * factor,
                            trace_range(trace.quantity).span * factor / 80.
                        )
                    ));
                }
            }
            let text = graph.layout_no_wrap(label, font, LABEL);
            let pos = r.right_bottom() - text.size() - vec2(8., 8.);
            graph.rect_filled(
                Rect::from_min_size(pos, text.size()).expand(4.),
                0,
                Color32::from_gray(25),
            );
            graph.galley(pos, text, LABEL);
        }
    }
}

fn format_tick(value: f64, span: f64) -> String {
    if span < 0.0001 || value.abs() >= 1e6 {
        return format!("{value:.3e}");
    }
    let precision = (2.0 - span.log10().floor()).clamp(0., 8.) as usize;
    let s = format!("{value:.precision$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        s
    }
}

fn scale(span: f64, unit: &str) -> (f64, String) {
    let (factor, prefix) = if span < 1e-6 {
        (1e9, "n")
    } else if span < 1e-3 {
        (1e6, "µ")
    } else if span < 1. {
        (1e3, "m")
    } else if span >= 1e6 {
        (1e-6, "M")
    } else if span >= 1e3 {
        (1e-3, "k")
    } else {
        (1., "")
    };
    (factor, format!("{prefix}{unit}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dense_display_preserves_column_peaks_and_restores_raw_points_on_zoom() {
        let time: Vec<_> = (0..15003).map(|i| (i as f64).powi(2)).collect();
        let samples: Vec<_> = (0..time.len())
            .map(|i| match i % 337 {
                1 => 123.,
                2 => -456.,
                _ => (i as f64).sin(),
            })
            .collect();
        let mut display = TraceDisplay::default();
        let x = Range {
            low: time[17],
            span: time[14501] - time[17],
        };
        let columns = 83;
        display.update(&time, &samples, x, columns, 16, 14503);
        assert!(display.indices.len() <= 4 * columns);
        assert!(display.indices.windows(2).all(|w| w[0] < w[1]));
        let mut start = 16;
        for col in 1..=columns {
            let edge = x.low + x.span * col as f64 / columns as f64;
            let end = if col == columns {
                14503
            } else {
                time.partition_point(|&t| t < edge)
            };
            let values = &samples[start..end];
            let shown: Vec<_> = display
                .indices
                .iter()
                .copied()
                .filter(|i| (start..end).contains(i))
                .map(|i| samples[i])
                .collect();
            for op in [f64::min, f64::max] {
                assert_eq!(
                    values.iter().copied().reduce(op),
                    shown.iter().copied().reduce(op)
                );
            }
            start = end;
        }
        display.update(
            &time,
            &samples,
            Range {
                low: time[299],
                span: time[310] - time[299],
            },
            columns,
            298,
            312,
        );
        assert_eq!(display.indices, (298..312).collect::<Vec<_>>());
    }
}
