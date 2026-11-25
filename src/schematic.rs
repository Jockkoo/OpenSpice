use eframe::egui::{self, Align2, Color32, FontId, Pos2, Stroke, Ui, vec2};
use std::collections::{HashMap, HashSet};

const SCHEMATIC_STROKE: f32 = 1.6;
const PAPER: Color32 = Color32::WHITE;
const WIRE: Color32 = Color32::from_rgb(0, 128, 35);
const INK: Color32 = Color32::from_rgb(135, 15, 24);
const LABEL: Color32 = Color32::from_rgb(0, 108, 116);
const ACCENT: Color32 = Color32::from_rgb(35, 111, 211);
pub(crate) const GRID: f32 = 14.;
const PIN_OFFSET: f32 = 2. * GRID;

#[derive(Clone, Copy, Debug, PartialEq, Hash)]
pub(crate) enum Part {
    Resistor,
    Capacitor,
    Inductor,
    Voltage,
    Current,
    Diode,
    Npn,
    Pnp,
    Nmos,
    Pmos,
    Njfet,
    Pjfet,
    Polarized,
    Op,
    OpRail,
    Pole,
    Transformer,
}

const DIODES: [(&str, &str); 5] = [
    ("os_diode", "Small-signal diode"),
    ("os_rectifier", "Rectifier"),
    ("os_schottky", "Schottky"),
    ("os_zener", "Zener (5.1 V)"),
    ("os_led", "LED (red)"),
];

fn diode_type(value: &str) -> (&'static str, &'static str) {
    DIODES
        .into_iter()
        .find(|(model, _)| {
            value
                .split_whitespace()
                .next()
                .is_some_and(|v| v.eq_ignore_ascii_case(model))
        })
        .unwrap_or(DIODES[0])
}

impl Part {
    const ALL: [Self; 5] = [
        Self::Resistor,
        Self::Capacitor,
        Self::Inductor,
        Self::Voltage,
        Self::Current,
    ];

    const LIBRARY: [Self; 12] = [
        Self::Diode,
        Self::Npn,
        Self::Pnp,
        Self::Nmos,
        Self::Pmos,
        Self::Njfet,
        Self::Pjfet,
        Self::Polarized,
        Self::Op,
        Self::OpRail,
        Self::Pole,
        Self::Transformer,
    ];
    fn layout(self) -> &'static [(i32, i32, &'static str)] {
        match self {
            Self::Npn | Self::Pnp => &[(1, -2, "C"), (-2, 0, "B"), (1, 2, "E")],
            Self::Nmos | Self::Pmos | Self::Njfet | Self::Pjfet => {
                &[(1, -2, "D"), (-2, 0, "G"), (1, 2, "S")]
            }
            Self::Op => &[(-2, -1, "+"), (-2, 1, "−"), (2, 0, "")],
            Self::OpRail | Self::Pole => &[
                (-2, -1, "+"),
                (-2, 1, "−"),
                (2, 0, ""),
                (0, -2, "+"),
                (0, 2, "−"),
            ],
            Self::Transformer => &[(-2, -1, ""), (-2, 1, ""), (2, -1, ""), (2, 1, "")],
            _ => &[(-2, 0, ""), (2, 0, "")],
        }
    }
    fn terminals(self, center: Point, turns: u8) -> Vec<Point> {
        self.layout()
            .iter()
            .map(|&(x, y, _)| {
                let (x, y) = match turns % 4 {
                    0 => (x, y),
                    1 => (-y, x),
                    2 => (-x, -y),
                    _ => (y, -x),
                };
                (center.0 + x, center.1 + y)
            })
            .collect()
    }
    fn parameters(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Transformer => &[("ratio", "Turns ratio (Ns/Np)")],
            Self::Op => &[
                ("vpos", "Positive supply (V)"),
                ("vneg", "Negative supply (V)"),
            ],
            Self::Pole => &[
                ("gbw", "Unity-gain bandwidth (Hz)"),
                ("gain", "DC gain (V/V)"),
                ("slew", "Slew rate (V/s)"),
            ],
            Self::Polarized => &[("cap", "Capacitance (F)")],
            _ => &[],
        }
    }
    fn toolbar_icon(self, p: &egui::Painter, center: Pos2, color: Color32, american: bool) {
        let scale = if self == Self::Voltage { 0.5 } else { 0.39 };
        draw(p, center, (self, self.spec().2), 1, scale, american, color);
    }

    fn spec(self) -> (char, &'static str, &'static str) {
        match self {
            Self::Resistor => ('R', "Resistor", "1k"),
            Self::Capacitor => ('C', "Capacitor", "1u"),
            Self::Inductor => ('L', "Inductor", "1m"),
            Self::Voltage => ('V', "Voltage source", "5"),
            Self::Current => ('I', "Current source", "1m"),
            Self::Diode => ('X', "Diode", "os_diode"),
            Self::Npn => ('X', "NPN transistor", "os_npn"),
            Self::Pnp => ('X', "PNP transistor", "os_pnp"),
            Self::Nmos => ('X', "NMOS", "os_nmos"),
            Self::Pmos => ('X', "PMOS", "os_pmos"),
            Self::Njfet => ('X', "N-channel JFET", "os_njfet"),
            Self::Pjfet => ('X', "P-channel JFET", "os_pjfet"),
            Self::Polarized => ('X', "Polarized capacitor", "os_cap cap=10u"),
            Self::Op => (
                'X',
                "Ideal op amp (internal rails)",
                "os_op_adjust vpos=15 vneg=-15",
            ),
            Self::OpRail => ('X', "Ideal op amp (external rails)", "os_oprail"),
            Self::Pole => (
                'X',
                "Single-pole op amp",
                "os_pole_ext gbw=1meg gain=100k slew=1meg",
            ),
            Self::Transformer => ('X', "Transformer", "os_xfmr ratio=1"),
        }
    }
}

fn icon_path(p: &egui::Painter, center: Pos2, pen: Stroke, points: &[(f32, f32)]) {
    p.add(egui::Shape::line(
        points.iter().map(|&(x, y)| center + vec2(x, y)).collect(),
        pen,
    ));
}

struct Card<'a> {
    part: Part,
    name: &'a str,
    nodes: Vec<&'a str>,
    line: usize,
    value: &'a str,
}

fn cards(text: &str) -> Vec<Card<'_>> {
    let mut result = Vec::new();
    let mut nested = 0usize;
    let mut control = false;
    for (index, line) in text.lines().enumerate().skip(1) {
        let mut remainder = line.trim_start();
        let mut next_word = || {
            let end = remainder
                .find(char::is_whitespace)
                .unwrap_or(remainder.len());
            let (word, rest) = remainder.split_at(end);
            remainder = rest.trim_start();
            (!word.is_empty()).then_some(word)
        };
        let Some(name) = next_word() else { continue };
        if name.starts_with(RAIL_SOURCE) {
            continue;
        }
        match name.to_ascii_lowercase().as_str() {
            ".control" => control = true,
            ".endc" => control = false,
            _ if control => (),
            ".subckt" => nested += 1,
            ".ends" => nested = nested.saturating_sub(1),
            ".end" => break,
            _ if nested == 0 => {
                let prefix = name.chars().next().unwrap().to_ascii_uppercase();
                let legacy_pole = prefix == 'X'
                    && remainder
                        .split_whitespace()
                        .nth(3)
                        .is_some_and(|v| v.eq_ignore_ascii_case("os_pole"));
                let part = if legacy_pole {
                    Some(Part::Pole)
                } else if prefix == 'X' {
                    Part::LIBRARY.into_iter().find(|p| {
                        remainder
                            .split_whitespace()
                            .nth(p.layout().len())
                            .is_some_and(|model| {
                                (*p == Part::Diode
                                    && DIODES
                                        .iter()
                                        .any(|(key, _)| model.eq_ignore_ascii_case(key)))
                                    || (*p == Part::Op && model.eq_ignore_ascii_case("os_op"))
                                    || model.eq_ignore_ascii_case(
                                        p.spec().2.split_whitespace().next().unwrap(),
                                    )
                            })
                    })
                } else {
                    Part::ALL.into_iter().find(|p| prefix == p.spec().0)
                };
                if let Some(part) = part {
                    let mut nodes = Vec::new();
                    let count = if legacy_pole { 3 } else { part.layout().len() };
                    for _ in 0..count {
                        let Some((node, rest)) = remainder.split_once(char::is_whitespace) else {
                            break;
                        };
                        nodes.push(node);
                        remainder = rest.trim_start();
                    }
                    if nodes.len() != count || remainder.is_empty() {
                        continue;
                    }
                    result.push(Card {
                        line: index,
                        part,
                        name,
                        nodes,
                        value: remainder.trim_end(),
                    });
                }
            }
            _ => (),
        }
    }
    result
}

// Embed the model used by this instance, including when editing older self-contained decks.
fn ensure_model(text: &mut String, definition: &str) {
    let name = definition.split_whitespace().next().unwrap_or("");
    let declares = |line: &str| {
        let mut fields = line.split_whitespace();
        fields
            .next()
            .is_some_and(|s| s.eq_ignore_ascii_case(".subckt"))
            && fields.next().is_some_and(|s| s.eq_ignore_ascii_case(name))
    };
    if text.lines().any(declares) {
        return;
    }
    let mut lines = include_str!("../assets/components.lib")
        .lines()
        .skip_while(|l| !declares(l));
    let mut model: Vec<_> = lines.by_ref().take_while(|l| *l != ".ends").collect();
    if model.is_empty() {
        return;
    }
    model.push(".ends");
    let mut deck: Vec<_> = text.lines().collect();
    let end = deck
        .iter()
        .position(|l| l.trim().eq_ignore_ascii_case(".end"))
        .unwrap_or(deck.len());
    let model = model.join("\n");
    deck.insert(end, &model);
    *text = deck.join("\n") + "\n";
}

pub fn upgrade_pole_pins(text: &mut String) {
    let mut replacements = Vec::new();
    let mut used: HashSet<_> = text
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect();
    let mut serial = 0;
    for c in cards(text)
        .into_iter()
        .filter(|c| c.part == Part::Pole && c.nodes.len() == 3)
    {
        let mut rail = || loop {
            serial += 1;
            let node = format!("os_rail_{serial}");
            if used.insert(node.clone()) {
                break node;
            }
        };
        let params = c
            .value
            .split_whitespace()
            .skip(1)
            .collect::<Vec<_>>()
            .join(" ");
        replacements.push((
            c.line,
            format!(
                "{} {} {} {} os_pole_ext biasp=15 biasn=-15 {params}",
                c.name,
                c.nodes.join(" "),
                rail(),
                rail()
            ),
        ));
    }
    if replacements.is_empty() {
        return;
    }
    let mut lines: Vec<_> = text.lines().collect();
    for (line, value) in &replacements {
        lines[*line] = value;
    }
    *text = lines.join("\n") + "\n";
    ensure_model(text, "os_pole_ext");
}

const PARASITICS_MARKER: &str = " $ openspice-parasitics ";

fn parasitic_fields(part: Part) -> &'static [(&'static str, &'static str)] {
    match part {
        Part::Resistor => &[
            ("ls", "Series inductance (H)"),
            ("cp", "Parallel capacitance (F)"),
        ],
        Part::Capacitor | Part::Polarized => &[
            ("rs", "Series resistance (Ω)"),
            ("ls", "Series inductance (H)"),
            ("rp", "Leakage resistance (Ω)"),
        ],
        Part::Inductor => &[
            ("rs", "Series resistance (Ω)"),
            ("cp", "Parallel capacitance (F)"),
        ],
        _ => &[],
    }
}

fn split_parasitics(part: Part, value: &str) -> (&str, &str) {
    if parasitic_fields(part).is_empty() {
        (value, "")
    } else {
        value.split_once(PARASITICS_MARKER).unwrap_or((value, ""))
    }
}

fn parasitic_value<'a>(settings: &'a str, key: &str) -> &'a str {
    settings
        .split_whitespace()
        .filter_map(|entry| entry.split_once('='))
        .find_map(|(name, value)| (name == key).then_some(value))
        .unwrap_or_default()
}

// Insert zero-volt series sources so branch currents are available in every analysis.
pub fn instrument_currents(text: &str) -> (String, Vec<(String, String)>) {
    let lower = text.to_ascii_lowercase();
    let mut lines: Vec<_> = text.lines().map(str::to_owned).collect();
    let mut sensors = Vec::new();
    let mut serial = 0;
    for card in cards(text)
        .into_iter()
        .filter(|card| card.part != Part::Voltage)
    {
        let (meter, node) = loop {
            serial += 1;
            let meter = format!("v__openspice_probe_{serial}");
            let node = format!("__openspice_probe_{serial}");
            if !lower.contains(&node) {
                break (meter, node);
            }
        };
        let terminal = match card.part {
            Part::Op | Part::OpRail | Part::Pole => 2,
            _ => 0,
        };
        let mut nodes: Vec<String> = card.nodes.iter().map(|n| (*n).to_owned()).collect();
        let external = std::mem::replace(&mut nodes[terminal], node.clone());
        let (value, parasitics) = split_parasitics(card.part, card.value);
        let mut extra = String::new();
        for &(key, _) in parasitic_fields(card.part) {
            let setting = parasitic_value(parasitics, key);
            if !crate::analysis::number(setting).is_some_and(|n| n > 0.) {
                continue;
            }
            let (prefix, a, b) = match key {
                "rs" | "ls" => {
                    let internal = format!("{node}_{key}");
                    let a = std::mem::replace(&mut nodes[0], internal.clone());
                    let prefix = if key == "rs" { 'r' } else { 'l' };
                    extra.push_str(&format!("\n{prefix}{internal} {a} {internal} {setting}"));
                    continue;
                }
                "rp" => ('r', node.as_str(), nodes[1].as_str()),
                "cp" => ('c', node.as_str(), nodes[1].as_str()),
                _ => unreachable!(),
            };
            extra.push_str(&format!("\n{prefix}{node}_{key} {a} {b} {setting}"));
        }
        lines[card.line] = format!(
            "{meter} {external} {node} 0\n{} {} {value}{extra}",
            card.name,
            nodes.join(" ")
        );
        sensors.push((node, card.name.to_ascii_lowercase()));
    }
    if !lines.is_empty() {
        lines.insert(1, ".save all".into());
    }
    (lines.join("\n") + "\n", sensors)
}

type Point = (i32, i32);
type Wire = (Point, Point);

fn pins((x, y): Point, turns: u8) -> [Point; 2] {
    let (dx, dy) = [(2, 0), (0, 2), (-2, 0), (0, -2)][usize::from(turns % 4)];
    [(x - dx, y - dy), (x + dx, y + dy)]
}

fn wire_direction(from: Point, to: Point) -> Point {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let (mut divisor, mut remainder) = (dx.abs(), dy.abs());
    while remainder != 0 {
        (divisor, remainder) = (remainder, divisor % remainder);
    }
    (dx / divisor, dy / divisor)
}

fn on_wire((x, y): Point, ((a, b), (c, d)): Wire) -> bool {
    i64::from(x - a) * i64::from(d - b) == i64::from(y - b) * i64::from(c - a)
        && (a.min(c)..=a.max(c)).contains(&x)
        && (b.min(d)..=b.max(d)).contains(&y)
}

#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) enum Tool {
    #[default]
    Cursor,
    Select,
    Move,
    Place(Part),
    Wire,
    Erase,
    Copy,
    Power,
    Ground,
}

struct Properties {
    part: Part,
    parameters: Vec<String>,
    parasitics: Vec<String>,
    source: Option<crate::source::Source>,
    original: String,
    reference: String,
    value: String,
}

const RAIL_SOURCE: &str = "v__openspice_rail_";

fn rail_net(voltage: Option<f64>) -> String {
    match voltage {
        Some(v) => format!(
            "os_vcc_{}",
            v.to_string().replace('-', "m").replace('.', "p")
        ),
        None => "vcc".into(),
    }
}

#[derive(Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Geometry {
    rotations: HashMap<String, u8>,
    positions: HashMap<String, Point>,
    wires: Vec<Wire>,
    grounds: Vec<Point>,
    powers: Vec<Point>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    power_voltages: HashMap<Point, f64>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    power_turns: HashMap<Point, u8>,
}

struct OperatingPoint {
    values: HashMap<String, f64>,
    display: crate::analysis::OperatingPointDisplay,
    probes: std::collections::BTreeMap<String, Point>,
}

#[derive(Default)]
pub struct Schematic {
    pub american_resistor: bool,
    kinds: HashMap<String, Part>,
    editing: Option<Properties>,
    rotations: HashMap<String, u8>,
    preview_turns: u8,
    zoom_steps: f32,
    grid_tiles: Vec<egui::TextureHandle>,
    pan: egui::Vec2,
    viewport: egui::Vec2,
    positions: HashMap<String, Point>,
    wires: Vec<Wire>,
    grounds: Vec<Point>,
    powers: Vec<Point>,
    power_voltages: HashMap<Point, f64>,
    power_turns: HashMap<Point, u8>,
    preview_voltage: Option<f64>,
    editing_power: Option<(Point, String)>,
    drag_leads: Vec<(Point, Point)>,
    pub(crate) tool: Tool,
    pub fit_pending: bool,
    start: Option<Point>,
    dragging_part: bool,
    selected: HashSet<Object>,
    drag_origin: Option<Geometry>,
    gesture: Option<Gesture>,
    copy_anchor: Option<Point>,
    operating_point: Option<OperatingPoint>,
    snapshot: String,
    label_layout: Option<Vec<egui::Rect>>,
}

#[derive(Clone, Copy)]
enum Gesture {
    Move(Point),
    Box(Pos2),
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum Object {
    Part(String),
    Power(usize),
    Ground(usize),
    Wire(usize),
}

impl Schematic {
    pub fn set_operating_point(
        &mut self,
        wave: crate::waveform::Waveform,
        display: crate::analysis::OperatingPointDisplay,
    ) {
        let mut values: HashMap<_, _> = wave
            .traces
            .into_iter()
            .filter_map(|trace| Some((trace.name.to_ascii_lowercase(), *trace.samples.first()?)))
            .collect();
        values.insert("v(0)".into(), 0.);
        self.operating_point = Some(OperatingPoint {
            values,
            display,
            probes: Default::default(),
        });
    }

    pub fn clear_operating_point(&mut self) {
        self.operating_point = None;
    }

    pub fn has_operating_point(&self) -> bool {
        self.operating_point.is_some()
    }

    pub fn geometry(&self) -> Geometry {
        Geometry {
            rotations: self.rotations.clone(),
            positions: self.positions.clone(),
            wires: self.wires.clone(),
            grounds: self.grounds.clone(),
            powers: self.powers.clone(),
            power_voltages: self.power_voltages.clone(),
            power_turns: self.power_turns.clone(),
        }
    }
    fn set_geometry(&mut self, g: Geometry) {
        self.label_layout = None;
        self.rotations = g.rotations;
        self.positions = g.positions;
        self.wires = g.wires;
        self.grounds = g.grounds;
        self.powers = g.powers;
        self.power_voltages = g.power_voltages;
        self.power_turns = g.power_turns;
    }
    pub fn matches_document(g: &Geometry, text: &str) -> bool {
        let parts = cards(text);
        g.power_voltages
            .iter()
            .all(|(p, v)| g.powers.contains(p) && v.is_finite())
            && parts.len() == g.positions.len()
            && parts.iter().all(|c| g.positions.contains_key(c.name))
            && g.positions
                .values()
                .chain(&g.grounds)
                .chain(&g.powers)
                .chain(g.wires.iter().flat_map(|(a, b)| [a, b]))
                .all(|&(x, y)| x.unsigned_abs() <= 1_000_000 && y.unsigned_abs() <= 1_000_000)
    }
    pub fn restore(&mut self, g: Geometry, text: &str) {
        self.cancel();
        self.drag_leads.clear();
        self.editing = None;
        self.editing_power = None;
        self.selected.clear();
        self.set_geometry(g);
        self.index_parts(text);
        self.label_layout = None;
        self.snapshot = text.into();
    }
    fn index_parts(&mut self, text: &str) {
        self.kinds = cards(text)
            .into_iter()
            .map(|c| (c.name.into(), c.part))
            .collect();
    }
    fn turns(&self, name: &str) -> u8 {
        self.rotations.get(name).copied().unwrap_or(0)
    }
    pub(crate) fn zoom_percent(&self) -> f32 {
        100. * 1.25_f32.powf(self.zoom_steps)
    }

    pub(crate) fn set_zoom_percent(&mut self, percent: f32) {
        self.zoom((percent / 100.).log(1.25) - self.zoom_steps, None);
    }

    pub fn zoom(&mut self, steps: f32, anchor: Option<egui::Vec2>) {
        let anchor = anchor.unwrap_or(self.viewport * 0.5);
        let next = (self.zoom_steps + steps).clamp(self.zoom_steps.min(-6.), 7.);
        self.pan = anchor + (self.pan - anchor) * 1.25_f32.powf(next - self.zoom_steps);
        self.zoom_steps = next;
    }
    fn draw_grid(&mut self, painter: &egui::Painter, rect: egui::Rect, origin: Pos2, pitch: f32) {
        // Reuse transparent dot textures throughout a wheel or pinch gesture.
        // Reference pitches span the displayed range up to the maximum schematic zoom.
        if self.grid_tiles.is_empty() {
            for spacing in (4..=68).step_by(4) {
                let side = spacing * 2;
                let mut tile = egui::ColorImage::filled([side, side], Color32::TRANSPARENT);
                let mid = side / 2;
                for y in mid - 2..mid + 2 {
                    for x in mid - 2..mid + 2 {
                        let d = vec2(x as f32 + 0.5 - mid as f32, y as f32 + 0.5 - mid as f32);
                        let alpha = ((1.9 - d.length()).clamp(0., 1.) * 255.).round() as u8;
                        tile[(x, y)] = Color32::from_rgba_unmultiplied(182, 182, 182, alpha);
                    }
                }
                self.grid_tiles.push(painter.ctx().load_texture(
                    "schematic dots",
                    tile,
                    egui::TextureOptions::LINEAR_REPEAT,
                ));
            }
        }

        // Adjacent levels are nested on the same world grid. Fade the intervening
        // dots as the pitch shrinks so panning and zooming keep a stable reference.
        let stride = 2_f32.powf((5. / pitch).log2().ceil().max(0.));
        let spacing = pitch * stride;
        let draw = |spacing: f32, opacity: f32| {
            let index = ((spacing / 4.).round() as usize).clamp(1, self.grid_tiles.len()) - 1;
            let uv = |pos: Pos2| ((pos - origin) / spacing + egui::Vec2::splat(0.5)).to_pos2();
            painter.image(
                self.grid_tiles[index].id(),
                rect,
                egui::Rect::from_min_max(uv(rect.min), uv(rect.max)),
                Color32::from_white_alpha((opacity * 255.).round() as u8),
            );
        };
        if spacing < 10. {
            draw(spacing * 2., 1.);
        }
        draw(spacing, ((spacing - 5.) / 5.).clamp(0., 1.));
    }

    fn rotate(&mut self) -> bool {
        self.label_layout = None;
        if matches!(self.tool, Tool::Place(_) | Tool::Power) {
            self.preview_turns = (self.preview_turns + 1) % 4;
            return false;
        }
        if self.tool == Tool::Move {
            if self.drag_origin.is_some() {
                for object in &self.selected {
                    if let Object::Part(name) = object {
                        let turns = self.rotations.entry(name.clone()).or_default();
                        *turns = (*turns + 1) % 4;
                    } else if let Object::Power(index) = *object {
                        rotate_power(&mut self.power_turns, self.powers[index]);
                    }
                }
            }
            // Commit the rotation together with the placement.
            return false;
        }
        if !matches!(self.tool, Tool::Cursor | Tool::Select) {
            return false;
        }
        let mut moves = HashMap::new();
        let mut power_rotated = false;
        for obj in &self.selected {
            if let Object::Part(n) = obj {
                let center = self.positions[n];
                let turns = self.rotations.entry(n.clone()).or_default();
                let old = self.kinds[n].terminals(center, *turns);
                *turns = (*turns + 1) % 4;
                moves.extend(old.into_iter().zip(self.kinds[n].terminals(center, *turns)));
            } else if let Object::Power(index) = *obj {
                rotate_power(&mut self.power_turns, self.powers[index]);
                power_rotated = true;
            }
        }
        if moves.is_empty() {
            return power_rotated;
        }
        let fixed: HashSet<_> = self
            .positions
            .iter()
            .filter(|(n, _)| !self.selected.contains(&Object::Part((*n).clone())))
            .flat_map(|(n, &pos)| self.kinds[n].terminals(pos, self.turns(n)))
            .collect();
        for (a, b) in &mut self.wires {
            *a = *moves.get(a).unwrap_or(a);
            *b = *moves.get(b).unwrap_or(b);
        }
        for g in &mut self.grounds {
            *g = *moves.get(g).unwrap_or(g);
        }
        self.wires
            .extend(moves.into_iter().filter(|(old, _)| fixed.contains(old)));
        true
    }
    pub fn picker(&mut self, ui: &mut Ui) -> bool {
        let mut chosen = false;
        for (label, tool) in [("Wire", Tool::Wire), ("Ground", Tool::Ground)] {
            let (rect, response) =
                ui.allocate_exact_size(crate::ui::TOOL_SIZE, egui::Sense::click());
            let color = crate::ui::toolbar_surface(ui, &response, self.tool == tool);
            let center = rect.center();
            let pen = Stroke::new(1.5, color);
            let path = |points: &[(f32, f32)]| {
                ui.painter().add(egui::Shape::line(
                    points.iter().map(|&(x, y)| center + vec2(x, y)).collect(),
                    pen,
                ));
            };
            match tool {
                Tool::Wire => {
                    path(&[(-8., 6.), (-2., 6.), (-2., -5.), (8., -5.)]);
                    for (x, y) in [(-8., 6.), (8., -5.)] {
                        ui.painter()
                            .circle_filled(center + vec2(x, y), 2., pen.color);
                    }
                }
                Tool::Ground => {
                    path(&[(0., -8.), (0., 1.)]);
                    for i in 0..3 {
                        let y = 1. + 3. * i as f32;
                        let w = 8. - 3. * i as f32;
                        path(&[(-w, y), (w, y)]);
                    }
                }
                _ => unreachable!(),
            }
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
            if response.on_hover_text(label).clicked() {
                self.cancel();
                self.tool = tool;
                chosen = true;
            }
        }
        let (_, rect) = ui.allocate_space(crate::ui::TOOL_SIZE + vec2(8., 0.));
        let response = ui
            .interact(rect, egui::Id::unique("VCC power"), egui::Sense::click())
            .on_hover_text("Power rails");
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Power rails"));
        let popup = egui::Popup::menu(&response);
        let color =
            crate::ui::toolbar_surface(ui, &response, popup.is_open() || self.tool == Tool::Power);
        power_arrow(ui.painter(), rect.center() + vec2(-4., 8.), 0.9, 0, color);
        icon_path(
            ui.painter(),
            rect.right_center() - vec2(7., 0.),
            Stroke::new(1.2, color),
            &[(-3., -1.), (0., 2.), (3., -1.)],
        );
        popup.show(|ui| {
            for (label, voltage) in [
                ("3.3 V", Some(3.3)),
                ("5 V", Some(5.)),
                ("12 V", Some(12.)),
                ("Vcc", None),
            ] {
                if ui.button(label).clicked() {
                    self.cancel();
                    self.preview_voltage = voltage;
                    self.preview_turns = 0;
                    self.tool = Tool::Power;
                    chosen = true;
                    ui.close();
                }
            }
        });
        for part in [
            Part::Voltage,
            Part::Current,
            Part::Resistor,
            Part::Capacitor,
            Part::Inductor,
            Part::Diode,
        ] {
            let (_, label, _) = part.spec();
            let (rect, response) =
                ui.allocate_exact_size(crate::ui::TOOL_SIZE, egui::Sense::click_and_drag());
            let color = crate::ui::toolbar_surface(ui, &response, self.tool == Tool::Place(part));
            part.toolbar_icon(ui.painter(), rect.center(), color, self.american_resistor);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
            let response = response.on_hover_text(format!("{label} · click or drag to place"));
            if response.clicked() || response.drag_started() {
                self.begin_placement(part, response.drag_started());
                chosen = true;
            }
            if part == Part::Current {
                ui.separator();
            }
        }
        chosen |= self.transistor_picker(ui, "BJT", [Part::Npn, Part::Pnp]);
        chosen |= self.transistor_picker(ui, "MOSFET", [Part::Nmos, Part::Pmos]);
        let (_, rect) = ui.allocate_space(crate::ui::TOOL_SIZE + vec2(8., 0.));
        let response = ui.interact(
            rect,
            egui::Id::unique("Choose component"),
            egui::Sense::click(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, true, "Choose component")
        });
        let response = response.on_hover_text("Choose a component");
        let popup = egui::Popup::menu(&response);
        let color = crate::ui::toolbar_surface(ui, &response, popup.is_open());
        let center = rect.center() - vec2(4., 0.);
        let pen = Stroke::new(1.5, color);
        ui.painter().rect_stroke(
            egui::Rect::from_center_size(center, vec2(13., 18.)),
            2,
            pen,
            egui::StrokeKind::Middle,
        );
        ui.painter()
            .circle_filled(center + vec2(-2.5, -5.), 1.25, color);
        for side in [-1., 1.] {
            for y in [-5.5, 0., 5.5] {
                icon_path(
                    ui.painter(),
                    center,
                    pen,
                    &[
                        (side * 6.5, y - 1.3),
                        (side * 9.5, y - 1.3),
                        (side * 9.5, y + 1.3),
                        (side * 6.5, y + 1.3),
                    ],
                );
            }
        }
        icon_path(
            ui.painter(),
            rect.right_center() - vec2(7., 0.),
            Stroke::new(1.2, color),
            &[(-3., -1.), (0., 2.), (3., -1.)],
        );
        popup.show(|ui| {
            ui.set_min_width(180.);
            egui::ScrollArea::vertical()
                .max_height(400.)
                .show(ui, |ui| {
                    for part in Part::ALL.into_iter().chain(Part::LIBRARY) {
                        let (_, label, _) = part.spec();
                        let r = ui.add_sized(
                            [ui.available_width(), 24.],
                            egui::Button::new(label)
                                .frame_when_inactive(false)
                                .sense(egui::Sense::click_and_drag()),
                        );
                        if r.clicked() || r.drag_started() {
                            self.begin_placement(part, r.drag_started());
                            chosen = true;
                            ui.close();
                        }
                    }
                });
        });
        chosen
    }
    fn begin_placement(&mut self, part: Part, dragging: bool) {
        self.cancel();
        self.preview_turns = 0;
        self.tool = Tool::Place(part);
        self.dragging_part = dragging;
    }

    fn transistor_picker(&mut self, ui: &mut Ui, label: &str, variants: [Part; 2]) -> bool {
        let (_, rect) = ui.allocate_space(crate::ui::TOOL_SIZE + vec2(8., 0.));
        let response = ui.interact(rect, egui::Id::unique(label), egui::Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
        let response = response.on_hover_text(label);
        let popup = egui::Popup::menu(&response);
        let color = crate::ui::toolbar_surface(
            ui,
            &response,
            popup.is_open() || variants.iter().any(|&part| self.tool == Tool::Place(part)),
        );
        let icon_center = rect.center() - vec2(4., 0.);
        draw(
            ui.painter(),
            icon_center,
            (variants[0], ""),
            0,
            0.36,
            self.american_resistor,
            color,
        );
        icon_path(
            ui.painter(),
            rect.right_center() - vec2(7., 0.),
            Stroke::new(1.2, color),
            &[(-3., -1.), (0., 2.), (3., -1.)],
        );
        let mut chosen = false;
        popup.show(|ui| {
            for part in variants {
                let (_, name, _) = part.spec();
                let (rect, response) =
                    ui.allocate_exact_size(vec2(104., 30.), egui::Sense::click());
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
                let color =
                    crate::ui::toolbar_surface(ui, &response, self.tool == Tool::Place(part));
                draw(
                    ui.painter(),
                    rect.left_center() + vec2(17., 0.),
                    (part, ""),
                    0,
                    0.42,
                    self.american_resistor,
                    color,
                );
                ui.painter().text(
                    rect.left_center() + vec2(35., 0.),
                    Align2::LEFT_CENTER,
                    match part {
                        Part::Npn => "NPN",
                        Part::Pnp => "PNP",
                        Part::Nmos => "NMOS",
                        Part::Pmos => "PMOS",
                        _ => name,
                    },
                    FontId::proportional(12.),
                    color,
                );
                if response.clicked() {
                    self.begin_placement(part, false);
                    chosen = true;
                    ui.close();
                }
            }
        });
        chosen
    }

    fn accepts_tool_shortcuts(&self, ui: &Ui) -> bool {
        self.editing.is_none()
            && self.editing_power.is_none()
            && !ui.ctx().text_edit_focused()
            && !egui::Popup::is_any_open(ui.ctx())
    }

    pub fn tool_shortcut(&mut self, ui: &Ui) -> Option<Tool> {
        use egui::{Key, Modifiers};

        if !self.accepts_tool_shortcuts(ui) {
            return None;
        }
        let mode = ui.input_mut(|input| {
            if consume_clipboard_event(input, egui::Event::Copy) {
                return Some(Tool::Copy);
            }
            [
                (Key::Escape, Modifiers::NONE, Tool::Cursor),
                (Key::S, Modifiers::NONE, Tool::Select),
                (Key::M, Modifiers::NONE, Tool::Move),
                (Key::C, Modifiers::CTRL, Tool::Copy),
                (Key::W, Modifiers::NONE, Tool::Wire),
                (Key::G, Modifiers::NONE, Tool::Ground),
                (Key::P, Modifiers::NONE, Tool::Power),
                (Key::V, Modifiers::NONE, Tool::Place(Part::Voltage)),
                (Key::I, Modifiers::NONE, Tool::Place(Part::Current)),
                (Key::R, Modifiers::NONE, Tool::Place(Part::Resistor)),
                (Key::L, Modifiers::NONE, Tool::Place(Part::Inductor)),
                (Key::C, Modifiers::NONE, Tool::Place(Part::Capacitor)),
                (Key::D, Modifiers::NONE, Tool::Place(Part::Diode)),
            ]
            .into_iter()
            .find_map(|(key, modifiers, tool)| input.consume_key(modifiers, key).then_some(tool))
        })?;
        self.cancel();
        if mode == Tool::Cursor {
            self.selected.clear();
        }
        if mode == Tool::Power {
            self.preview_voltage = None;
            self.preview_turns = 0;
        }
        self.tool = mode;
        Some(mode)
    }

    pub fn cancel(&mut self) {
        self.tool = Tool::Cursor;
        if self.copy_anchor.take().is_some() {
            self.selected.clear();
        }
        self.start = None;
        if let Some(original) = self.drag_origin.take() {
            self.set_geometry(original);
        }
        self.drag_leads.clear();
        self.gesture = None;
        self.dragging_part = false;
    }

    pub fn import(&mut self, text: &str) {
        if self.copy_anchor.take().is_some() {
            self.selected.clear();
        }
        self.index_parts(text);
        let parts = cards(text);
        let old = cards(&self.snapshot);
        if old.len() != parts.len()
            || !old
                .iter()
                .zip(&parts)
                .all(|(a, b)| a.name == b.name && a.part == b.part && a.nodes == b.nodes)
        {
            self.wires.clear();
            self.grounds.clear();
            self.powers.clear();
            self.power_voltages.clear();
            self.power_turns.clear();
            self.selected.clear();
            self.positions
                .retain(|name, _| parts.iter().any(|c| c.name == name));
            if text == crate::simulation::EXAMPLE && self.positions.is_empty() {
                self.positions.extend(
                    [
                        ("V1", (4, 7)),
                        ("R1", (10, 5)),
                        ("L1", (18, 5)),
                        ("C1", (24, 7)),
                    ]
                    .map(|(n, p)| (n.into(), p)),
                );
                self.rotations.extend([("V1".into(), 1), ("C1".into(), 1)]);
            }
            let mut nets = HashMap::new();
            for (i, c) in parts.iter().enumerate() {
                let pos = *self.positions.entry(c.name.into()).or_insert((
                    6 + 9 * (i % 4) as i32,
                    3 + 4 * (i % 3) as i32 + 13 * (i / 4) as i32,
                ));
                for (pin, node) in c
                    .part
                    .terminals(pos, self.turns(c.name))
                    .into_iter()
                    .zip(c.nodes.iter().copied())
                {
                    if node == "0" {
                        self.grounds.push(pin);
                    } else if let Some(other) = nets.insert(node.to_ascii_lowercase(), pin) {
                        self.wires.push((other, pin));
                    }
                }
            }
        }
        self.label_layout = None;
        self.snapshot = text.into();
    }

    fn routed_segments(&self, wire: Wire) -> Vec<Wire> {
        let mut stops = vec![wire.0, wire.1];
        let mut bodies = HashSet::new();
        for (name, &pos) in &self.positions {
            let terminals = self.kinds[name].terminals(pos, self.turns(name));
            let [a, b] = terminals[..] else { continue };
            if on_wire(a, wire) && on_wire(b, wire) {
                stops.extend([a, b]);
                bodies.insert((a.min(b), a.max(b)));
            }
        }
        stops.sort_unstable();
        stops.dedup();
        stops
            .windows(2)
            .map(|p| (p[0], p[1]))
            .filter(|w| !bodies.contains(w))
            .collect()
    }

    fn route(&mut self, wire: Wire) {
        self.wires.extend(self.routed_segments(wire));
    }

    fn preview_junctions(&self, route: &[Wire]) -> Vec<Point> {
        let terminals: Vec<_> = self
            .positions
            .iter()
            .flat_map(|(name, &center)| self.kinds[name].terminals(center, self.turns(name)))
            .chain(self.grounds.iter().chain(&self.powers).copied())
            .collect();
        let points: HashSet<_> = terminals
            .iter()
            .copied()
            .chain(self.wires.iter().chain(route).flat_map(|&(a, b)| [a, b]))
            .collect();
        points
            .into_iter()
            .filter(|&point| {
                if !route.iter().any(|&wire| on_wire(point, wire)) {
                    return false;
                }
                let terminal_count = terminals
                    .iter()
                    .filter(|&&terminal| terminal == point)
                    .count();
                let mut directions = HashSet::new();
                for &(a, b) in &self.wires {
                    if on_wire(point, (a, b)) {
                        directions.extend(
                            [a, b]
                                .into_iter()
                                .filter(|&end| end != point)
                                .map(|end| wire_direction(point, end)),
                        );
                    }
                }
                if directions.len() + terminal_count >= 3 {
                    return false;
                }
                for &(a, b) in route {
                    if on_wire(point, (a, b)) {
                        directions.extend(
                            [a, b]
                                .into_iter()
                                .filter(|&end| end != point)
                                .map(|end| wire_direction(point, end)),
                        );
                    }
                }
                directions.len() + terminal_count >= 3
            })
            .collect()
    }

    fn move_selection(&mut self, delta: Point) {
        self.label_layout = None;
        let shift = |p: &mut Point| {
            p.0 += delta.0;
            p.1 += delta.1;
        };
        let mut moving = HashSet::new();
        let mut fixed = HashSet::new();
        for (name, pos) in &mut self.positions {
            let terminals =
                self.kinds[name].terminals(*pos, self.rotations.get(name).copied().unwrap_or(0));
            if self.selected.contains(&Object::Part(name.clone())) {
                moving.extend(terminals);
                shift(pos);
            } else {
                fixed.extend(terminals);
            }
        }
        for (i, pos) in self.grounds.iter_mut().enumerate() {
            if self.selected.contains(&Object::Ground(i)) {
                moving.insert(*pos);
                shift(pos);
            } else {
                fixed.insert(*pos);
            }
        }
        let old_voltages = std::mem::take(&mut self.power_voltages);
        let old_turns = std::mem::take(&mut self.power_turns);
        for (i, pos) in self.powers.iter_mut().enumerate() {
            let old = *pos;
            if self.selected.contains(&Object::Power(i)) {
                moving.insert(*pos);
                shift(pos);
            } else {
                fixed.insert(*pos);
            }
            if let Some(&turns) = old_turns.get(&old) {
                self.power_turns.insert(*pos, turns);
            }
            if let Some(&voltage) = old_voltages.get(&old) {
                self.power_voltages.insert(*pos, voltage);
            }
        }
        if self.tool == Tool::Move {
            for (index, (a, b)) in self.wires.iter_mut().enumerate() {
                if self.selected.contains(&Object::Wire(index)) {
                    shift(a);
                    shift(b);
                }
            }
            return;
        }
        // A lone wire at a moving pin stretches to follow it. Branches and
        // stationary terminals keep their junction and get a connecting lead.
        let mut counts = HashMap::new();
        for &(a, b) in &self.wires {
            for p in [a, b] {
                *counts.entry(p).or_insert(0) += 1;
            }
        }
        for (i, &(a, b)) in self.wires.iter().enumerate() {
            if self.selected.contains(&Object::Wire(i)) {
                moving.extend([a, b]);
            }
        }
        for (i, (a, b)) in self.wires.iter_mut().enumerate() {
            let selected = self.selected.contains(&Object::Wire(i));
            for end in [a, b] {
                if selected || (moving.contains(end) && counts[end] == 1 && !fixed.contains(end)) {
                    shift(end);
                } else if moving.contains(end) {
                    fixed.insert(*end);
                }
            }
        }
        let previous: HashSet<_> = self.drag_leads.iter().map(|w| w.1).collect();
        for (_, end) in &mut self.drag_leads {
            shift(end);
        }
        for &p in moving.intersection(&fixed) {
            if !previous.contains(&p) {
                let mut end = p;
                shift(&mut end);
                self.drag_leads.push((p, end));
            }
        }
    }

    fn normalize(&mut self) {
        let mut points: Vec<_> = self
            .positions
            .iter()
            .flat_map(|(n, &p)| self.kinds[n].terminals(p, self.turns(n)))
            .chain(self.grounds.iter().chain(&self.powers).copied())
            .collect();
        for &(a, b) in &self.wires {
            points.extend([a, b]);
        }
        points.sort_unstable();
        points.dedup();
        let mut segments = Vec::new();
        let mut selected_segments = HashSet::new();
        for (index, &wire) in self.wires.iter().enumerate() {
            let selected = self.selected.contains(&Object::Wire(index));
            let mut prev = None;
            for &p in points.iter().filter(|&&p| on_wire(p, wire)) {
                if let Some(a) = prev {
                    segments.push((a, p));
                    if selected {
                        selected_segments.insert((a, p));
                    }
                }
                prev = Some(p);
            }
        }
        segments.sort_unstable();
        segments.dedup();
        self.selected.retain(|o| !matches!(o, Object::Wire(_)));
        for (i, w) in segments.iter().enumerate() {
            if selected_segments.contains(w) {
                self.selected.insert(Object::Wire(i));
            }
        }
        self.wires = segments;
    }

    fn symbol_at(&self, q: Point) -> Option<Object> {
        self.positions
            .iter()
            .find(|(n, p)| {
                if self.kinds[*n].layout().len() > 2 {
                    return (q.0 - p.0).abs() <= 1 && (q.1 - p.1).abs() <= 1;
                }
                if matches!(self.kinds[*n], Part::Voltage | Part::Current)
                    && (q.0 - p.0).abs() + (q.1 - p.1).abs() <= 1
                {
                    return true;
                }
                let [a, b] = pins(**p, self.turns(n));
                on_wire(q, (a, b))
            })
            .map(|(n, _)| Object::Part(n.clone()))
            .or_else(|| {
                self.grounds
                    .iter()
                    .position(|&g| g == q || g == (q.0, q.1 - 1))
                    .map(Object::Ground)
            })
            .or_else(|| {
                self.powers
                    .iter()
                    .position(|&pin| {
                        let (dx, dy) =
                            power_direction(self.power_turns.get(&pin).copied().unwrap_or(0));
                        pin == q || (pin.0 + dx, pin.1 + dy) == q
                    })
                    .map(Object::Power)
            })
    }

    fn delete_objects(&mut self, text: &mut String, hovered: Option<Object>) -> bool {
        let mut targets = self.selected.clone();
        if targets.is_empty() {
            targets.extend(hovered);
        }
        if targets.is_empty() {
            return false;
        }
        let lines: HashSet<_> = cards(text)
            .iter()
            .filter(|c| targets.contains(&Object::Part(c.name.into())))
            .map(|c| c.line)
            .collect();
        let mut skip_continuation = false;
        *text = text
            .split_inclusive('\n')
            .enumerate()
            .filter_map(|(i, line)| {
                if !line.trim_start().starts_with('+') {
                    skip_continuation = lines.contains(&i);
                }
                (!skip_continuation).then_some(line)
            })
            .collect();
        self.positions
            .retain(|n, _| !targets.contains(&Object::Part(n.clone())));
        self.rotations.retain(|n, _| self.positions.contains_key(n));
        let mut i = 0;
        self.grounds.retain(|_| {
            let keep = !targets.contains(&Object::Ground(i));
            i += 1;
            keep
        });
        i = 0;
        self.powers.retain(|_| {
            let keep = !targets.contains(&Object::Power(i));
            i += 1;
            keep
        });
        self.power_voltages.retain(|p, _| self.powers.contains(p));
        self.power_turns.retain(|p, _| self.powers.contains(p));
        i = 0;
        self.wires.retain(|_| {
            let keep = !targets.contains(&Object::Wire(i));
            i += 1;
            keep
        });
        self.selected.clear();
        self.gesture = None;
        self.sync(text);
        true
    }

    fn place_copy(&mut self, text: &mut String, delta: Point) -> bool {
        let offset = |(x, y): Point| (x + delta.0, y + delta.1);
        if delta == (0, 0)
            || self.selected.iter().any(|object| {
                if let Object::Part(name) = object {
                    let position = offset(self.positions[name]);
                    self.positions.values().any(|&other| other == position)
                } else {
                    false
                }
            })
        {
            return false;
        }

        let lines: Vec<_> = text.lines().collect();
        let mut names: HashSet<_> = lines
            .iter()
            .skip(1)
            .filter_map(|line| line.split_whitespace().next())
            .map(str::to_ascii_lowercase)
            .collect();
        let mut next_serial = HashMap::new();
        let mut new_cards = String::new();
        for card in cards(text) {
            if !self.selected.contains(&Object::Part(card.name.into())) {
                continue;
            }
            let prefix = card.part.spec().0;
            let serial = next_serial.entry(prefix).or_insert(1);
            let name = loop {
                let name = format!("{prefix}{serial}");
                *serial += 1;
                if names.insert(name.to_ascii_lowercase()) {
                    break name;
                }
            };
            let nodes = vec!["?"; card.part.layout().len()].join(" ");
            new_cards.push_str(&format!("{name} {nodes} {}\n", card.value));
            for line in lines[card.line + 1..]
                .iter()
                .take_while(|line| line.trim_start().starts_with('+'))
            {
                new_cards.push_str(line);
                new_cards.push('\n');
            }
            self.positions
                .insert(name.clone(), offset(self.positions[card.name]));
            self.rotations.insert(name.clone(), self.turns(card.name));
            self.kinds.insert(name, card.part);
        }
        if !new_cards.is_empty() {
            text.insert_str(text.find('\n').unwrap() + 1, &new_cards);
        }
        for object in self.selected.drain() {
            match object {
                Object::Wire(index) => {
                    let (a, b) = self.wires[index];
                    self.wires.push((offset(a), offset(b)));
                }
                Object::Ground(index) => {
                    let point = offset(self.grounds[index]);
                    if !self.grounds.contains(&point) {
                        self.grounds.push(point);
                    }
                }
                Object::Power(index) => {
                    let original = self.powers[index];
                    let point = offset(original);
                    if !self.powers.contains(&point) {
                        self.powers.push(point);
                        if let Some(&turns) = self.power_turns.get(&original) {
                            self.power_turns.insert(point, turns);
                        }
                        if let Some(&voltage) = self.power_voltages.get(&original) {
                            self.power_voltages.insert(point, voltage);
                        }
                    }
                }
                Object::Part(_) => {}
            }
        }
        self.copy_anchor = None;
        true
    }

    fn sync(&mut self, text: &mut String) {
        self.normalize();
        let mut parts = cards(text);
        if self.tool == Tool::Move {
            // Give stationary nets first claim on their original node names.
            parts.sort_by_key(|card| self.selected.contains(&Object::Part(card.name.into())));
        }
        let mut graph: HashMap<Point, Vec<Point>> = HashMap::new();
        for &(a, b) in &self.wires {
            graph.entry(a).or_default().push(b);
            graph.entry(b).or_default().push(a);
        }
        let mut rails = HashMap::new();
        let mut first_terminal = std::collections::BTreeMap::new();
        for &point in &self.powers {
            let voltage = self.power_voltages.get(&point).copied();
            let name = rail_net(voltage);
            if let Some(&(first, _)) = first_terminal.get(&name) {
                graph.entry(first).or_default().push(point);
                graph.entry(point).or_default().push(first);
            } else {
                first_terminal.insert(name.clone(), (point, voltage));
            }
            rails.insert(point, name);
        }
        let mut groups = HashMap::new();
        let mut labels = Vec::new();
        let mut used = HashSet::new();
        let reserved: HashSet<_> = text
            .split_whitespace()
            .map(str::to_ascii_lowercase)
            .collect();
        let mut serial = 1;
        let terminals = parts.iter().flat_map(|c| {
            c.part
                .terminals(self.positions[c.name], self.turns(c.name))
                .into_iter()
                .zip(c.nodes.iter().copied())
        });
        for (p, old) in terminals.chain(self.powers.iter().map(|&p| (p, "?"))) {
            if groups.contains_key(&p) {
                continue;
            }
            let id = labels.len();
            let mut power = None;
            let mut grounded = false;
            let mut stack = vec![p];
            while let Some(q) = stack.pop() {
                if groups.contains_key(&q) {
                    continue;
                }
                groups.insert(q, id);
                if let Some(name) = rails.get(&q)
                    && power.is_none_or(|old| name < old)
                {
                    power = Some(name);
                }
                grounded |= self.grounds.contains(&q);
                if let Some(next) = graph.get(&q) {
                    stack.extend(next);
                }
            }
            let name = if grounded {
                "0".into()
            } else if let Some(name) = power {
                name.clone()
            } else if (self.powers.is_empty() || !old.eq_ignore_ascii_case("vcc"))
                && !old.to_ascii_lowercase().starts_with("os_vcc_")
                && old != "0"
                && old != "?"
                && used.insert(old.to_ascii_lowercase())
            {
                old.to_owned()
            } else {
                loop {
                    let n = format!("n{serial}");
                    serial += 1;
                    if !reserved.contains(&n) && used.insert(n.clone()) {
                        break n;
                    }
                }
            };
            labels.push(name);
        }
        let mut lines: Vec<_> = text.lines().map(str::to_owned).collect();
        for c in parts {
            let nodes = c
                .part
                .terminals(self.positions[c.name], self.turns(c.name))
                .iter()
                .map(|p| labels[groups[p]].as_str())
                .collect::<Vec<_>>()
                .join(" ");
            lines[c.line] = format!("{} {nodes} {}", c.name, c.value);
        }
        if lines.is_empty() {
            lines.push("OpenSpice schematic".into());
        }
        // The first line is the SPICE title, even if it resembles a source.
        let mut line_index = 0;
        lines.retain(|line| {
            let keep = line_index == 0 || !line.trim_start().starts_with(RAIL_SOURCE);
            line_index += 1;
            keep
        });
        let sources = first_terminal.values().filter_map(|&(point, voltage)| {
            let voltage = voltage?;
            Some(format!(
                "{RAIL_SOURCE}{} {} 0 DC {voltage}",
                rail_net(Some(voltage)),
                labels[groups[&point]]
            ))
        });
        lines.splice(1..1, sources);
        *text = lines.join("\n") + "\n";
        self.label_layout = None;
        self.snapshot = text.clone();
    }

    fn power_node<'a>(&self, point: Point, text: &'a str) -> Option<&'a str> {
        let voltage = self.power_voltages.get(&point)?;
        let source = format!("{RAIL_SOURCE}{}", rail_net(Some(*voltage)));
        text.lines().skip(1).find_map(|line| {
            let mut fields = line.split_whitespace();
            if fields.next()? == source {
                fields.next()
            } else {
                None
            }
        })
    }

    fn wire_node<'a>(&self, wire: usize, text: &'a str) -> Option<&'a str> {
        let terminals: Vec<_> = cards(text)
            .into_iter()
            .flat_map(|card| {
                card.part
                    .terminals(self.positions[card.name], self.turns(card.name))
                    .into_iter()
                    .zip(card.nodes)
            })
            .collect();
        let mut pending = vec![wire];
        let mut visited = HashSet::new();
        while let Some(index) = pending.pop() {
            if !visited.insert(index) {
                continue;
            }
            let segment = self.wires[index];
            if self.grounds.iter().any(|&point| on_wire(point, segment)) {
                return Some("0");
            }
            if let Some((_, node)) = terminals
                .iter()
                .find(|&&(point, _)| on_wire(point, segment))
            {
                return Some(node);
            }
            if let Some(node) = self
                .powers
                .iter()
                .copied()
                .filter(|&point| on_wire(point, segment))
                .find_map(|point| self.power_node(point, text))
            {
                return Some(node);
            }
            for (index, &(a, b)) in self.wires.iter().enumerate() {
                if !visited.contains(&index)
                    && (on_wire(a, segment)
                        || on_wire(b, segment)
                        || on_wire(segment.0, (a, b))
                        || on_wire(segment.1, (a, b)))
                {
                    pending.push(index);
                }
            }
        }
        None
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        text: &mut String,
        probe: Option<&mut Option<(String, bool)>>,
    ) -> bool {
        if self.snapshot != *text {
            self.import(text);
        }
        let mut changed = false;
        if !ui.ctx().text_edit_focused()
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::R))
            && self.rotate()
        {
            changed = true;
        }
        {
            let (r, p) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
            self.viewport = r.rect.size();
            if let Some(pos) = r.hover_pos() {
                let factor = ui.input(crate::ui::canvas_zoom_factor);
                if factor != 1. {
                    self.zoom(factor.log(1.25) as f32, Some(pos - r.rect.min));
                }
            }
            if r.dragged_by(egui::PointerButton::Middle) {
                self.pan += r.drag_delta();
            }
            let scale = 1.25_f32.powf(self.zoom_steps);
            let origin = r.rect.min + self.pan;
            let screen = |(x, y): Point| origin + vec2(x as f32, y as f32) * (GRID * scale);
            let snap = |pos: Pos2| {
                let d = ((pos - origin) / (GRID * scale)).round();
                (d.x as i32, d.y as i32)
            };
            let rect = r.rect.intersect(p.clip_rect());
            p.rect_filled(rect, 0, PAPER);
            self.draw_grid(&p, rect, origin, GRID * scale);
            let mouse = ui.input(|i| i.pointer.interact_pos());
            let point = mouse.map(snap).unwrap_or_default();
            let inside = mouse.is_some_and(|pos| rect.contains(pos));
            let wire_at = |pos: Pos2, wires: &[Wire]| {
                wires
                    .iter()
                    .enumerate()
                    .filter_map(|(i, &(a, b))| {
                        let direction = screen(b) - screen(a);
                        let fraction = ((pos - screen(a)).dot(direction)
                            / direction.length_sq().max(0.001))
                        .clamp(0., 1.);
                        let distance = pos.distance(screen(a) + direction * fraction);
                        (distance <= 6.).then_some((i, distance))
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(i, _)| i)
            };
            if r.hovered()
                && self.accepts_tool_shortcuts(ui)
                && self.drag_origin.is_none()
                && self.copy_anchor.is_none()
                && !ui.input(|i| i.pointer.any_down())
                && !ui.ctx().memory(|memory| memory.top_modal_layer().is_some())
                && ui.input_mut(|i| {
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                        || i.consume_key(egui::Modifiers::CTRL, egui::Key::X)
                        || consume_clipboard_event(i, egui::Event::Cut)
                })
            {
                let hovered = r.hover_pos().and_then(|pos| {
                    self.symbol_at(snap(pos))
                        .or_else(|| wire_at(pos, &self.wires).map(Object::Wire))
                });
                if self.delete_objects(text, hovered) {
                    ui.ctx().request_repaint();
                    return true;
                }
                self.cancel();
                self.tool = Tool::Erase;
            }
            let color = |selected| {
                if selected { ACCENT } else { WIRE }
            };
            // Use the same path rendering as component leads. egui pixel-snaps
            // line segments independently, which shifts them off the pin center.
            let mut degree = HashMap::<Point, usize>::new();
            for (i, &(a, b)) in self.wires.iter().enumerate() {
                let endpoints = [screen(a), screen(b)];
                if egui::Rect::from_two_pos(endpoints[0], endpoints[1])
                    .intersects(rect.expand(SCHEMATIC_STROKE))
                {
                    p.line(
                        endpoints.to_vec(),
                        Stroke::new(
                            SCHEMATIC_STROKE,
                            color(self.selected.contains(&Object::Wire(i))),
                        ),
                    );
                }
                for q in [a, b] {
                    *degree.entry(q).or_default() += 1;
                }
            }
            for &(a, b) in &self.drag_leads {
                p.line(
                    vec![screen(a), screen(b)],
                    Stroke::new(SCHEMATIC_STROKE, WIRE),
                );
            }
            let parts = cards(text);
            // Only geometry edits can reflow existing labels. A floating
            // component finds room for its own label after these are placed.
            let preview_layout = self.drag_origin.is_some() || self.start.is_some();
            let placing = inside && matches!(self.tool, Tool::Place(_));
            let world = |(x, y): Point| egui::pos2(x as f32 * GRID, y as f32 * GRID);
            let project = |r: egui::Rect| {
                egui::Rect::from_min_max(
                    origin + r.min.to_vec2() * scale,
                    origin + r.max.to_vec2() * scale,
                )
            };
            if !preview_layout && self.label_layout.is_none() {
                let mut occupied: Vec<_> = parts
                    .iter()
                    .map(|c| {
                        symbol_bounds(
                            c.part,
                            world(self.positions[c.name]),
                            self.turns(c.name),
                            1.,
                        )
                    })
                    .collect();
                let wires: Vec<_> = self
                    .wires
                    .iter()
                    .map(|&(a, b)| [world(a), world(b)])
                    .collect();
                self.label_layout = Some(
                    parts
                        .iter()
                        .map(|c| {
                            let (name, value) = label_text(&p, c, 1.);
                            place_label(
                                world(self.positions[c.name]),
                                1.,
                                label_size(&name, &value),
                                12.,
                                &wires,
                                &mut occupied,
                                egui::Rect::EVERYTHING,
                            )
                            .unwrap()
                        })
                        .collect(),
                );
            }
            let mut obstacles: Vec<_> = parts
                .iter()
                .map(|c| {
                    symbol_bounds(
                        c.part,
                        screen(self.positions[c.name]),
                        self.turns(c.name),
                        scale,
                    )
                })
                .collect();
            let mut segments: Vec<_> = self
                .wires
                .iter()
                .chain(&self.drag_leads)
                .map(|&(a, b)| [screen(a), screen(b)])
                .collect();
            if let Some(start) = self.start.filter(|_| inside && self.tool == Tool::Wire) {
                let elbow = (point.0, start.1);
                segments.extend([
                    [screen(start), screen(elbow)],
                    [screen(elbow), screen(point)],
                ]);
            }
            for &pin in self.grounds.iter().chain(&self.powers) {
                *degree.entry(pin).or_default() += 1;
            }
            for &(a, b) in &self.drag_leads {
                *degree.entry(a).or_default() += 1;
                *degree.entry(b).or_default() += 1;
            }
            for c in &parts {
                for pin in c.part.terminals(self.positions[c.name], self.turns(c.name)) {
                    *degree.entry(pin).or_default() += 1;
                }
            }
            let mut hovered_label = None;
            for (index, c) in parts.into_iter().enumerate() {
                let pos = self.positions[c.name];
                let color = if self.selected.contains(&Object::Part(c.name.into())) {
                    ACCENT
                } else {
                    INK
                };
                if obstacles[index].intersects(rect) {
                    draw(
                        &p,
                        screen(pos),
                        (c.part, c.value),
                        self.turns(c.name),
                        scale,
                        self.american_resistor,
                        color,
                    );
                    for pin in c.part.terminals(pos, self.turns(c.name)) {
                        if degree[&pin] == 1 {
                            draw_open_pin(&p, screen(pin), scale, color);
                        }
                    }
                }
                let bounds = if preview_layout {
                    draw_labels(&p, screen(pos), &c, scale, &segments, &mut obstacles)
                } else {
                    let bounds = project(self.label_layout.as_ref().unwrap()[index]);
                    if placing || self.fit_pending || self.operating_point.is_some() {
                        obstacles.push(bounds);
                    }
                    if !bounds.intersects(rect) || 11. * scale * p.ctx().pixels_per_point() < 6. {
                        continue;
                    }
                    let (name, value) = label_text(&p, &c, scale);
                    paint_label(&p, bounds.min, name, value);
                    bounds
                };
                if r.hover_pos().is_some_and(|pos| bounds.contains(pos)) {
                    hovered_label = Some(c.name.to_owned());
                }
            }
            let ground = |q, col| {
                let z = screen(q);
                let pen = Stroke::new(SCHEMATIC_STROKE, col);
                p.line(vec![z, z + vec2(0., 9.) * scale], pen);
                for (height, half_width) in [(9., 9.), (13., 6.), (17., 3.)] {
                    p.line(
                        vec![
                            z + vec2(-half_width, height) * scale,
                            z + vec2(half_width, height) * scale,
                        ],
                        pen,
                    );
                }
            };
            for (i, &q) in self.grounds.iter().enumerate() {
                if rect.expand(22. * scale + 2.).contains(screen(q)) {
                    ground(
                        q,
                        if self.selected.contains(&Object::Ground(i)) {
                            ACCENT
                        } else {
                            INK
                        },
                    );
                }
            }
            let power = |q, voltage: Option<f64>, turns, col| {
                let z = screen(q);
                power_arrow(&p, z, scale, turns, col);
                let (dx, dy) = power_direction(turns);
                let direction = vec2(dx as f32, dy as f32);
                let align = [
                    Align2::CENTER_BOTTOM,
                    Align2::LEFT_CENTER,
                    Align2::CENTER_TOP,
                    Align2::RIGHT_CENTER,
                ][usize::from(turns % 4)];
                let label = voltage.map_or_else(|| "Vcc".into(), |v| format!("{v} V"));
                let bounds = p.text(
                    z + direction * (18. * scale),
                    align,
                    label,
                    FontId::proportional(12. * scale),
                    col,
                );
                bounds.union(
                    egui::Rect::from_two_pos(z, z + direction * (16. * scale)).expand(6. * scale),
                )
            };
            let mut hovered_power = None;
            for (i, &q) in self.powers.iter().enumerate() {
                if rect.expand(36. * scale + 2.).contains(screen(q)) {
                    let bounds = power(
                        q,
                        self.power_voltages.get(&q).copied(),
                        self.power_turns.get(&q).copied().unwrap_or(0),
                        if self.selected.contains(&Object::Power(i)) {
                            ACCENT
                        } else {
                            INK
                        },
                    );
                    if r.hover_pos()
                        .is_some_and(|pos| bounds.expand(3.).contains(pos))
                    {
                        hovered_power = Some(q);
                    }
                }
            }
            if std::mem::take(&mut self.fit_pending) {
                let mut bounds = egui::Rect::NOTHING;
                for rect in &obstacles {
                    bounds = bounds.union(*rect);
                }
                for &[a, b] in &segments {
                    bounds.extend_with(a);
                    bounds.extend_with(b);
                }
                for &q in self.grounds.iter().chain(&self.powers) {
                    bounds = bounds.union(egui::Rect::from_center_size(
                        screen(q),
                        vec2(42., 76.) * scale,
                    ));
                }
                if bounds.is_finite() {
                    let available = (self.viewport - vec2(40., 40.)).max(vec2(1., 1.));
                    let size = bounds.size().max(vec2(1., 1.));
                    let factor = (available.x / size.x)
                        .min(available.y / size.y)
                        .min(1.25_f32.powf(7.) / scale);
                    self.pan = self.viewport * 0.5 - (bounds.center() - origin) * factor;
                    self.zoom_steps += factor.log(1.25);
                    ui.ctx().request_repaint();
                }
            }
            for (&q, &n) in &degree {
                if n >= 3 && rect.expand(4.).contains(screen(q)) {
                    p.circle_filled(screen(q), 4., WIRE);
                }
            }
            let mut probe_clicked = false;
            if (probe.is_some() || self.operating_point.is_some())
                && self.tool == Tool::Cursor
                && self.editing.is_none()
                && self.editing_power.is_none()
                && !r.dragged()
                && !r.secondary_clicked()
                && let Some(pos) = r.hover_pos()
            {
                let pin = if self.operating_point.is_some() && screen(point).distance(pos) <= 6. {
                    cards(text)
                        .into_iter()
                        .find_map(|card| {
                            card.part
                                .terminals(self.positions[card.name], self.turns(card.name))
                                .into_iter()
                                .zip(card.nodes)
                                .find_map(|(pin, node)| (pin == point).then_some(node))
                        })
                        .or_else(|| self.grounds.contains(&point).then_some("0"))
                        .or_else(|| self.power_node(point, text))
                } else {
                    None
                };
                let wire = wire_at(pos, &self.wires);
                let target = if pin.is_some() || wire.is_some() {
                    probe_cursor(ui, pos, false);
                    if r.clicked() {
                        let node =
                            pin.or_else(|| wire.and_then(|index| self.wire_node(index, text)));
                        node.map(|node| (format!("V({node})"), point))
                    } else {
                        None
                    }
                } else if let Some(name) = hovered_label.as_ref().or_else(|| {
                    self.positions
                        .iter()
                        .find(|&(_, &center)| {
                            egui::Rect::from_center_size(screen(center), vec2(34., 34.) * scale)
                                .contains(pos)
                        })
                        .map(|(name, _)| name)
                }) {
                    probe_cursor(ui, pos, true);
                    r.clicked()
                        .then(|| (format!("I({name})"), self.positions[name]))
                } else {
                    None
                };
                if let Some((signal, anchor)) = target {
                    probe_clicked = true;
                    if let Some(op) = &mut self.operating_point {
                        let key = signal.to_ascii_lowercase();
                        if op.values.contains_key(&key) {
                            op.probes.insert(key, anchor);
                            ui.ctx().request_repaint();
                        }
                    } else if let Some(probe) = probe {
                        *probe = Some((signal, r.double_clicked() || r.triple_clicked()));
                    }
                }
            }

            if let Some(op) = &self.operating_point {
                let mut annotations = op.probes.clone();
                if op.display.voltages || op.display.currents {
                    for card in cards(text) {
                        let center = self.positions[card.name];
                        if op.display.currents {
                            annotations
                                .entry(format!("i({})", card.name.to_ascii_lowercase()))
                                .or_insert(center);
                        }
                        if op.display.voltages {
                            for (pin, node) in card
                                .part
                                .terminals(center, self.turns(card.name))
                                .into_iter()
                                .zip(card.nodes)
                                .filter(|&(_, node)| node != "0")
                            {
                                annotations
                                    .entry(format!("v({})", node.to_ascii_lowercase()))
                                    .or_insert(pin);
                            }
                        }
                    }
                    if op.display.voltages {
                        for &point in &self.powers {
                            if let Some(node) =
                                self.power_node(point, text).filter(|&node| node != "0")
                            {
                                annotations
                                    .entry(format!("v({})", node.to_ascii_lowercase()))
                                    .or_insert(point);
                            }
                        }
                    }
                }
                for (signal, anchor) in annotations {
                    let Some(&value) = op.values.get(&signal).filter(|v| v.is_finite()) else {
                        continue;
                    };
                    let center = screen(anchor);
                    if !rect.expand(30.).contains(center) {
                        continue;
                    }
                    let text = format_operating_value(value, signal.starts_with("i("));
                    let galley = p.layout_no_wrap(text, FontId::proportional(17.), ACCENT);
                    let Some(bounds) = place_label(
                        center,
                        scale,
                        galley.size(),
                        4.,
                        &segments,
                        &mut obstacles,
                        rect,
                    ) else {
                        continue;
                    };
                    p.galley(bounds.min, galley, ACCENT);
                }
            }
            let edit_power =
                r.secondary_clicked() && self.tool == Tool::Power && hovered_power.is_some();
            if edit_power
                || (self.tool == Tool::Cursor
                    && ((!probe_clicked && r.double_clicked()) || r.secondary_clicked()))
            {
                let name = hovered_label.or_else(|| match self.symbol_at(point) {
                    Some(Object::Part(n)) => Some(n),
                    _ => None,
                });
                if let Some(point) = hovered_power {
                    self.editing_power = Some((
                        point,
                        self.power_voltages
                            .get(&point)
                            .map(ToString::to_string)
                            .unwrap_or_default(),
                    ));
                } else if let Some(c) = cards(text)
                    .into_iter()
                    .find(|c| Some(c.name) == name.as_deref())
                {
                    self.cancel();
                    self.editing = Some(Properties {
                        part: c.part,
                        parasitics: parasitic_fields(c.part)
                            .iter()
                            .map(|(key, _)| {
                                parasitic_value(split_parasitics(c.part, c.value).1, key).to_owned()
                            })
                            .collect(),
                        parameters: c
                            .part
                            .parameters()
                            .iter()
                            .map(|(key, _)| {
                                c.value
                                    .split_whitespace()
                                    .chain(c.part.spec().2.split_whitespace())
                                    .filter_map(|s| s.split_once('='))
                                    .find(|(name, _)| name.eq_ignore_ascii_case(key))
                                    .map(|(_, v)| v.to_owned())
                                    .unwrap_or_default()
                            })
                            .collect(),
                        source: matches!(c.part, Part::Voltage | Part::Current)
                            .then(|| crate::source::Source::parse(c.value)),
                        original: c.name.into(),
                        reference: c.name.into(),
                        value: split_parasitics(c.part, c.value).0.into(),
                    });
                }
            }
            if r.secondary_clicked() {
                self.cancel();
            }
            let released = ui.input(|i| i.pointer.primary_released());
            let drop = inside && self.dragging_part && released;
            if released {
                self.dragging_part = false;
            }
            if let Some(pos) = r
                .hover_pos()
                .or_else(|| r.is_pointer_button_down_on().then_some(mouse).flatten())
                .filter(|&pos| rect.contains(pos))
            {
                crate::ui::schematic_tool_cursor(ui, pos, rect, self.tool);
            }
            match self.tool {
                Tool::Place(part) if inside => {
                    let (prefix, _, default_value) = part.spec();
                    let name = (1..)
                        .map(|i| format!("{prefix}{i}"))
                        .find(|n| {
                            !text
                                .lines()
                                .skip(1)
                                .filter_map(|l| l.split_whitespace().next())
                                .any(|s| s.eq_ignore_ascii_case(n))
                        })
                        .unwrap();
                    let c = Card {
                        part,
                        name: &name,
                        nodes: vec!["?"; part.layout().len()],
                        value: default_value,
                        line: 0,
                    };
                    draw(
                        &p,
                        screen(point),
                        (c.part, c.value),
                        self.preview_turns,
                        scale,
                        self.american_resistor,
                        ACCENT,
                    );
                    for pin in part.terminals(point, self.preview_turns) {
                        if !degree.contains_key(&pin)
                            && !self.wires.iter().any(|&wire| on_wire(pin, wire))
                        {
                            draw_open_pin(&p, screen(pin), scale, ACCENT);
                        }
                    }
                    obstacles.push(symbol_bounds(
                        part,
                        screen(point),
                        self.preview_turns,
                        scale,
                    ));
                    draw_labels(&p, screen(point), &c, scale, &segments, &mut obstacles);
                    if (r.clicked() || drop) && !self.positions.values().any(|&p| p == point) {
                        if !text.contains('\n') {
                            text.push('\n');
                        }
                        text.insert_str(
                            text.find('\n').unwrap() + 1,
                            &format!("{name} {} {}\n", c.nodes.join(" "), c.value),
                        );
                        if part.spec().0 == 'X' {
                            ensure_model(text, default_value);
                        }
                        self.kinds.insert(name.clone(), part);
                        self.rotations.insert(name.clone(), self.preview_turns);
                        self.positions.insert(name, point);
                        changed = true;
                    }
                }
                Tool::Wire if inside => {
                    if let Some(a) = self.start {
                        let elbow = (point.0, a.1);
                        p.add(egui::Shape::line(
                            vec![screen(a), screen(elbow), screen(point)],
                            Stroke::new(SCHEMATIC_STROKE, ACCENT),
                        ));
                        let mut route = self.routed_segments((a, elbow));
                        route.extend(self.routed_segments((elbow, point)));
                        for junction in self.preview_junctions(&route) {
                            p.circle(screen(junction), 5., ACCENT, Stroke::new(1.5, PAPER));
                        }
                    }
                    if r.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::None);
                        let center = screen(point);
                        for axis in [vec2(11., 0.), vec2(0., 11.)] {
                            let line = [center - axis, center + axis];
                            p.line_segment(line, Stroke::new(3., PAPER));
                            p.line_segment(line, Stroke::new(1., ACCENT));
                        }
                    }
                    if r.clicked() {
                        if let Some(a) = self.start.replace(point) {
                            if a != point {
                                let elbow = (point.0, a.1);
                                self.route((a, elbow));
                                self.route((elbow, point));
                                changed = true;
                            }
                        } else {
                            self.start = Some(point);
                        }
                    }
                }
                Tool::Power if inside => {
                    power(point, self.preview_voltage, self.preview_turns, ACCENT);
                    if r.clicked() && !self.powers.contains(&point) {
                        self.powers.push(point);
                        if self.preview_turns != 0 {
                            self.power_turns.insert(point, self.preview_turns);
                        }
                        if let Some(voltage) = self.preview_voltage {
                            self.power_voltages.insert(point, voltage);
                        }
                        changed = true;
                    }
                }
                Tool::Ground if inside => {
                    ground(point, ACCENT);
                    if r.clicked() && !self.grounds.contains(&point) {
                        self.grounds.push(point);
                        changed = true;
                    }
                }
                Tool::Copy if let Some(anchor) = self.copy_anchor => {
                    if inside {
                        let delta = (point.0 - anchor.0, point.1 - anchor.1);
                        let offset = |(x, y): Point| (x + delta.0, y + delta.1);
                        for (index, card) in cards(text).into_iter().enumerate() {
                            if self.selected.contains(&Object::Part(card.name.into())) {
                                draw(
                                    &p,
                                    screen(offset(self.positions[card.name])),
                                    (card.part, card.value),
                                    self.turns(card.name),
                                    scale,
                                    self.american_resistor,
                                    ACCENT,
                                );
                                if let Some(layout) = &self.label_layout
                                    && 11. * scale * p.ctx().pixels_per_point() >= 6.
                                {
                                    let label_position =
                                        project(layout[index]).min + (screen(delta) - origin);
                                    let (name, value) = label_text(&p, &card, scale);
                                    paint_label(&p, label_position, name, value);
                                }
                            }
                        }
                        for object in &self.selected {
                            match *object {
                                Object::Wire(index) => {
                                    let (a, b) = self.wires[index];
                                    p.line(
                                        vec![screen(offset(a)), screen(offset(b))],
                                        Stroke::new(SCHEMATIC_STROKE, ACCENT),
                                    );
                                }
                                Object::Ground(index) => {
                                    ground(offset(self.grounds[index]), ACCENT)
                                }
                                Object::Power(index) => {
                                    let point = self.powers[index];
                                    power(
                                        offset(point),
                                        self.power_voltages.get(&point).copied(),
                                        self.power_turns.get(&point).copied().unwrap_or(0),
                                        ACCENT,
                                    );
                                }
                                Object::Part(_) => {}
                            }
                        }
                        if r.clicked() {
                            changed = self.place_copy(text, delta);
                        }
                    }
                }
                Tool::Erase | Tool::Copy if r.clicked() => {
                    if let Some(hit) = self.symbol_at(point).or_else(|| {
                        mouse
                            .and_then(|p| wire_at(p, &self.wires))
                            .map(Object::Wire)
                    }) {
                        if !self.selected.contains(&hit) {
                            self.selected.clear();
                        }
                        if self.tool == Tool::Erase {
                            return self.delete_objects(text, Some(hit));
                        }
                        self.selected.insert(hit);
                        self.copy_anchor = Some(point);
                        ui.ctx().request_repaint();
                    }
                }
                Tool::Move if inside => {
                    if let Some(Gesture::Move(anchor)) = self.gesture
                        && let Some(original) = &self.drag_origin
                    {
                        let mut baseline = original.clone();
                        baseline.rotations.clone_from(&self.rotations);
                        baseline.power_turns = baseline
                            .powers
                            .iter()
                            .zip(&self.powers)
                            .filter_map(|(&origin, current)| {
                                self.power_turns.get(current).map(|&turns| (origin, turns))
                            })
                            .collect();
                        self.set_geometry(baseline);
                        self.move_selection((point.0 - anchor.0, point.1 - anchor.1));

                        let unique = |points: &[Point]| {
                            points.iter().collect::<HashSet<_>>().len() == points.len()
                        };
                        if r.clicked()
                            && unique(&self.positions.values().copied().collect::<Vec<_>>())
                            && unique(&self.grounds)
                            && unique(&self.powers)
                        {
                            changed = self
                                .drag_origin
                                .take()
                                .is_some_and(|g| g != self.geometry());
                            self.gesture = None;
                        }
                    } else if r.clicked()
                        && let Some(hit) = self.symbol_at(point).or_else(|| {
                            mouse
                                .and_then(|pos| wire_at(pos, &self.wires))
                                .map(Object::Wire)
                        })
                    {
                        if !self.selected.contains(&hit) {
                            self.selected.clear();
                            self.selected.insert(hit);
                        }
                        self.drag_origin = Some(self.geometry());
                        self.gesture = Some(Gesture::Move(point));
                    }
                }
                Tool::Cursor | Tool::Select | Tool::Erase | Tool::Copy => {
                    if r.drag_started_by(egui::PointerButton::Primary) || r.clicked() {
                        let pos = ui
                            .input(|i| i.pointer.press_origin())
                            .or(mouse)
                            .unwrap_or(screen(point));
                        let q = snap(pos);
                        let hit = self
                            .symbol_at(q)
                            .or_else(|| wire_at(pos, &self.wires).map(Object::Wire));
                        if matches!(self.tool, Tool::Select | Tool::Erase | Tool::Copy)
                            && r.drag_started_by(egui::PointerButton::Primary)
                        {
                            if self.tool != Tool::Select || !ui.input(|i| i.modifiers.shift) {
                                self.selected.clear();
                            }
                        } else if self.tool == Tool::Select && ui.input(|i| i.modifiers.shift) {
                            if let Some(obj) = hit
                                && !self.selected.remove(&obj)
                            {
                                self.selected.insert(obj);
                            }
                        } else if self.tool == Tool::Select
                            || hit.as_ref().is_none_or(|h| !self.selected.contains(h))
                        {
                            self.selected.clear();
                            self.selected.extend(hit);
                        }
                        self.drag_origin = None;
                        self.gesture =
                            Some(if self.tool == Tool::Cursor && !self.selected.is_empty() {
                                Gesture::Move(q)
                            } else {
                                Gesture::Box(pos)
                            });
                    }
                    if let Some(gesture) = self.gesture {
                        let selection = match gesture {
                            Gesture::Box(anchor) => {
                                Some(egui::Rect::from_two_pos(anchor, mouse.unwrap_or(anchor)))
                            }
                            Gesture::Move(_) => None,
                        };
                        if let Some(rect) = selection
                            && r.dragged_by(egui::PointerButton::Primary)
                        {
                            let outline = [
                                rect.left_top(),
                                rect.right_top(),
                                rect.right_bottom(),
                                rect.left_bottom(),
                                rect.left_top(),
                            ];
                            p.extend(egui::Shape::dashed_line(
                                &outline,
                                Stroke::new(1., ACCENT),
                                5.,
                                3.,
                            ));
                        }
                        if let Gesture::Move(q) = gesture
                            && (r.dragged_by(egui::PointerButton::Primary)
                                || r.drag_stopped_by(egui::PointerButton::Primary))
                            && (q != point || self.drag_origin.is_some())
                        {
                            if self.drag_origin.is_none() {
                                self.drag_origin = Some(self.geometry());
                            }
                            self.set_geometry(self.drag_origin.clone().unwrap());
                            self.drag_leads.clear();
                            let delta = (point.0 - q.0, point.1 - q.1);
                            self.move_selection(delta);
                        }
                        if r.drag_stopped_by(egui::PointerButton::Primary) {
                            if let Some(selection) = selection {
                                let contains = |p| selection.contains(screen(p));
                                for (n, &pos) in &self.positions {
                                    if contains(pos) {
                                        self.selected.insert(Object::Part(n.clone()));
                                    }
                                }
                                for (i, &pos) in self.grounds.iter().enumerate() {
                                    if contains(pos) {
                                        self.selected.insert(Object::Ground(i));
                                    }
                                }
                                for (i, &pos) in self.powers.iter().enumerate() {
                                    if contains(pos) {
                                        self.selected.insert(Object::Power(i));
                                    }
                                }
                                for (i, &(a, b)) in self.wires.iter().enumerate() {
                                    if contains(a) && contains(b) {
                                        self.selected.insert(Object::Wire(i));
                                    }
                                }
                            } else {
                                self.wires.append(&mut self.drag_leads);
                                changed = true;
                            }
                            self.drag_origin = None;
                            self.gesture = None;
                            if self.tool == Tool::Erase {
                                return self.delete_objects(text, None);
                            }
                            if self.tool == Tool::Copy && !self.selected.is_empty() {
                                self.copy_anchor = Some(point);
                                ui.ctx().request_repaint();
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        if let Some((point, mut value)) = self.editing_power.take() {
            let dialog = crate::ui::dialog(ui.ctx(), "power rail properties", "Power rail", |ui| {
                egui::Grid::new("power rail fields")
                    .min_col_width(132.)
                    .show(ui, |ui| {
                        crate::ui::dialog_field(ui, "DC voltage (V)", &mut value);
                    });
                ui.weak("Leave blank to use Vcc as a net connected to an external source.");
                let valid = value.trim().is_empty() || crate::analysis::number(&value).is_some();
                crate::ui::dialog_actions(ui, "Apply", valid, true)
            });
            if dialog.inner {
                let voltage = crate::analysis::number(&value).map(|v| if v == 0. { 0. } else { v });
                if voltage != self.power_voltages.get(&point).copied() {
                    if let Some(voltage) = voltage {
                        self.power_voltages.insert(point, voltage);
                    } else {
                        self.power_voltages.remove(&point);
                    }
                    changed = true;
                }
            } else if !dialog.should_close() {
                self.editing_power = Some((point, value));
            }
        }

        if let Some(mut edit) = self.editing.take() {
            let min_height = edit
                .source
                .as_ref()
                .map(crate::source::Source::dialog_min_height);
            let dialog = crate::ui::dialog_with_min_height(
                ui.ctx(),
                // egui remembers the modal's size by ID.
                egui::Id::unique(("component properties", edit.part)),
                "Component properties",
                min_height,
                |ui| {
                    egui::Grid::new("component fields")
                        .min_col_width(132.)
                        .show(ui, |ui| {
                            crate::ui::dialog_field(ui, "Reference", &mut edit.reference);
                            if let Some(source) = &mut edit.source {
                                source.show(ui, edit.original.starts_with(['I', 'i']));
                                edit.value = source.definition();
                            } else if edit.part == Part::Diode {
                                ui.label("Diode type");
                                egui::ComboBox::from_id_salt("diode type")
                                    .width(crate::ui::DIALOG_FIELD_WIDTH)
                                    .selected_text(diode_type(&edit.value).1)
                                    .show_ui(ui, |ui| {
                                        for (model, label) in DIODES {
                                            ui.selectable_value(
                                                &mut edit.value,
                                                model.to_owned(),
                                                label,
                                            );
                                        }
                                    });
                                ui.end_row();
                            } else if edit.part.spec().0 == 'X' {
                                ui.label("Model");
                                ui.strong(edit.part.spec().1);
                                ui.end_row();
                                for ((_, label), v) in
                                    edit.part.parameters().iter().zip(&mut edit.parameters)
                                {
                                    crate::ui::dialog_field(ui, label, v);
                                }
                            } else {
                                crate::ui::dialog_field(ui, "Value", &mut edit.value);
                            }
                            for ((_, label), value) in
                                parasitic_fields(edit.part).iter().zip(&mut edit.parasitics)
                            {
                                crate::ui::dialog_field(ui, label, value);
                            }
                        });
                    let valid_name = edit.reference.starts_with(|c: char| {
                        c.eq_ignore_ascii_case(&edit.original.chars().next().unwrap())
                    }) && edit
                        .reference
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_');
                    let duplicate = cards(text).iter().any(|c| {
                        c.name != edit.original && c.name.eq_ignore_ascii_case(&edit.reference)
                    });
                    let error = if !valid_name {
                        Some("Keep the component prefix and use letters, numbers or underscores.")
                    } else if duplicate {
                        Some("This reference is already in use.")
                    } else if edit.source.as_ref().is_some_and(|s| !s.valid()) {
                        Some("Check waveform parameters and enter numeric AC magnitude and phase.")
                    } else if edit.part == Part::Op
                        && !matches!(
                            (crate::analysis::number(&edit.parameters[0]), crate::analysis::number(&edit.parameters[1])),
                            (Some(high), Some(low)) if high > low
                        )
                    {
                        Some(
                            "Enter two supply voltages with the positive rail above the negative rail.",
                        )
                    } else if edit.part != Part::Op
                        && edit
                            .parameters
                            .iter()
                            .any(|v| !crate::analysis::number(v.trim()).is_some_and(|n| n > 0.))
                    {
                        Some("Enter a positive value for each parameter.")
                    } else if edit.parasitics.iter().any(|v| {
                        !v.trim().is_empty()
                            && !crate::analysis::number(v.trim()).is_some_and(|n| n >= 0.)
                    }) {
                        Some("Enter nonnegative parasitic values or leave them blank.")
                    } else if edit.value.trim().is_empty() {
                        Some("Enter a value.")
                    } else {
                        None
                    };
                    if let Some(msg) = error {
                        ui.label(msg);
                    }
                    crate::ui::dialog_actions(ui, "Apply", error.is_none(), true)
                },
            );
            if dialog.inner {
                if let Some(c) = cards(text).into_iter().find(|c| c.name == edit.original) {
                    if edit.part.spec().0 == 'X' && edit.part != Part::Diode {
                        let model = edit.part.spec().2.split_whitespace().next().unwrap();
                        let mut fields = vec![model.to_owned()];
                        for (key, value) in
                            c.value.split_whitespace().filter_map(|s| s.split_once('='))
                        {
                            if !edit
                                .part
                                .parameters()
                                .iter()
                                .any(|(k, _)| k.eq_ignore_ascii_case(key))
                            {
                                fields.push(format!("{key}={value}"));
                            }
                        }
                        for ((key, _), value) in edit.part.parameters().iter().zip(&edit.parameters)
                        {
                            fields.push(format!("{key}={}", value.trim()));
                        }
                        edit.value = fields.join(" ");
                    }
                    let line = c.line;
                    let parasitics = parasitic_fields(edit.part)
                        .iter()
                        .zip(&edit.parasitics)
                        .filter(|(_, value)| {
                            crate::analysis::number(value.trim()).is_some_and(|n| n > 0.)
                        })
                        .map(|((key, _), value)| format!("{key}={}", value.trim()))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let mut replacement = format!(
                        "{} {} {}",
                        edit.reference,
                        c.nodes.join(" "),
                        edit.value.trim()
                    );
                    if !parasitics.is_empty() {
                        replacement.push_str(PARASITICS_MARKER);
                        replacement.push_str(&parasitics);
                    }
                    *text = text
                        .lines()
                        .enumerate()
                        .map(|(i, l)| if i == line { replacement.as_str() } else { l })
                        .collect::<Vec<_>>()
                        .join("\n")
                        + "\n";
                    if let Some(pos) = self.positions.remove(&edit.original) {
                        self.positions.insert(edit.reference.clone(), pos);
                    }
                    if let Some(turns) = self.rotations.remove(&edit.original) {
                        self.rotations.insert(edit.reference.clone(), turns);
                    }
                    if self.selected.remove(&Object::Part(edit.original)) {
                        self.selected.insert(Object::Part(edit.reference));
                    }
                    if edit.part.spec().0 == 'X' {
                        ensure_model(text, &edit.value);
                    }
                    self.index_parts(text);
                    self.label_layout = None;
                    self.snapshot = text.clone();
                    return true;
                }
            } else if !dialog.should_close() {
                self.editing = Some(edit);
            }
        }
        if changed {
            self.sync(text);
        }
        changed
    }
}
fn format_operating_value(value: f64, current: bool) -> String {
    let magnitude = value.abs();
    let (scale, prefix) = if magnitude == 0. || (1. ..1000.).contains(&magnitude) {
        (1., "")
    } else if (1000. ..1e6).contains(&magnitude) {
        (1e-3, "k")
    } else if (1e-3..1.).contains(&magnitude) {
        (1e3, "m")
    } else if (1e-6..1e-3).contains(&magnitude) {
        (1e6, "µ")
    } else if (1e-9..1e-6).contains(&magnitude) {
        (1e9, "n")
    } else {
        return format!("{value:.3e} {}", if current { "A" } else { "V" });
    };
    let scaled = format!("{:.3}", value * scale);
    format!(
        "{} {prefix}{}",
        scaled.trim_end_matches('0').trim_end_matches('.'),
        if current { "A" } else { "V" }
    )
}

fn rotate_power(rotations: &mut HashMap<Point, u8>, pin: Point) {
    let turns = (rotations.get(&pin).copied().unwrap_or(0) + 1) % 4;
    if turns == 0 {
        rotations.remove(&pin);
    } else {
        rotations.insert(pin, turns);
    }
}

fn power_direction(turns: u8) -> Point {
    [(0, -1), (1, 0), (0, 1), (-1, 0)][usize::from(turns % 4)]
}

fn power_arrow(painter: &egui::Painter, pin: Pos2, scale: f32, turns: u8, color: Color32) {
    let (dx, dy) = power_direction(turns);
    let forward = vec2(dx as f32, dy as f32);
    let side = vec2(-forward.y, forward.x);
    let pen = Stroke::new(SCHEMATIC_STROKE, color);
    for points in [
        &[(0., 0.), (0., -16.)][..],
        &[(-6., -9.), (0., -16.), (6., -9.)],
    ] {
        painter.line(
            points
                .iter()
                .map(|&(x, y)| pin + (side * x - forward * y) * scale)
                .collect(),
            pen,
        );
    }
}

fn probe_cursor(ui: &Ui, tip: Pos2, current: bool) {
    ui.ctx().set_cursor_icon(egui::CursorIcon::None);
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::unique("schematic probe cursor"),
    ));
    let ink = Color32::from_gray(35);
    let path = |points: &[(f32, f32)], width| {
        painter.line(
            points.iter().map(|&(x, y)| tip + vec2(x, y)).collect(),
            Stroke::new(width, ink),
        );
    };
    let fill = |points: &[(f32, f32)]| {
        painter.add(egui::Shape::convex_polygon(
            points.iter().map(|&(x, y)| tip + vec2(x, y)).collect(),
            ink,
            Stroke::NONE,
        ));
    };
    if current {
        // The center of the jaws is the measurement point.
        for side in [-1., 1.] {
            let jaw: Vec<_> = (0..=12)
                .map(|i| {
                    let angle = (18. + 162. * i as f32 / 12.).to_radians();
                    (side * 7. * angle.sin(), -7. * angle.cos())
                })
                .collect();
            path(&jaw, 2.4);
        }
        let red = Color32::from_rgb(232, 103, 105);
        painter.add(egui::Shape::convex_polygon(
            [(-4., 8.), (-9., 12.), (-4., 14.)]
                .map(|(x, y)| tip + vec2(x, y))
                .to_vec(),
            red,
            Stroke::new(1.6, ink),
        ));
        painter.rect(
            egui::Rect::from_min_max(tip + vec2(-5., 5.), tip + vec2(5., 19.)),
            2,
            red,
            Stroke::new(2., ink),
            egui::StrokeKind::Middle,
        );
        path(&[(-2., 12.), (2., 12.)], 1.5);
    } else {
        // The pointed end of the voltage probe is the measurement point.
        fill(&[(0., 0.), (2.5, -5.5), (5.5, -2.5)]);
        path(&[(5., -5.), (12., -12.)], 3.);
        let grip = [tip + vec2(14., -14.), tip + vec2(24., -24.)];
        path(&[(14., -14.), (24., -24.)], 8.);
        for end in grip {
            painter.circle_filled(end, 4., ink);
        }
        let accent = Color32::from_rgb(112, 192, 211);
        painter.line(grip.to_vec(), Stroke::new(4.5, accent));
        for end in grip {
            painter.circle_filled(end, 2.25, accent);
        }

        let cable =
            [(25., -25.), (38., -38.), (39., -22.), (33., -16.)].map(|(x, y)| tip + vec2(x, y));
        painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
            cable,
            false,
            Color32::TRANSPARENT,
            Stroke::new(2.4, ink),
        ));
        painter.circle_filled(cable[3], 1.2, ink);
    }
}

fn symbol_bounds(part: Part, center: Pos2, turns: u8, scale: f32) -> egui::Rect {
    let size = if part.layout().len() > 2 {
        vec2(2. * PIN_OFFSET + 5., 2. * PIN_OFFSET + 5.)
    } else if turns.is_multiple_of(2) {
        vec2(2. * PIN_OFFSET, 36.)
    } else {
        vec2(36., 2. * PIN_OFFSET)
    };
    egui::Rect::from_center_size(center, size * scale).expand(SCHEMATIC_STROKE / 2.)
}

fn draw_labels(
    p: &egui::Painter,
    center: Pos2,
    card: &Card<'_>,
    scale: f32,
    wires: &[[Pos2; 2]],
    occupied: &mut Vec<egui::Rect>,
) -> egui::Rect {
    let (name, value) = label_text(p, card, scale);
    let rect = place_label(
        center,
        scale,
        label_size(&name, &value),
        12.,
        wires,
        occupied,
        egui::Rect::EVERYTHING,
    )
    .unwrap();
    paint_label(p, rect.min, name, value);
    rect
}

// Native egui translates clipboard shortcuts before widgets see the key.
fn consume_clipboard_event(input: &mut egui::InputState, event: egui::Event) -> bool {
    let Some(index) = input
        .events
        .iter()
        .position(|candidate| *candidate == event)
    else {
        return false;
    };
    input.events.remove(index);
    true
}

fn label_text(
    p: &egui::Painter,
    card: &Card<'_>,
    scale: f32,
) -> (std::sync::Arc<egui::Galley>, std::sync::Arc<egui::Galley>) {
    let value = split_parasitics(card.part, card.value).0;
    let value = value
        .split_once('(')
        .map_or(value, |(function, _)| function);
    let value = if card.part == Part::Diode {
        diode_type(card.value).1
    } else if card.part.spec().0 == 'X' {
        card.part.spec().1
    } else {
        value
    };
    let name = p.layout_no_wrap(card.name.into(), FontId::proportional(13. * scale), LABEL);
    let value = p.layout_no_wrap(value.into(), FontId::proportional(11. * scale), LABEL);
    (name, value)
}

fn label_size(name: &egui::Galley, value: &egui::Galley) -> egui::Vec2 {
    vec2(
        name.size().x.max(value.size().x),
        name.size().y + value.size().y,
    )
}

fn paint_label(
    p: &egui::Painter,
    pos: Pos2,
    name: std::sync::Arc<egui::Galley>,
    value: std::sync::Arc<egui::Galley>,
) {
    let value_pos = pos + vec2(0., name.size().y);
    p.galley(pos, name, LABEL);
    p.galley(value_pos, value, LABEL);
}

fn place_label(
    center: Pos2,
    scale: f32,
    size: egui::Vec2,
    clearance: f32,
    wires: &[[Pos2; 2]],
    occupied: &mut Vec<egui::Rect>,
    viewport: egui::Rect,
) -> Option<egui::Rect> {
    let gap = 3. * scale + 2.;
    for ring in 0.. {
        if viewport.is_finite() && ring > 8 {
            return None;
        }
        let offset = gap + ring as f32 * (8. * scale).max(2.);
        for dir in [
            vec2(0., -1.),
            vec2(1., 0.),
            vec2(0., 1.),
            vec2(-1., 0.),
            vec2(1., -1.),
            vec2(-1., -1.),
            vec2(1., 1.),
            vec2(-1., 1.),
        ] {
            let rect = egui::Rect::from_center_size(
                center + dir * (size * 0.5 + egui::Vec2::splat(clearance * scale + offset)),
                size,
            );
            let padded = rect.expand(2.);
            if !viewport.contains_rect(padded)
                || occupied.iter().any(|r| r.intersects(padded))
                || wires.iter().any(|&[a, b]| {
                    padded.contains(a)
                        || padded.contains(b)
                        || (a != b
                            && egui::Rect::from_two_pos(a, b).intersects(padded)
                            && padded.intersects_ray(a, (b - a).normalized())
                            && padded.intersects_ray(b, (a - b).normalized()))
                })
            {
                continue;
            }
            occupied.push(padded);
            return Some(rect);
        }
    }
    unreachable!()
}

fn draw_open_pin(p: &egui::Painter, pin: Pos2, scale: f32, color: Color32) {
    let radius = 2. * scale;
    let width = SCHEMATIC_STROKE.min(radius);
    p.rect(
        egui::Rect::from_center_size(pin, vec2(radius * 2., radius * 2.)),
        0,
        PAPER,
        Stroke::new(width, color),
        egui::StrokeKind::Middle,
    );
}

fn draw(
    p: &egui::Painter,
    center: Pos2,
    (part, value): (Part, &str),
    turns: u8,
    scale: f32,
    american: bool,
    color: Color32,
) {
    let transform = |x: f32, y: f32| {
        let (u, v) = match turns % 4 {
            0 => (x, y),
            1 => (-y, x),
            2 => (-x, -y),
            _ => (y, -x),
        };
        center + vec2(u, v) * scale
    };
    let pen = Stroke::new(SCHEMATIC_STROKE, color);
    let stroke_path = |coords: &[(f32, f32)], stroke: Stroke| {
        p.add(egui::Shape::line(
            coords.iter().map(|&(x, y)| transform(x, y)).collect(),
            stroke,
        ));
    };
    let path = |coords: &[(f32, f32)]| stroke_path(coords, pen);
    let heavy = Stroke::new(SCHEMATIC_STROKE + 0.9 * scale.min(1.5), color);
    let body = |coords: &[(f32, f32)]| stroke_path(coords, heavy);
    let filled = |coords: &[(f32, f32)]| {
        p.add(egui::Shape::convex_polygon(
            coords.iter().map(|&(x, y)| transform(x, y)).collect(),
            color,
            Stroke::NONE,
        ));
    };
    let half = match part {
        Part::Resistor if american => {
            path(&[
                (-21., 0.),
                (-17.5, -7.),
                (-10.5, 7.),
                (-3.5, -7.),
                (3.5, 7.),
                (10.5, -7.),
                (17.5, 7.),
                (21., 0.),
            ]);
            21.
        }
        Part::Resistor => {
            p.add(egui::Shape::convex_polygon(
                [(-16., -6.), (16., -6.), (16., 6.), (-16., 6.)]
                    .into_iter()
                    .map(|(x, y)| transform(x, y))
                    .collect(),
                PAPER,
                pen,
            ));
            16.
        }
        Part::Capacitor => {
            for x in [-4., 4.] {
                body(&[(x, -11.), (x, 11.)]);
            }
            4.
        }
        Part::Inductor => {
            for i in 0..4 {
                let arc: Vec<_> = (0..=12)
                    .map(|j| {
                        let angle = j as f32 * std::f32::consts::PI / 12.;
                        (-15. + i as f32 * 10. - 5. * angle.cos(), -5. * angle.sin())
                    })
                    .collect();
                path(&arc);
            }
            20.
        }
        Part::Diode => {
            filled(&[(-9., -9.), (8., 0.), (-9., 9.)]);
            match diode_type(value).0 {
                "os_zener" => body(&[(4., -6.), (8., -10.), (8., 10.), (12., 6.)]),
                "os_schottky" => body(&[
                    (12., -6.),
                    (12., -10.),
                    (8., -10.),
                    (8., 10.),
                    (4., 10.),
                    (4., 6.),
                ]),
                "os_led" => {
                    body(&[(8., -10.), (8., 10.)]);
                    for (x, y) in [(-5., -10.), (2., -7.)] {
                        path(&[(x, y), (x + 7., y - 7.)]);
                        path(&[(x + 3., y - 7.), (x + 7., y - 7.), (x + 7., y - 3.)]);
                    }
                }
                _ => body(&[(8., -10.), (8., 10.)]),
            }
            8.
        }
        Part::Polarized => {
            for x in [-4., 4.] {
                body(&[(x, -11.), (x, 11.)]);
            }
            path(&[(-15., -8.), (-9., -8.)]);
            path(&[(-12., -11.), (-12., -5.)]);
            4.
        }
        Part::Op | Part::OpRail | Part::Pole => {
            const BACK: f32 = -17.;
            const TIP: f32 = 19.;
            const HEIGHT: f32 = 20.;
            body(&[(BACK, -HEIGHT), (TIP, 0.), (BACK, HEIGHT), (BACK, -HEIGHT)]);
            for y in [-14., 14.] {
                path(&[(-PIN_OFFSET, y), (BACK, y)]);
            }
            for y in [-9., 9.] {
                body(&[(-12., y), (-6., y)]);
            }
            body(&[(-9., -12.), (-9., -6.)]);
            path(&[(TIP, 0.), (PIN_OFFSET, 0.)]);
            if matches!(part, Part::OpRail | Part::Pole) {
                let shoulder = HEIGHT * TIP / (TIP - BACK);
                for sign in [-1., 1.] {
                    path(&[(0., sign * shoulder), (0., sign * PIN_OFFSET)]);
                }
            }
            return;
        }
        Part::Transformer => {
            for sign in [-1., 1.] {
                path(&[(sign * 28., -14.), (sign * 11., -14.)]);
                path(&[(sign * 11., 14.), (sign * 28., 14.)]);
                let coil: Vec<_> = (0..4)
                    .flat_map(|turn| {
                        (0..=10).map(move |i| {
                            let angle = i as f32 * std::f32::consts::PI / 10.;
                            (
                                sign * (11. - 5. * angle.sin()),
                                -14. + 7. * turn as f32 - 3.5 * angle.cos() + 3.5,
                            )
                        })
                    })
                    .collect();
                path(&coil);
                body(&[(sign * 2.5, -15.), (sign * 2.5, 15.)]);
            }
            return;
        }
        Part::Npn | Part::Pnp | Part::Nmos | Part::Pmos | Part::Njfet | Part::Pjfet => {
            let arrow = |tip: (f32, f32), tail: (f32, f32)| {
                let d = vec2(tip.0 - tail.0, tip.1 - tail.1).normalized();
                filled(&[
                    (tail.0 - d.y * 3., tail.1 + d.x * 3.),
                    tip,
                    (tail.0 + d.y * 3., tail.1 - d.x * 3.),
                ]);
            };
            p.circle_stroke(transform(3., 0.), 20. * scale, pen);
            match part {
                Part::Npn | Part::Pnp => {
                    path(&[(-28., 0.), (-5., 0.)]);
                    body(&[(-5., -13.), (-5., 13.)]);
                    for sign in [-1., 1.] {
                        path(&[(-5., sign * 5.), (14., sign * 16.), (14., sign * 28.)]);
                    }
                    if part == Part::Npn {
                        arrow((13., 15.4), (6., 11.4));
                    } else {
                        arrow((-3., 6.2), (4., 10.2));
                    }
                }
                Part::Nmos | Part::Pmos => {
                    path(&[(-28., 0.), (-7., 0.)]);
                    body(&[(-7., -12.), (-7., 12.)]);
                    for y in [-11., 0., 11.] {
                        body(&[(-2., y - 3.5), (-2., y + 3.5)]);
                    }
                    path(&[(-2., -11.), (14., -11.), (14., -28.)]);
                    path(&[(-2., 11.), (14., 11.), (14., 28.)]);
                    path(&[(-2., 0.), (14., 0.), (14., 11.)]);
                    if part == Part::Nmos {
                        arrow((0., 0.), (6., 0.));
                    } else {
                        arrow((10., 0.), (4., 0.));
                    }
                    p.circle_filled(transform(14., 11.), 1.5 * scale, color);
                }
                _ => {
                    for sign in [-1., 1.] {
                        path(&[(14., 28. * sign), (14., 12. * sign), (-2., 12. * sign)]);
                    }
                    body(&[(-2., -12.), (-2., 12.)]);
                    path(&[(-28., 0.), (-2., 0.)]);
                    if part == Part::Njfet {
                        arrow((-2., 0.), (-9., 0.));
                    } else {
                        arrow((-10., 0.), (-3., 0.));
                    }
                }
            }
            return;
        }
        Part::Current => {
            p.circle_stroke(center, 14. * scale, pen);
            path(&[(-9., 0.), (8., 0.)]);
            path(&[(2., -4.), (8., 0.), (2., 4.)]);
            14.
        }
        Part::Voltage => {
            use crate::source::Kind;
            let kind = Kind::detect(value);
            if kind == Kind::Dc {
                body(&[(-3., -11.), (-3., 11.)]);
                stroke_path(
                    &[(3., -6.), (3., 6.)],
                    Stroke::new(SCHEMATIC_STROKE + 2.4 * scale.min(1.5), color),
                );
                path(&[(-13., -8.), (-7., -8.)]);
                path(&[(-10., -11.), (-10., -5.)]);
                3.
            } else {
                p.circle(center, 14. * scale, Color32::from_rgb(255, 250, 194), pen);
                path(&[(-16., -13.), (-10., -13.)]);
                path(&[(-13., -16.), (-13., -10.)]);
                // Keep the waveform upright when the source rotates.
                let path = |coords: &[(f32, f32)]| {
                    p.add(egui::Shape::line(
                        coords
                            .iter()
                            .map(|&(x, y)| center + vec2(x, y - 4.) * scale)
                            .collect(),
                        pen,
                    ));
                };
                match kind {
                    Kind::Sine => {
                        let wave: Vec<_> = (0..=24)
                            .map(|i| {
                                let t = i as f32 / 24.;
                                (-8. + 16. * t, 4. - 3. * (t * std::f32::consts::TAU).sin())
                            })
                            .collect();
                        path(&wave);
                    }
                    Kind::Pulse => {
                        path(&[(-8., 4.), (-8., 1.), (0., 1.), (0., 7.), (8., 7.), (8., 4.)])
                    }
                    Kind::Exp => path(&[(-8., 7.), (-6., 4.), (-4., 2.), (0., 1.), (7., 1.)]),
                    _ => path(&[(-4., 4.), (4., 4.)]),
                }
                14.
            }
        }
    };
    for side in [-1., 1.] {
        path(&[(side * half, 0.), (side * PIN_OFFSET, 0.)]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires ngspice on PATH"]
    fn preset_power_rails_drive_separate_nets_and_persist() {
        let mut text = "Power rails\nR1 a 0 1k\nR2 b 0 1k\nR3 c 0 1k\n.op\n.end\n".to_owned();
        let mut schematic = Schematic::default();
        schematic.import(&text);
        for (name, voltage) in [("R1", 3.3), ("R2", 5.), ("R3", 3.3)] {
            let pin = Part::Resistor.terminals(schematic.positions[name], 0)[0];
            schematic.powers.push(pin);
            schematic.power_voltages.insert(pin, voltage);
        }
        schematic.sync(&mut text);
        assert!(text.contains("R1 os_vcc_3p3 0 1k"));
        assert!(text.contains("R2 os_vcc_5 0 1k"));
        assert!(text.contains("R3 os_vcc_3p3 0 1k"));
        assert_eq!(
            text.lines()
                .filter(|line| line.starts_with(RAIL_SOURCE))
                .count(),
            2
        );
        let run = crate::simulation::simulate(&text);
        let wave = run.waveform.expect(&run.log);
        for (node, voltage) in [("V(os_vcc_3p3)", 3.3), ("V(os_vcc_5)", 5.)] {
            let trace = wave
                .traces
                .iter()
                .find(|trace| trace.name.eq_ignore_ascii_case(node))
                .unwrap();
            assert!((trace.samples[0] - voltage).abs() < 1e-8);
        }
        let saved = ron::to_string(&schematic.geometry()).unwrap();
        let geometry: Geometry = ron::from_str(&saved).unwrap();
        assert!(Schematic::matches_document(&geometry, &text));
        assert!(geometry == schematic.geometry());
    }

    #[test]
    #[ignore = "requires ngspice on PATH"]
    fn diode_types_and_supply_limits() {
        let mut deck = String::from("Types and rails\nVhi hi 0 5\nVlo lo 0 -2\n");
        for (i, (model, _)) in DIODES.iter().enumerate() {
            deck.push_str(&format!("I{i} 0 d{i} 1m\nX{i} d{i} 0 {model}\n"));
        }
        deck.push_str("Iz 0 reverse 1m\nXz 0 reverse os_zener\nXp hi 0 pos hi lo os_pole_ext\nXn lo 0 neg hi lo os_pole_ext\nXa lo 0 adj os_op_adjust vpos=8 vneg=-3\n.op\n.end\n");
        for model in DIODES
            .iter()
            .map(|d| d.0)
            .chain(["os_pole_ext", "os_op_adjust"])
        {
            ensure_model(&mut deck, model);
        }
        let run = crate::simulation::simulate(&deck);
        let wave = run.waveform.expect(&run.log);
        let v = |name: &str| wave.traces.iter().find(|t| t.name == name).unwrap().samples[0];
        assert!(v("V(d2)") < v("V(d0)") && v("V(d0)") < v("V(d4)"));
        assert!((v("V(reverse)") - 5.1).abs() < 0.05);
        for (node, voltage) in [("V(pos)", 5.), ("V(neg)", -2.), ("V(adj)", -3.)] {
            assert!((v(node) - voltage).abs() < 1e-5);
        }
    }

    #[test]
    fn legacy_pole_gains_external_pins_once() {
        let mut deck = "Legacy\nX1 in 0 out os_pole gain=123\n.end\n".to_owned();
        upgrade_pole_pins(&mut deck);
        let parts = cards(&deck);
        assert_eq!(parts.len(), 1);
        assert_eq!(&parts[0].nodes[..3], &["in", "0", "out"]);
        assert_eq!(parts[0].nodes.len(), 5);
        assert_ne!(parts[0].nodes[3], parts[0].nodes[4]);
        assert!(parts[0].value.contains("biasp=15 biasn=-15 gain=123"));
        let first = deck.clone();
        upgrade_pole_pins(&mut deck);
        assert_eq!(deck, first);
    }

    #[test]
    fn wire_preview_marks_new_junctions_without_marking_crossings() {
        let schematic = Schematic {
            wires: vec![((0, 0), (8, 0))],
            ..Default::default()
        };
        assert_eq!(
            schematic.preview_junctions(&[((4, 3), (4, 0))]),
            vec![(4, 0)]
        );
        assert!(schematic.preview_junctions(&[((4, 3), (4, -3))]).is_empty());
        assert!(schematic.preview_junctions(&[((4, 0), (6, 0))]).is_empty());
    }

    #[test]
    #[ignore = "requires ngspice on PATH"]
    fn current_source_survives_connectivity_rebuild_and_drives_resistor() {
        let mut text = String::from("Current source test\nI1 0 out 1m\nR1 out 0 1k\n.op\n.end\n");
        let mut schematic = Schematic::default();
        schematic.import(&text);
        schematic.sync(&mut text);
        assert!(text.contains("I1 0 out 1m"));
        let result = crate::simulation::simulate(&text);
        let wave = result.waveform.expect(&result.log);
        assert!(
            (wave
                .traces
                .iter()
                .find(|t| t.name == "V(out)")
                .unwrap()
                .samples[0]
                - 1.)
                .abs()
                < 1e-10
        );
    }
    #[test]
    fn drag_resistor_stretches_both_original_wires() {
        let mut s = Schematic::default();
        s.import(include_str!("../examples/rc-low-pass.cir"));
        let [left, right] = pins(s.positions["R1"], 0);
        let source = pins(s.positions["V1"], 0)[0];
        let capacitor = pins(s.positions["C1"], 0)[0];
        let dy = source.1 - left.1;
        s.selected.insert(Object::Part("R1".into()));
        s.move_selection((0, dy));
        assert!(s.drag_leads.is_empty());
        assert!(s.wires.contains(&(source, (left.0, source.1))));
        assert!(s.wires.contains(&((right.0, right.1 + dy), capacitor)));
    }

    #[test]
    fn route_through_resistor_then_drag_component_and_ground() {
        let mut text = "branch
R1 in 0 1k
.end
"
        .to_owned();
        let mut s = Schematic::default();
        s.import(&text);
        let [left, right] = pins(s.positions["R1"], 0);
        s.route(((left.0 - 3, left.1), (right.0 + 3, right.1)));
        s.sync(&mut text);
        assert_eq!(s.wires.len(), 2);
        assert_eq!(cards(&text)[0].nodes, ["in", "0"]);
        s.selected.insert(Object::Part("R1".into()));
        s.move_selection((0, 2));
        s.move_selection((0, 1));
        assert_eq!(s.grounds, [right]);
        assert_eq!(s.drag_leads.len(), 1);
        s.wires.append(&mut s.drag_leads);
        s.sync(&mut text);
        assert_eq!(cards(&text)[0].nodes, ["in", "0"]);
        s.selected.clear();
        s.selected.insert(Object::Ground(0));
        s.move_selection((1, 2));
        s.wires.append(&mut s.drag_leads);
        s.sync(&mut text);
        assert_eq!(cards(&text)[0].nodes, ["in", "0"]);
    }

    #[test]
    fn delete_mixed_selection_and_hovered_wire() {
        let mut text = include_str!("../examples/rc-low-pass.cir").to_owned();
        let mut s = Schematic::default();
        s.import(&text);
        s.wires
            .extend([((90, 90), (100, 90)), ((90, 90), (90, 100))]);
        let selected = s.wires.len() - 2;
        s.selected
            .extend([Object::Part("R1".into()), Object::Wire(selected)]);
        s.normalize();
        let remaining = ((90, 90), (90, 100));
        let hovered = Object::Wire(s.wires.iter().position(|w| *w == remaining).unwrap());
        assert!(s.delete_objects(&mut text, Some(hovered)));
        assert!(!s.positions.contains_key("R1"));
        assert!(!s.wires.contains(&((90, 90), (100, 90))));
        let i = s.wires.iter().position(|w| *w == remaining).unwrap();
        assert!(s.delete_objects(&mut text, Some(Object::Wire(i))));
        assert!(!s.wires.contains(&remaining));
    }

    #[test]
    fn delete_hovered_or_selected_parts() {
        let mut text = include_str!("../examples/rc-low-pass.cir").to_owned();
        let mut s = Schematic::default();
        s.import(&text);
        let hovered = s.symbol_at(s.positions["V1"]);
        s.selected
            .extend(["R1", "C1"].map(|n| Object::Part(n.into())));
        assert!(s.delete_objects(&mut text, hovered.clone()));
        assert_eq!(cards(&text).len(), 1);
        assert_eq!(cards(&text)[0].name, "V1");
        assert!(s.delete_objects(&mut text, hovered));
        assert!(cards(&text).is_empty() && s.positions.is_empty());
        assert!(text.contains(".tran") && text.contains(".end"));
    }

    #[test]
    fn wheel_and_pinch_follow_waveform_zoom_and_reuse_the_grid() {
        let ctx = egui::Context::default();
        let mut schematic = Schematic::default();
        let mut text = crate::simulation::EXAMPLE.to_owned();
        let cursor = egui::pos2(420., 220.);
        let mut expected = 1_f64;
        let mut tiles = Vec::new();
        let mut anchor = None;
        for delta in [0., 6., 14., -9., -11., 0.] {
            let mut events = vec![egui::Event::PointerMoved(cursor)];
            if delta != 0. {
                events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: vec2(0., delta),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                    modifiers: Default::default(),
                });
            } else if anchor.is_some() {
                events.push(egui::Event::Zoom(1.2));
            }
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(800., 500.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    expected *= ui.input(crate::ui::canvas_zoom_factor);
                    let origin = ui.cursor().min;
                    schematic.show(ui, &mut text, None);
                    let scale = 1.25_f32.powf(schematic.zoom_steps);
                    assert!((scale - expected as f32).abs() < 1e-5);
                    let world = (cursor - origin - schematic.pan) / scale;
                    assert!((world - *anchor.get_or_insert(world)).length() < 0.001);
                },
            );
            if tiles.is_empty() {
                tiles.extend(schematic.grid_tiles.iter().map(|t| t.id()));
            } else {
                assert!(
                    output
                        .textures_delta
                        .set
                        .iter()
                        .all(|(id, _)| !tiles.contains(id))
                );
                assert!(
                    output
                        .textures_delta
                        .free
                        .iter()
                        .all(|id| !tiles.contains(id))
                );
            }
            output.textures_delta.clear();
        }
        assert!(expected > 1.1);
    }

    #[test]
    fn floating_pins_use_squares_on_passives_and_op_amps() {
        let ctx = egui::Context::default();
        let mut schematic = Schematic::default();
        let mut deck = "Markers\nR1 a b 1k\nX1 b c d os_op_adjust\n.end\n".to_owned();
        schematic.import(&deck);
        for ground_output in [false, true] {
            if ground_output {
                let output = Part::Op.terminals(schematic.positions["X1"], 0)[2];
                schematic.grounds.push(output);
            }
            let mut frame = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(800., 600.))),
                    ..Default::default()
                },
                |ui| {
                    schematic.show(ui, &mut deck, None);
                },
            );
            let squares = frame.shapes.iter().filter(|shape| matches!(&shape.shape,
                egui::Shape::Rect(r) if r.fill == PAPER && r.stroke.color == INK && r.rect.size() == vec2(4.,4.)
            )).count();
            frame.textures_delta.clear();
            assert_eq!(squares, if ground_output { 2 } else { 3 });
        }
    }

    #[test]
    fn rotate_connected_voltage_source_and_zoom_about_cursor() {
        let mut text = include_str!("../examples/rc-low-pass.cir").to_owned();
        let mut schematic = Schematic::default();
        schematic.import(&text);
        schematic.selected.insert(Object::Part("V1".into()));
        let center = schematic.positions["V1"];
        for turn in 1..=4 {
            assert!(schematic.rotate());
            schematic.sync(&mut text);
            assert_eq!(cards(&text)[0].nodes, ["in", "0"]);
            assert!(schematic.grounds.contains(&pins(center, turn)[1]));
        }
        let cursor = vec2(120., 75.);
        let before = (cursor - schematic.pan) / GRID;
        schematic.zoom(2., Some(cursor));
        let after = (cursor - schematic.pan) / (GRID * 1.25_f32.powf(schematic.zoom_steps));
        assert!((before - after).length() < 0.001);
    }

    #[test]
    fn ground_branch_and_cut_update_pin_nodes() {
        let mut text = "junction test\nR1 a b 1k\nR2 c d 2k\n.end\n".to_owned();
        let mut s = Schematic::default();
        s.import(&text);
        s.positions.insert("R1".into(), (2, 2));
        s.positions.insert("R2".into(), (6, 6));
        s.wires = vec![((4, 2), (4, 8)), ((4, 4), (8, 4))];
        s.grounds.push((8, 4));
        s.sync(&mut text);
        assert_eq!(cards(&text)[0].nodes[1], "0");
        assert_eq!(cards(&text)[1].nodes[0], "0");
        assert!(s.wires.contains(&((4, 2), (4, 4))));
        s.wires.retain(|&w| w != ((4, 2), (4, 4)));
        s.sync(&mut text);
        assert_ne!(cards(&text)[0].nodes[1], "0");
        assert_eq!(cards(&text)[1].nodes[0], "0");
    }
}
