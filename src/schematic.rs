use eframe::egui::{self, Align2, Color32, FontId, Pos2, Stroke, Ui, vec2};
use std::collections::{HashMap, HashSet};

const SCHEMATIC_STROKE: f32 = 1.6;
pub(crate) const PAPER: Color32 = Color32::from_rgb(29, 32, 37);
const WIRE: Color32 = Color32::from_rgb(118, 199, 155);
const INK: Color32 = Color32::from_rgb(233, 210, 181);
const LABEL: Color32 = Color32::from_rgb(149, 210, 222);
const ACCENT: Color32 = Color32::from_rgb(101, 177, 255);
pub(crate) const GRID: f32 = 14.;
const PIN_OFFSET: f32 = 2. * GRID;
const WINDING_HALF_SPAN: i32 = 2;

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
    CenterTappedTransformer,
    CustomTransformer { inputs: u8, outputs: u8 },
}

const MAX_TRANSFORMER_WINDINGS: u8 = 8;

fn winding_centers(count: u8, half_pitch: i32) -> impl Iterator<Item = i32> {
    (0..i32::from(count)).map(move |index| half_pitch * (2 * index - i32::from(count) + 1))
}

// The low two bits keep the saved quarter turns. Bit 2 reflects the
// rotated symbol left-to-right in schematic coordinates.
const MIRRORED: u8 = 4;

fn orient_offset<T: std::ops::Neg<Output = T>>((x, y): (T, T), orientation: u8) -> (T, T) {
    let (x, y) = match orientation % 4 {
        0 => (x, y),
        1 => (-y, x),
        2 => (-x, -y),
        _ => (y, -x),
    };
    (if orientation & MIRRORED != 0 { -x } else { x }, y)
}

#[derive(Clone, Copy)]
enum OrientationEdit {
    Rotate,
    MirrorHorizontal,
    MirrorVertical,
}

impl OrientationEdit {
    fn apply(self, orientation: u8) -> u8 {
        match self {
            Self::Rotate => {
                let mirror = orientation & MIRRORED;
                let step = if mirror == 0 { 1 } else { 3 };
                mirror | ((orientation % 4 + step) % 4)
            }
            Self::MirrorHorizontal => orientation ^ MIRRORED,
            Self::MirrorVertical => orientation ^ (MIRRORED | 2),
        }
    }
}

const DIODES: [(&str, &str); 6] = [
    ("os_diode_standard", "Standard diode"),
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
                .is_some_and(|v| v.trim_end_matches("_adjust").eq_ignore_ascii_case(model))
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

    const LIBRARY: [Self; 14] = [
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
        Self::CenterTappedTransformer,
        Self::CustomTransformer {
            inputs: 1,
            outputs: 2,
        },
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
            Self::Transformer => &[
                (-2, -WINDING_HALF_SPAN, ""),
                (-2, WINDING_HALF_SPAN, ""),
                (2, -WINDING_HALF_SPAN, ""),
                (2, WINDING_HALF_SPAN, ""),
            ],
            Self::CenterTappedTransformer => &[
                (-2, -WINDING_HALF_SPAN, ""),
                (-2, WINDING_HALF_SPAN, ""),
                (2, -2 * WINDING_HALF_SPAN, ""),
                (2, 0, ""),
                (2, 2 * WINDING_HALF_SPAN, ""),
            ],
            _ => &[(-2, 0, ""), (2, 0, "")],
        }
    }
    fn transformer_windings(self) -> Option<(u8, u8)> {
        match self {
            Self::Transformer | Self::CenterTappedTransformer => Some((1, 1)),
            Self::CustomTransformer { inputs, outputs } => Some((inputs, outputs)),
            _ => None,
        }
    }

    fn pin_count(self) -> usize {
        match self {
            Self::CustomTransformer { inputs, outputs } => {
                2 * (usize::from(inputs) + usize::from(outputs))
            }
            _ => self.layout().len(),
        }
    }

    fn custom_transformer(model: &str) -> Option<Self> {
        const PREFIX: &str = "os_xfmr_multi_";
        let (prefix, windings) = model.split_at_checked(PREFIX.len())?;
        if !prefix.eq_ignore_ascii_case(PREFIX) {
            return None;
        }
        let (inputs, outputs) = windings.split_once('_')?;
        let (inputs, outputs) = (inputs.parse().ok()?, outputs.parse().ok()?);
        let range = 1..=MAX_TRANSFORMER_WINDINGS;
        (range.contains(&inputs) && range.contains(&outputs))
            .then_some(Self::CustomTransformer { inputs, outputs })
    }

    fn terminals(self, center: Point, turns: u8) -> Vec<Point> {
        let rotate = |offset| {
            let (x, y) = orient_offset(offset, turns);
            (center.0 + x, center.1 + y)
        };
        if let Self::CustomTransformer { inputs, outputs } = self {
            return [(-2, inputs), (2, outputs)]
                .into_iter()
                .flat_map(|(x, count)| {
                    winding_centers(count, 3)
                        .flat_map(move |y| [(x, y - WINDING_HALF_SPAN), (x, y + WINDING_HALF_SPAN)])
                })
                .map(rotate)
                .collect();
        }
        self.layout()
            .iter()
            .map(|&(x, y, _)| rotate((x, y)))
            .collect()
    }
    fn adjustable_transistor_model(self) -> Option<&'static str> {
        match self {
            Self::Npn => Some("os_npn_adjust"),
            Self::Pnp => Some("os_pnp_adjust"),
            Self::Nmos => Some("os_nmos_adjust"),
            Self::Pmos => Some("os_pmos_adjust"),
            Self::Njfet => Some("os_njfet_adjust"),
            Self::Pjfet => Some("os_pjfet_adjust"),
            _ => None,
        }
    }

    fn current_probe_terminal(self) -> usize {
        match self {
            Self::Op | Self::OpRail | Self::Pole => 2,
            _ => 0,
        }
    }

    fn current_probe_direction(self, orientation: u8) -> egui::Vec2 {
        let pin = self.terminals((0, 0), 0)[self.current_probe_terminal()];
        let inward = if self.transformer_windings().is_some() || pin.0.abs() >= pin.1.abs() {
            (-pin.0.signum(), 0)
        } else {
            (0, -pin.1.signum())
        };
        let (x, y) = orient_offset(inward, orientation);
        vec2(x as f32, y as f32)
    }

    fn parameters(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Npn | Self::Pnp => &[
                ("bf", "BF · Forward current gain"),
                ("is", "IS · Saturation current (A)"),
                ("nf", "NF · Forward emission factor"),
                ("vaf", "VAF · Forward Early voltage (V)"),
                ("rb", "RB · Base resistance (Ω)"),
                ("rc", "RC · Collector resistance (Ω)"),
                ("re", "RE · Emitter resistance (Ω)"),
                ("cje", "CJE · Base–emitter capacitance (F)"),
                ("cjc", "CJC · Base–collector capacitance (F)"),
                ("tf", "TF · Forward transit time (s)"),
            ],
            Self::Nmos | Self::Pmos => &[
                ("vto", "VTO · Threshold voltage (V)"),
                ("kp", "KP · Transconductance (A/V²)"),
                ("lambda", "LAMBDA · Channel-length mod. (1/V)"),
                ("rd", "RD · Drain resistance (Ω)"),
                ("rs", "RS · Source resistance (Ω)"),
                ("cgso", "CGSO · Gate–source overlap (F/m)"),
                ("cgdo", "CGDO · Gate–drain overlap (F/m)"),
            ],
            Self::Njfet | Self::Pjfet => &[
                ("vto", "VTO · Cutoff voltage (V)"),
                ("beta", "BETA · Transconductance (A/V²)"),
                ("lambda", "LAMBDA · Channel-length mod. (1/V)"),
                ("rd", "RD · Drain resistance (Ω)"),
                ("rs", "RS · Source resistance (Ω)"),
                ("cgs", "CGS · Gate–source capacitance (F)"),
                ("cgd", "CGD · Gate–drain capacitance (F)"),
                ("is", "IS · Gate saturation current (A)"),
            ],
            Self::Transformer | Self::CenterTappedTransformer | Self::CustomTransformer { .. } => {
                &[("ratio", "Turns ratio (Ns/Np)")]
            }
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
    fn parameter_defaults(self) -> &'static str {
        self.adjustable_transistor_model()
            .and_then(|name| {
                include_str!("../assets/components.lib")
                    .lines()
                    .find_map(|line| {
                        let declaration = line.strip_prefix(".subckt ")?;
                        let (model, pins_and_params) = declaration.split_once(' ')?;
                        if model == name {
                            pins_and_params
                                .split_once("params: ")
                                .map(|(_, params)| params)
                        } else {
                            None
                        }
                    })
            })
            .unwrap_or(self.spec().2)
    }

    fn toolbar_icon(self, p: &egui::Painter, center: Pos2, color: Color32, american: bool) {
        let scale = if self == Self::Voltage { 0.56 } else { 0.49 };
        draw(
            p,
            center,
            (self, self.spec().2),
            1,
            scale,
            american,
            (color, Color32::TRANSPARENT),
        );
    }

    fn spec(self) -> (char, &'static str, &'static str) {
        match self {
            Self::Resistor => ('R', "Resistor", "1k"),
            Self::Capacitor => ('C', "Capacitor", "1u"),
            Self::Inductor => ('L', "Inductor", "1m"),
            Self::Voltage => ('V', "Voltage source", "5"),
            Self::Current => ('I', "Current source", "1m"),
            Self::Diode => ('X', "Diode", "os_diode_piecewise"),
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
            Self::CenterTappedTransformer => {
                ('X', "Center-tapped transformer", "os_xfmr_ct ratio=1")
            }
            Self::CustomTransformer { .. } => {
                ('X', "Custom transformer", "os_xfmr_multi_1_2 ratio=1")
            }
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

fn transformer_model(model: &str) -> (&str, u16) {
    model
        .rsplit_once("_pol_")
        .and_then(|(base, mask)| mask.parse().ok().map(|mask| (base, mask)))
        .unwrap_or((model, 0))
}

fn polarized_transformer_definition(model: &str, base: &str, mask: u16) -> Option<String> {
    use std::fmt::Write as _;
    if base.eq_ignore_ascii_case("os_xfmr_ct") {
        let primary = if mask & 1 == 0 { "p1 p2" } else { "p2 p1" };
        let upper = if mask & 2 == 0 { "" } else { "-" };
        let lower = if mask & 4 == 0 { "" } else { "-" };
        let reflected_upper = if upper.is_empty() { "-" } else { "" };
        let reflected_lower = if lower.is_empty() { "-" } else { "" };
        return Some(format!(
            ".subckt {model} p1 p2 s1 tap s2 params: ratio=1\n\
             Eupper s1 tap {primary} {{{upper}ratio/2}}\n\
             Elower tap s2 {primary} {{{lower}ratio/2}}\n\
             Fupper {primary} Eupper {{{reflected_upper}ratio/2}}\n\
             Flower {primary} Elower {{{reflected_lower}ratio/2}}\n\
             Rtapref tap 0 1T\n.ends"
        ));
    }
    let count = if base.eq_ignore_ascii_case("os_xfmr") {
        4
    } else {
        Part::custom_transformer(base)?.pin_count()
    };
    let pins: Vec<_> = (1..=count).map(|n| format!("pin{n}")).collect();
    let mut connection = pins.clone();
    for (index, pair) in connection.chunks_exact_mut(2).enumerate() {
        if mask & (1 << index) != 0 {
            pair.swap(0, 1);
        }
    }
    let mut definition = format!(".subckt {model} {} params: ratio=1\n", pins.join(" "));
    writeln!(
        definition,
        "Xcore {} {base} ratio={{ratio}}",
        connection.join(" ")
    )
    .unwrap();
    definition.push_str(".ends");
    Some(definition)
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
                    remainder
                        .split_whitespace()
                        .enumerate()
                        .find_map(|(index, word)| {
                            Part::custom_transformer(transformer_model(word).0)
                                .filter(|part| part.pin_count() == index)
                        })
                        .or_else(|| {
                            Part::LIBRARY.into_iter().find(|p| {
                                if matches!(p, Part::CustomTransformer { .. }) {
                                    return false;
                                }
                                remainder.split_whitespace().nth(p.pin_count()).is_some_and(
                                    |model| {
                                        let model = if p.transformer_windings().is_some() {
                                            transformer_model(model).0
                                        } else {
                                            model
                                        };
                                        (*p == Part::Diode
                                            && DIODES
                                                .iter()
                                                .any(|(key, _)| model.trim_end_matches("_adjust").eq_ignore_ascii_case(key) || matches!(model, "os_diode_piecewise" | "os_diode_ideal")))
                                            || (*p == Part::Op
                                                && model.eq_ignore_ascii_case("os_op"))
                                            || p.adjustable_transistor_model().is_some_and(|name| {
                                                model.eq_ignore_ascii_case(name)
                                            })
                                            || model.eq_ignore_ascii_case(
                                                p.spec().2.split_whitespace().next().unwrap(),
                                            )
                                    },
                                )
                            })
                        })
                } else {
                    Part::ALL.into_iter().find(|p| prefix == p.spec().0)
                };
                if let Some(part) = part {
                    let mut nodes = Vec::new();
                    let count = if legacy_pole { 3 } else { part.pin_count() };
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
    let (base, mask) = transformer_model(name);
    let model = if mask != 0 {
        let Some(definition) = polarized_transformer_definition(name, base, mask) else {
            return;
        };
        if !base.eq_ignore_ascii_case("os_xfmr_ct") {
            ensure_model(text, base);
        }
        definition
    } else if let Some(Part::CustomTransformer { inputs, outputs }) = Part::custom_transformer(name)
    {
        use std::fmt::Write as _;
        let mut model = format!(".subckt {name} p1a p1b");
        for (prefix, start, count) in [('p', 2, inputs), ('s', 1, outputs)] {
            for n in start..=count {
                write!(model, " {prefix}{n}a {prefix}{n}b").unwrap();
            }
        }
        // Give floating windings a DC reference without materially loading them.
        model.push_str(" params: ratio=1\nRp1ref p1b 0 1T\n");
        // Each ideal winding has its own voltage source. A current-controlled
        // source reflects that winding's load back to the reference primary.
        for (prefix, start, count, gain, reflected) in [
            ('p', 2, inputs, "1", "-1"),
            ('s', 1, outputs, "{ratio}", "{-ratio}"),
        ] {
            for n in start..=count {
                writeln!(
                    model,
                    "E{prefix}{n} {prefix}{n}a {prefix}{n}b p1a p1b {gain}"
                )
                .unwrap();
                writeln!(model, "F{prefix}{n} p1a p1b E{prefix}{n} {reflected}").unwrap();
                writeln!(model, "R{prefix}{n}ref {prefix}{n}b 0 1T").unwrap();
            }
        }
        model.push_str(".ends");
        model
    } else {
        let mut lines = include_str!("../assets/components.lib")
            .lines()
            .skip_while(|l| !declares(l));
        let mut model: Vec<_> = lines.by_ref().take_while(|l| *l != ".ends").collect();
        if model.is_empty() {
            return;
        }
        model.push(".ends");
        model.join("\n")
    };
    let mut deck: Vec<_> = text.lines().collect();
    let end = deck
        .iter()
        .position(|l| l.trim().eq_ignore_ascii_case(".end"))
        .unwrap_or(deck.len());
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
        let terminal = card.part.current_probe_terminal();
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
    Comment,
}

struct Properties {
    part: Part,
    parameters: Vec<String>,
    winding_polarity: u16,
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

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct Comment {
    position: Point,
    text: String,
}

struct CommentEdit {
    index: usize,
    draft: Comment,
    focus: bool,
}

const COMMENT_FONT_SIZE: f32 = 15.;
const LABEL_NAME_SIZE: f32 = 12.;
const LABEL_VALUE_SIZE: f32 = 10.;
const LABEL_CLEARANCE: f32 = 8.;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
enum LabelPlacement {
    Offset(f32, f32),
    Hidden,
}

#[derive(Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Geometry {
    #[serde(default)]
    #[serde(skip_serializing)]
    extended_transformer_primaries: bool,
    #[serde(default)]
    wide_transformer_windings: bool,
    #[serde(default)]
    full_center_tapped_secondary: bool,
    rotations: HashMap<String, u8>,
    positions: HashMap<String, Point>,
    wires: Vec<Wire>,
    grounds: Vec<Point>,
    powers: Vec<Point>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    power_voltages: HashMap<Point, f64>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    power_turns: HashMap<Point, u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    comments: Vec<Comment>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    labels: HashMap<String, LabelPlacement>,
}

impl Geometry {
    pub(crate) fn same_circuit(&self, other: &Self) -> bool {
        self.rotations == other.rotations
            && self.positions == other.positions
            && self.wires == other.wires
            && self.grounds == other.grounds
            && self.powers == other.powers
            && self.power_voltages == other.power_voltages
            && self.power_turns == other.power_turns
    }
}

struct OperatingPoint {
    values: HashMap<String, f64>,
    display: crate::analysis::OperatingPointDisplay,
    probes: std::collections::BTreeMap<String, Point>,
}

#[derive(Default)]
pub struct Schematic {
    pub american_resistor: bool,
    pub light_canvas: bool,
    kinds: HashMap<String, Part>,
    pub selector_open: bool,
    search: String,
    preview_part: Option<Part>,
    editing: Option<Properties>,
    transformer_setup: Option<(u8, u8)>,
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
    comments: Vec<Comment>,
    labels: HashMap<String, LabelPlacement>,
    editing_comment: Option<CommentEdit>,
    new_comment: Option<Comment>,
    drag_leads: Vec<(Point, Point)>,
    pub(crate) tool: Tool,
    pub fit_pending: bool,
    pub center_pending: bool,
    start: Option<Point>,
    dragging_part: bool,
    selected: HashSet<Object>,
    drag_origin: Option<Geometry>,
    gesture: Option<Gesture>,
    copy_anchor: Option<Point>,
    operating_point: Option<OperatingPoint>,
    voltage_drag: Option<String>,
    snapshot: String,
    label_layout: Option<HashMap<String, egui::Rect>>,
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
    Comment(usize),
    Label(String),
}

impl Object {
    fn is_annotation(&self) -> bool {
        matches!(self, Self::Comment(_) | Self::Label(_))
    }
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
            extended_transformer_primaries: false,
            wide_transformer_windings: true,
            full_center_tapped_secondary: true,
            rotations: self.rotations.clone(),
            positions: self.positions.clone(),
            wires: self.wires.clone(),
            grounds: self.grounds.clone(),
            powers: self.powers.clone(),
            power_voltages: self.power_voltages.clone(),
            power_turns: self.power_turns.clone(),
            comments: self.comments.clone(),
            labels: self.labels.clone(),
        }
    }
    fn set_geometry(&mut self, g: Geometry) {
        self.rotations = g.rotations;
        self.positions = g.positions;
        self.wires = g.wires;
        self.grounds = g.grounds;
        self.powers = g.powers;
        self.power_voltages = g.power_voltages;
        self.power_turns = g.power_turns;
        self.comments = g.comments;
        self.labels = g.labels;
    }
    pub fn matches_document(g: &Geometry, text: &str) -> bool {
        let parts = cards(text);
        g.power_voltages
            .iter()
            .all(|(p, v)| g.powers.contains(p) && v.is_finite())
            && parts.len() == g.positions.len()
            && parts.iter().all(|c| g.positions.contains_key(c.name))
            && g.labels.iter().all(|(name, placement)| {
                g.positions.contains_key(name)
                    && match placement {
                        LabelPlacement::Hidden => true,
                        LabelPlacement::Offset(x, y) => x.is_finite() && y.is_finite(),
                    }
            })
            && g.positions
                .values()
                .chain(&g.grounds)
                .chain(&g.powers)
                .chain(g.comments.iter().map(|comment| &comment.position))
                .chain(g.wires.iter().flat_map(|(a, b)| [a, b]))
                .all(|&(x, y)| x.unsigned_abs() <= 1_000_000 && y.unsigned_abs() <= 1_000_000)
    }
    pub fn restore(&mut self, g: Geometry, text: &str) {
        self.cancel();
        self.drag_leads.clear();
        self.editing = None;
        self.editing_power = None;
        self.editing_comment = None;
        self.new_comment = None;
        self.selected.clear();
        let wide_windings = g.wide_transformer_windings;
        let extended_primaries = g.extended_transformer_primaries;
        let migrate_transformers = !wide_windings || !g.full_center_tapped_secondary;
        self.set_geometry(g);
        self.index_parts(text);
        if migrate_transformers {
            self.migrate_transformer_connections(wide_windings, extended_primaries);
        }
        self.label_layout = None;
        self.snapshot = text.into();
    }
    fn migrate_transformer_connections(&mut self, wide_windings: bool, extended_primaries: bool) {
        let mut moved = HashMap::<Point, Vec<Point>>::new();
        let mut stationary = HashSet::new();
        for (name, &center) in &self.positions {
            let part = self.kinds[name];
            let turns = self.turns(name);
            let terminals = part.terminals(center, turns);
            let Some((inputs, outputs)) = part.transformer_windings() else {
                stationary.extend(terminals);
                continue;
            };
            let mut old = Vec::new();
            let primary_x = if !wide_windings && extended_primaries {
                -3
            } else {
                -2
            };
            let half_span = if wide_windings { WINDING_HALF_SPAN } else { 1 };
            let half_pitch = if wide_windings { 3 } else { 2 };
            for (x, count) in [(primary_x, inputs), (2, outputs)] {
                for y in winding_centers(count, half_pitch) {
                    old.push((x, y - half_span));
                    if x > 0 && part == Part::CenterTappedTransformer {
                        old.push((x, y));
                    }
                    old.push((x, y + half_span));
                }
            }
            for (offset, new) in old.into_iter().zip(terminals) {
                let (x, y) = orient_offset(offset, turns);
                let old = (center.0 + x, center.1 + y);
                if old == new {
                    stationary.insert(old);
                } else {
                    moved.entry(old).or_default().push(new);
                }
            }
        }
        if moved.is_empty() {
            return;
        }
        // Move each shared wire vertex once. Split wires first when a pin was
        // connected in the middle of a segment rather than at an endpoint.
        let remap = |point| {
            if stationary.contains(&point) {
                return point;
            }
            moved
                .get(&point)
                .map_or(point, |destinations| destinations[0])
        };
        let mut wires = Vec::new();
        for (a, b) in std::mem::take(&mut self.wires) {
            let mut vertices = vec![a, b];
            vertices.extend(moved.keys().copied().filter(|&p| on_wire(p, (a, b))));
            vertices.sort_unstable();
            vertices.dedup();
            wires.extend(
                vertices
                    .windows(2)
                    .map(|pair| (remap(pair[0]), remap(pair[1]))),
            );
        }
        self.grounds.iter_mut().for_each(|p| *p = remap(*p));
        self.powers.iter_mut().for_each(|p| *p = remap(*p));
        self.power_voltages = std::mem::take(&mut self.power_voltages)
            .into_iter()
            .map(|(p, v)| (remap(p), v))
            .collect();
        self.power_turns = std::mem::take(&mut self.power_turns)
            .into_iter()
            .map(|(p, turns)| (remap(p), turns))
            .collect();
        for (&old, destinations) in &moved {
            let junction = remap(old);
            wires.extend(
                destinations
                    .iter()
                    .filter(|&&p| p != junction)
                    .map(|&p| (junction, p)),
            );
        }
        self.wires = wires;
        self.normalize();
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
    #[cfg(test)]
    pub(crate) fn grid_offset(&self, offset: egui::Vec2) -> egui::Vec2 {
        self.pan + offset * 1.25_f32.powf(self.zoom_steps)
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
                        tile[(x, y)] = Color32::from_rgba_unmultiplied(105, 123, 141, alpha);
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

    fn change_orientation(&mut self, edit: OrientationEdit) -> bool {
        if matches!(self.tool, Tool::Place(_) | Tool::Power) {
            if self.tool != Tool::Power || matches!(edit, OrientationEdit::Rotate) {
                self.preview_turns = edit.apply(self.preview_turns);
            }
            return false;
        }
        if self.tool == Tool::Move {
            if self.drag_origin.is_some() {
                for object in &self.selected {
                    if let Object::Part(name) = object {
                        let turns = self.rotations.entry(name.clone()).or_default();
                        *turns = edit.apply(*turns);
                    } else if let Object::Power(index) = *object
                        && matches!(edit, OrientationEdit::Rotate)
                    {
                        rotate_power(&mut self.power_turns, self.powers[index]);
                    }
                }
            }
            // Commit the orientation change together with the placement.
            return false;
        }
        if !matches!(self.tool, Tool::Cursor | Tool::Select) {
            return false;
        }
        self.label_layout = None;
        let mut moves = HashMap::new();
        let mut power_rotated = false;
        for obj in &self.selected {
            if let Object::Part(n) = obj {
                let center = self.positions[n];
                let turns = self.rotations.entry(n.clone()).or_default();
                let old = self.kinds[n].terminals(center, *turns);
                *turns = edit.apply(*turns);
                moves.extend(old.into_iter().zip(self.kinds[n].terminals(center, *turns)));
            } else if let Object::Power(index) = *obj
                && matches!(edit, OrientationEdit::Rotate)
            {
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
    fn canvas_colors(&self) -> (Color32, Color32, Color32, Color32, Color32) {
        if self.light_canvas {
            (
                Color32::WHITE,
                Color32::from_rgb(0, 128, 35),
                Color32::from_rgb(135, 15, 24),
                Color32::from_rgb(0, 108, 116),
                Color32::from_rgb(35, 111, 211),
            )
        } else {
            (PAPER, WIRE, INK, LABEL, ACCENT)
        }
    }

    fn toolbar_colors(&self) -> (Color32, Color32) {
        if self.light_canvas {
            (
                Color32::from_rgb(88, 206, 117),
                Color32::from_rgb(237, 129, 139),
            )
        } else {
            (WIRE, INK)
        }
    }

    pub fn picker(&mut self, ui: &mut Ui) -> bool {
        let (wire, ink) = self.toolbar_colors();
        let mut chosen = false;
        for (label, tool) in [("Wire", Tool::Wire), ("Ground", Tool::Ground)] {
            let (rect, response) =
                ui.allocate_exact_size(crate::ui::TOOL_SIZE, egui::Sense::click());
            crate::ui::toolbar_surface(ui, &response, self.tool == tool);
            let center = rect.center();
            let pen = Stroke::new(1.7, if tool == Tool::Wire { wire } else { ink });
            let path = |points: &[(f32, f32)]| {
                ui.painter().add(egui::Shape::line(
                    points
                        .iter()
                        .map(|&(x, y)| center + vec2(x, y) * 1.2)
                        .collect(),
                    pen,
                ));
            };
            match tool {
                Tool::Wire => {
                    path(&[(-8., 6.), (-2., 6.), (-2., -5.), (8., -5.)]);
                    for (x, y) in [(-8., 6.), (8., -5.)] {
                        ui.painter()
                            .circle_filled(center + vec2(x, y) * 1.2, 2., pen.color);
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
        let center = rect.center() - vec2(4., 0.);
        power_arrow(ui.painter(), center + vec2(0., 10.), 1.1, 0, ink);
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
            crate::ui::toolbar_surface(ui, &response, self.tool == Tool::Place(part));
            part.toolbar_icon(ui.painter(), rect.center(), ink, self.american_resistor);
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
        if response.clicked() { self.selector_open = !self.selector_open; }
        let color = crate::ui::toolbar_surface(ui, &response, self.selector_open);
        let center = rect.center() - vec2(4., 0.);
        let pen = Stroke::new(1.7, ink);
        ui.painter().rect_stroke(
            egui::Rect::from_center_size(center, vec2(15., 22.)),
            2,
            pen,
            egui::StrokeKind::Middle,
        );
        ui.painter()
            .circle_filled(center + vec2(-3., -6.), 1.5, ink);
        for side in [-1., 1.] {
            for y in [-6.5, 0., 6.5] {
                icon_path(
                    ui.painter(),
                    center,
                    pen,
                    &[
                        (side * 7.5, y - 1.3),
                        (side * 11., y - 1.3),
                        (side * 11., y + 1.3),
                        (side * 7.5, y + 1.3),
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
        chosen
    }

    pub fn component_selector(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("Components");
            if ui.small_button("×").clicked() { self.selector_open = false; }
        });
        ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search components"));
        let query = self.search.trim().to_lowercase();
        let height = (ui.available_height() - 96.).max(24.);
        egui::ScrollArea::vertical().max_height(height).show(ui, |ui| {
            for part in Part::ALL.into_iter().chain(Part::LIBRARY) {
                let label = part.spec().1;
                if !label.to_lowercase().contains(&query) { continue; }
                let row = ui.selectable_label(self.preview_part == Some(part), label);
                if row.hovered() { self.preview_part = Some(part); }
                if row.clicked() {
                    self.preview_part = Some(part);
                    if let Part::CustomTransformer { inputs, outputs } = part {
                        self.cancel();
                        self.transformer_setup = Some((inputs, outputs));
                    } else { self.begin_placement(part, false); }
                }
            }
        });
        ui.separator();
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 86.), egui::Sense::hover());
        if let Some(part) = self.preview_part {
            draw(ui.painter(), rect.center(), (part, ""), 0, 0.78, self.american_resistor, (self.toolbar_colors().1, Color32::TRANSPARENT));
        }
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
        let (_, ink) = self.toolbar_colors();
        let icon_center = rect.center() - vec2(4., 0.);
        draw(
            ui.painter(),
            icon_center,
            (variants[0], ""),
            0,
            0.47,
            self.american_resistor,
            (ink, Color32::TRANSPARENT),
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
                let center = rect.left_center() + vec2(17., 0.);
                draw(
                    ui.painter(),
                    center,
                    (part, ""),
                    0,
                    0.47,
                    self.american_resistor,
                    (ink, Color32::TRANSPARENT),
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

    pub fn editor_open(&self) -> bool { self.editing.is_some() }

    pub fn component_editor(&mut self, ui: &mut Ui, text: &mut String) -> bool {
        if let Some(mut edit) = self.editing.take() {
            ui.heading("Component properties");
            ui.separator();
            let height = (ui.available_height() - 44.).max(24.);
            let valid = egui::ScrollArea::vertical().max_height(height).show(ui, |ui| {
                    egui::Grid::new("component fields")
                        .min_col_width(132.)
                        .show(ui, |ui| {
                            crate::ui::dialog_field(ui, "Reference", &mut edit.reference);
                            if let Some(source) = &mut edit.source {
                                source.show(ui, edit.original.starts_with(['I', 'i']));
                                edit.value = source.definition();
                            } else if edit.part == Part::Diode {
                                let original = edit.value.split_whitespace().next().unwrap_or("").to_owned();
                                let was_ideal = matches!(original.as_str(), "os_diode_piecewise" | "os_diode_ideal");
                                let mut ideal = was_ideal;
                                ui.label("Model");
                                ui.horizontal(|ui| {
                                    ui.selectable_value(&mut ideal, true, "Idealized");
                                    ui.selectable_value(&mut ideal, false, "Realistic");
                                });
                                ui.end_row();
                                let mut kind = diode_type(&original).0;
                                if !ideal {
                                    ui.label("Diode type");
                                    egui::ComboBox::from_id_salt("diode type")
                                        .selected_text(diode_type(kind).1).show_ui(ui, |ui| {
                                            for (model, label) in DIODES { ui.selectable_value(&mut kind, model, label); }
                                        });
                                    ui.end_row();
                                }
                                let model = if ideal { "os_diode_piecewise".to_owned() } else { format!("{kind}_adjust") };
                                let mut definition = model.clone();
                                let defaults = include_str!("../assets/components.lib").lines()
                                    .find(|l| l.split_whitespace().nth(1) == Some(model.as_str()) && l.starts_with(".subckt")).unwrap_or("");
                                for (key, default) in defaults.split_whitespace().filter_map(|s| s.split_once('=')) {
                                    let mut v = edit.value.split_whitespace().filter_map(|s| s.split_once('='))
                                        .find(|(k,_)| *k == key && (original == model || original == "os_diode_ideal" && ideal))
                                        .map_or(default, |(_,v)| v).to_owned();
                                    let label = match key { "vf" => "Forward drop (V)", "ron" => "On resistance (Ω)", "roff" => "Off resistance (Ω)", "bv" => "Breakdown voltage (V)", _ => key };
                                    crate::ui::dialog_field(ui, label, &mut v);
                                    definition.push_str(&format!(" {key}={v}"));
                                }
                                edit.value = definition;
                            } else if edit.part.spec().0 == 'X' {
                                ui.label("Model");
                                ui.strong(edit.part.spec().1);
                                ui.end_row();
                                for ((_, label), v) in
                                    edit.part.parameters().iter().zip(&mut edit.parameters)
                                {
                                    crate::ui::dialog_field(ui, label, v);
                                }
                                if let Some((inputs, outputs)) = edit.part.transformer_windings() {
                                    let tapped = edit.part == Part::CenterTappedTransformer;
                                    let outputs = if tapped { 2 } else { outputs };
                                    for index in 0..inputs + outputs {
                                        let name = if index < inputs {
                                            format!("Primary {}", index + 1)
                                        } else if tapped {
                                            if index == inputs {
                                                "Upper secondary".into()
                                            } else {
                                                "Lower secondary".into()
                                            }
                                        } else {
                                            format!("Secondary {}", index - inputs + 1)
                                        };
                                        let bit = 1 << index;
                                        let mut reversed = edit.winding_polarity & bit != 0;
                                        ui.label(name);
                                        if ui.checkbox(&mut reversed, "Reverse polarity").changed()
                                        {
                                            edit.winding_polarity ^= bit;
                                        }
                                        ui.end_row();
                                    }
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
                    } else if edit.part == Part::Diode && edit.value.split_whitespace().skip(1).any(|s| {
                        s.split_once('=').is_none_or(|(k,v)| crate::analysis::number(v).is_none_or(|n| !n.is_finite() || n < 0. || matches!(k,"ron"|"roff"|"is"|"n") && n == 0.))
                    }) { Some("Enter valid numeric diode parameters.") } else if duplicate {
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
                    } else if edit.part.adjustable_transistor_model().is_some() {
                        edit.part.parameters().iter().zip(&edit.parameters).find_map(|((key, _), value)| {
                            match crate::analysis::number(value) {
                                None => Some("Enter a numeric value for each model parameter."),
                                Some(n) if matches!(*key, "bf" | "is" | "nf" | "kp" | "beta") && n <= 0. => {
                                    Some("BF, IS, NF, KP and BETA must be positive where applicable.")
                                }
                                Some(n) if *key != "vto" && n < 0. => {
                                    Some("Only VTO can be negative.")
                                }
                                _ => None,
                            }
                        })
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
                    error.is_none()
            }).inner;
            ui.separator();
            let mut cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
            let apply = ui.horizontal(|ui| {
                cancel |= ui.button("Cancel").clicked();
                ui.add_enabled(valid, egui::Button::new("Apply")).clicked()
            }).inner;
            if apply {
                if let Some(c) = cards(text).into_iter().find(|c| c.name == edit.original) {
                    if edit.part.spec().0 == 'X' && edit.part != Part::Diode {
                        let model = edit.part.adjustable_transistor_model().unwrap_or_else(|| {
                            if matches!(edit.part, Part::CustomTransformer { .. }) {
                                c.value.split_whitespace().next().unwrap()
                            } else {
                                edit.part.spec().2.split_whitespace().next().unwrap()
                            }
                        });
                        let base = transformer_model(model).0;
                        let model_name = if edit.part.transformer_windings().is_some()
                            && edit.winding_polarity != 0
                        {
                            format!("{base}_pol_{}", edit.winding_polarity)
                        } else {
                            base.to_owned()
                        };
                        let mut fields = vec![model_name];
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
                    self.rename_label(&edit.original, &edit.reference);
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
            } else if !cancel {
                self.editing = Some(edit);
            }
        }
        false
    }

    fn accepts_tool_shortcuts(&self, ui: &Ui) -> bool {
        self.transformer_setup.is_none()
            && self.editing_power.is_none()
            && self.editing_comment.is_none()
            && self.new_comment.is_none()
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
                (Key::T, Modifiers::NONE, Tool::Comment),
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

    pub(crate) fn writing_comment(&self) -> bool {
        self.new_comment.is_some()
    }

    pub fn cancel(&mut self) {
        self.voltage_drag = None;
        self.tool = Tool::Cursor;
        self.transformer_setup = None;
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
        self.labels
            .retain(|name, _| self.positions.contains_key(name));
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

    fn label_bounds(&self, name: &str) -> Option<egui::Rect> {
        let bounds = *self.label_layout.as_ref()?.get(name)?;
        match self.labels.get(name) {
            Some(LabelPlacement::Hidden) => None,
            Some(LabelPlacement::Offset(x, y)) => Some(egui::Rect::from_min_size(
                world_position(self.positions[name]) + vec2(*x, *y),
                bounds.size(),
            )),
            None => Some(self.drag_origin.as_ref().map_or(bounds, |origin| {
                bounds.translate(label_move_offset(name, &self.positions, origin))
            })),
        }
    }

    fn translate_moved_labels(&mut self, origin: &Geometry) {
        if let Some(layout) = &mut self.label_layout {
            for (name, bounds) in layout {
                *bounds = bounds.translate(label_move_offset(name, &self.positions, origin));
            }
        }
    }

    fn rename_label(&mut self, original: &str, reference: &str) {
        if original != reference {
            if let Some(placement @ LabelPlacement::Offset(..)) = self.labels.remove(original) {
                self.labels.insert(reference.to_owned(), placement);
            }
            if self.selected.remove(&Object::Label(original.to_owned())) {
                self.selected.insert(Object::Label(reference.to_owned()));
            }
        }
    }

    fn move_selection(&mut self, delta: Point) {
        let shift = |p: &mut Point| {
            p.0 += delta.0;
            p.1 += delta.1;
        };
        for (i, comment) in self.comments.iter_mut().enumerate() {
            if self.selected.contains(&Object::Comment(i)) {
                shift(&mut comment.position);
            }
        }
        if delta != (0, 0) {
            for object in &self.selected {
                if let Object::Label(name) = object
                    && !self.selected.contains(&Object::Part(name.clone()))
                    && let Some(bounds) = self.label_bounds(name)
                {
                    let offset = bounds.min - world_position(self.positions[name])
                        + world_position(delta).to_vec2();
                    self.labels
                        .insert(name.clone(), LabelPlacement::Offset(offset.x, offset.y));
                }
            }
        }
        if self.selected.iter().all(Object::is_annotation) {
            return;
        }
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
                if self.kinds[*n].transformer_windings().is_some() {
                    let center = egui::pos2(p.0 as f32 * GRID, p.1 as f32 * GRID);
                    return symbol_bounds(self.kinds[*n], center, self.turns(n), 1.)
                        .contains(egui::pos2(q.0 as f32 * GRID, q.1 as f32 * GRID));
                }
                if self.kinds[*n].pin_count() > 2 {
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
        let mut index = 0;
        self.comments.retain(|_| {
            let keep = !targets.contains(&Object::Comment(index));
            index += 1;
            keep
        });
        for object in &targets {
            if let Object::Label(name) = object {
                self.labels.insert(name.clone(), LabelPlacement::Hidden);
            }
        }
        if targets.iter().all(Object::is_annotation) {
            self.selected.clear();
            self.gesture = None;
            return true;
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
        self.labels.retain(|n, _| self.positions.contains_key(n));
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
            let nodes = vec!["?"; card.part.pin_count()].join(" ");
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
            if let Some(&placement) = self.labels.get(card.name) {
                self.labels.insert(name.clone(), placement);
            }
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
                Object::Comment(index) => {
                    let mut comment = self.comments[index].clone();
                    comment.position = offset(comment.position);
                    self.comments.push(comment);
                }
                Object::Part(_) | Object::Label(_) => {}
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
        mut probe: Option<&mut Option<(String, bool)>>,
        current_hover: Option<&str>,
    ) -> bool {
        if self.snapshot != *text {
            self.import(text);
        }
        let mut changed = false;
        let mut annotations_only = false;
        let mut keep_label_layout = false;
        if self.accepts_tool_shortcuts(ui)
            && !ui.ctx().memory(|memory| memory.top_modal_layer().is_some())
        {
            let edit = ui.input_mut(|input| {
                use egui::{Key, Modifiers};
                [
                    (
                        Modifiers::CTRL | Modifiers::SHIFT,
                        Key::E,
                        OrientationEdit::MirrorVertical,
                    ),
                    (Modifiers::CTRL, Key::E, OrientationEdit::MirrorHorizontal),
                    (Modifiers::CTRL, Key::R, OrientationEdit::Rotate),
                ]
                .into_iter()
                .find_map(|(modifiers, key, edit)| {
                    input.consume_key(modifiers, key).then_some(edit)
                })
            });
            if let Some(edit) = edit {
                changed = self.change_orientation(edit);
            }
        }
        {
            let (r, p) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
            let (paper, wire_color, ink, label, accent) = self.canvas_colors();
            let comment_color = if self.light_canvas {
                Color32::from_rgb(24, 116, 67)
            } else {
                Color32::from_rgb(125, 211, 153)
            };
            let comment_galley = |comment: &Comment, scale| {
                p.layout_no_wrap(
                    comment.text.clone(),
                    FontId::proportional(COMMENT_FONT_SIZE * scale),
                    comment_color,
                )
            };
            let parts = cards(text);
            // Only geometry edits can reflow existing labels. A floating
            // component finds room for its own label after these are placed.
            let world = world_position;
            if self.label_layout.is_none() {
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
                let mut layout = HashMap::new();
                let mut automatic = Vec::new();
                for c in &parts {
                    let center = world(self.positions[c.name]);
                    let placement = self.labels.get(c.name);
                    if placement == Some(&LabelPlacement::Hidden) {
                        continue;
                    }
                    let (name, value) = label_text(&p, c, 1., label);
                    let size = label_size(&name, &value);
                    if let Some(&LabelPlacement::Offset(x, y)) = placement {
                        let bounds = egui::Rect::from_min_size(center + vec2(x, y), size);
                        occupied.push(bounds.expand(2.));
                        layout.insert(c.name.to_owned(), bounds);
                    } else {
                        automatic.push((c.name, center, size));
                    }
                }
                for (name, center, size) in automatic {
                    let bounds = place_label(
                        center,
                        1.,
                        size,
                        LABEL_CLEARANCE,
                        &wires,
                        &mut occupied,
                        egui::Rect::EVERYTHING,
                    )
                    .unwrap();
                    layout.insert(name.to_owned(), bounds);
                }
                self.label_layout = Some(layout);
            }
            let viewport = r.rect.size();
            let resized = self.viewport != egui::Vec2::ZERO && viewport != self.viewport;
            self.viewport = viewport;
            let fit = std::mem::take(&mut self.fit_pending);
            let center = std::mem::take(&mut self.center_pending);
            if fit || center || resized {
                let mut bounds = egui::Rect::NOTHING;
                for c in &parts {
                    bounds = bounds.union(symbol_bounds(
                        c.part,
                        world(self.positions[c.name]),
                        self.turns(c.name),
                        1.,
                    ));
                }
                for label in parts.iter().filter_map(|c| self.label_bounds(c.name)) {
                    bounds = bounds.union(label);
                }
                for &(a, b) in &self.wires {
                    bounds.extend_with(world(a));
                    bounds.extend_with(world(b));
                }
                for &point in self.grounds.iter().chain(&self.powers) {
                    bounds =
                        bounds.union(egui::Rect::from_center_size(world(point), vec2(42., 76.)));
                }
                for comment in &self.comments {
                    bounds = bounds.union(egui::Rect::from_min_size(
                        world(comment.position),
                        comment_galley(comment, 1.).size(),
                    ));
                }
                if bounds.is_finite() {
                    let available = (viewport - vec2(40., 40.)).max(vec2(1., 1.));
                    let size = bounds.size().max(vec2(1., 1.));
                    let scale_limit = if fit {
                        1.25_f32.powf(7.)
                    } else {
                        1.25_f32.powf(self.zoom_steps)
                    };
                    let scale = (available.x / size.x)
                        .min(available.y / size.y)
                        .min(scale_limit);
                    self.zoom_steps = scale.log(1.25);
                    self.pan = viewport * 0.5 - bounds.center().to_vec2() * scale;
                }
            }
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
            let show_labels = LABEL_VALUE_SIZE * scale * p.ctx().pixels_per_point() >= 6.;
            let label_bounds: Vec<_> = if show_labels {
                parts
                    .iter()
                    .filter_map(|c| {
                        let bounds = self.label_bounds(c.name)?;
                        let bounds = egui::Rect::from_min_max(
                            origin + bounds.min.to_vec2() * scale,
                            origin + bounds.max.to_vec2() * scale,
                        );
                        bounds.intersects(rect).then_some((c, bounds))
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let label_at = |pos: Pos2| {
                label_bounds
                    .iter()
                    .rfind(|(_, bounds)| bounds.expand(2.).contains(pos))
                    .map(|(card, _)| card.name)
            };
            p.rect_filled(rect, 0, paper);
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
            let comments: Vec<_> = self
                .comments
                .iter()
                .map(|comment| {
                    let galley = comment_galley(comment, scale);
                    let bounds = egui::Rect::from_min_size(screen(comment.position), galley.size());
                    (bounds, galley)
                })
                .collect();
            let comment_at = |pos: Pos2| {
                comments
                    .iter()
                    .rposition(|(bounds, _)| bounds.expand(3.).contains(pos))
            };
            let hit_at = |schematic: &Self, pos: Pos2| {
                comment_at(pos)
                    .map(Object::Comment)
                    .or_else(|| {
                        label_at(pos)
                            .filter(|_| schematic.tool != Tool::Copy)
                            .map(|name| Object::Label(name.into()))
                    })
                    .or_else(|| schematic.symbol_at(snap(pos)))
                    .or_else(|| wire_at(pos, &schematic.wires).map(Object::Wire))
            };
            let keyboard_delete = if r.hovered()
                && self.accepts_tool_shortcuts(ui)
                && self.drag_origin.is_none()
                && self.copy_anchor.is_none()
                && !ui.input(|i| i.pointer.any_down())
                && !ui.ctx().memory(|memory| memory.top_modal_layer().is_some())
                && ui.input_mut(|i| {
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                        || i.consume_key(egui::Modifiers::CTRL, egui::Key::X)
                        || consume_clipboard_event(i, egui::Event::Cut)
                }) {
                let hovered = r.hover_pos().and_then(|pos| hit_at(self, pos));
                if hovered.is_some() || !self.selected.is_empty() {
                    Some(hovered)
                } else {
                    self.cancel();
                    self.tool = Tool::Erase;
                    None
                }
            } else {
                None
            };
            let color = |selected| {
                if selected { accent } else { wire_color }
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
                    Stroke::new(SCHEMATIC_STROKE, wire_color),
                );
            }
            let placing = inside && matches!(self.tool, Tool::Place(_));
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
            for (index, c) in parts.iter().enumerate() {
                let pos = self.positions[c.name];
                let color = if self.selected.contains(&Object::Part(c.name.into())) {
                    accent
                } else {
                    ink
                };
                if obstacles[index].intersects(rect) {
                    draw(
                        &p,
                        screen(pos),
                        (c.part, c.value),
                        self.turns(c.name),
                        scale,
                        self.american_resistor,
                        (color, paper),
                    );
                    for pin in c.part.terminals(pos, self.turns(c.name)) {
                        if degree[&pin] == 1 {
                            draw_open_pin(&p, screen(pin), scale, color, paper);
                        }
                    }
                }
            }
            for &(c, bounds) in &label_bounds {
                let color = if self.selected.contains(&Object::Label(c.name.into())) {
                    accent
                } else {
                    label
                };
                let (name, value) = label_text(&p, c, scale, color);
                paint_label(&p, bounds.min, name, value);
                if placing || self.operating_point.is_some() {
                    obstacles.push(bounds);
                }
            }
            let hovered_label = r.hover_pos().and_then(label_at);
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
                            accent
                        } else {
                            ink
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
                            accent
                        } else {
                            ink
                        },
                    );
                    if r.hover_pos()
                        .is_some_and(|pos| bounds.expand(3.).contains(pos))
                    {
                        hovered_power = Some(q);
                    }
                }
            }
            for (&q, &n) in &degree {
                if n >= 3 && rect.expand(4.).contains(screen(q)) {
                    p.circle_filled(screen(q), 4., wire_color);
                }
            }
            for (index, (bounds, galley)) in comments.iter().enumerate() {
                if bounds.intersects(rect) {
                    if self.selected.contains(&Object::Comment(index)) {
                        p.rect_stroke(
                            bounds.expand(2.),
                            2,
                            Stroke::new(1., accent),
                            egui::StrokeKind::Outside,
                        );
                    }
                    p.galley(bounds.min, galley.clone(), comment_color);
                }
            }
            let voltage_at = |schematic: &Self, pos: Pos2| {
                let point = snap(pos);
                let pin = if screen(point).distance(pos) <= 6. {
                    parts
                        .iter()
                        .find_map(|card| {
                            card.part
                                .terminals(
                                    schematic.positions[card.name],
                                    schematic.turns(card.name),
                                )
                                .into_iter()
                                .zip(card.nodes.iter().copied())
                                .find_map(|(pin, node)| {
                                    (pin == point && node != "?").then_some(node)
                                })
                        })
                        .or_else(|| schematic.grounds.contains(&point).then_some("0"))
                        .or_else(|| schematic.power_node(point, text))
                } else {
                    None
                };
                pin.or_else(|| {
                    wire_at(pos, &schematic.wires)
                        .and_then(|index| schematic.wire_node(index, text))
                })
            };
            let can_probe = self.new_comment.is_none()
                && (probe.is_some() || self.operating_point.is_some())
                && self.tool == Tool::Cursor
                && self.editing.is_none()
                && self.editing_power.is_none()
                && !r.secondary_clicked();
            let target_node = mouse
                .filter(|&pos| {
                    can_probe
                        && rect.contains(pos)
                        && comment_at(pos).is_none()
                        && label_at(pos).is_none()
                })
                .and_then(|pos| voltage_at(self, pos));
            if can_probe
                && r.is_pointer_button_down_on()
                && ui.input(|i| i.pointer.primary_pressed())
            {
                self.voltage_drag = target_node.map(str::to_owned);
            }
            let captured_voltage_drag = can_probe && self.voltage_drag.is_some() && !r.clicked();
            let mut differential = None;
            if captured_voltage_drag {
                let start = self.voltage_drag.as_deref().unwrap();
                let end = target_node.filter(|node| !node.eq_ignore_ascii_case(start));
                if r.drag_stopped_by(egui::PointerButton::Primary) {
                    differential = end.map(|end| (format!("V({start},{end})"), point));
                } else if ui.input(|i| i.pointer.primary_down())
                    && let Some(pos) = mouse.filter(|&pos| rect.contains(pos))
                {
                    let color = if end.is_some() {
                        Color32::from_rgb(100, 205, 133)
                    } else {
                        Color32::GRAY
                    };
                    probe_cursor(ui, pos, None, self.light_canvas, Some(color));
                }
            }
            if !can_probe || !ui.input(|i| i.pointer.primary_down()) {
                self.voltage_drag = None;
            }

            if let Some((component, reversed)) =
                current_hover.and_then(crate::waveform::current_trace)
                && let Some((name, &center)) = self
                    .positions
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(component))
            {
                let part = self.kinds[name];
                let orientation = self.turns(name);
                let pin = part.terminals(center, orientation)[part.current_probe_terminal()];
                let inward = part.current_probe_direction(orientation);
                let sideways = vec2(-inward.y, inward.x);
                let anchor = screen(pin) + inward * (8. * scale) + sideways * (8. * scale).max(7.);
                let painter = p.with_clip_rect(rect);
                let colors = crate::ui::CURRENT_ARROW_COLORS;
                let direction = if reversed { -inward } else { inward };
                crate::ui::paint_play_triangle(
                    &painter, anchor, direction, 1.65, 1.25, 12., colors,
                );
            }

            let mut probe_clicked = false;
            let mut target = differential;
            if can_probe
                && !captured_voltage_drag
                && mouse.and_then(comment_at).is_none()
                && !r.dragged()
                && let Some(pos) = r.hover_pos()
            {
                target = if let Some(node) = target_node {
                    probe_cursor(ui, pos, None, self.light_canvas, None);
                    r.clicked().then(|| (format!("V({node})"), point))
                } else if let Some(name) = hovered_label.or_else(|| {
                    self.positions
                        .iter()
                        .find(|&(_, &center)| {
                            egui::Rect::from_center_size(screen(center), vec2(34., 34.) * scale)
                                .contains(pos)
                        })
                        .map(|(name, _)| name.as_str())
                }) {
                    let orientation = self.turns(name);
                    let direction = self.kinds[name].current_probe_direction(orientation);
                    let reverse = ui.input(|input| input.modifiers.ctrl);
                    let direction = if reverse { -direction } else { direction };
                    probe_cursor(ui, pos, Some(direction), self.light_canvas, None);
                    r.clicked().then(|| {
                        let sign = if reverse { "-" } else { "" };
                        (format!("{sign}I({name})"), self.positions[name])
                    })
                } else {
                    None
                };
            }
            if let Some((signal, anchor)) = target {
                probe_clicked = true;
                if let Some(op) = &mut self.operating_point {
                    let key = signal.to_ascii_lowercase();
                    if let Some((positive, negative)) = crate::waveform::differential_nodes(&key)
                        && let (Some(a), Some(b)) = (
                            op.values.get(&format!("v({positive})")),
                            op.values.get(&format!("v({negative})")),
                        )
                    {
                        op.values.insert(key.clone(), a - b);
                    }
                    if let Some((component, reversed)) = crate::waveform::current_trace(&key) {
                        let positive = format!("i({component})");
                        let negative = format!("-i({component})");
                        if reversed {
                            if let Some(&value) = op.values.get(&positive) {
                                op.values.insert(negative.clone(), -value);
                            }
                            op.probes.remove(&positive);
                        } else {
                            op.probes.remove(&negative);
                        }
                    }
                    if op.values.contains_key(&key) {
                        op.probes.insert(key, anchor);
                        ui.ctx().request_repaint();
                    }
                } else if let Some(probe) = probe.as_deref_mut() {
                    *probe = Some((signal, r.double_clicked() || r.triple_clicked()));
                }
            }

            if let Some(op) = &self.operating_point {
                let mut annotations = op.probes.clone();
                if op.display.voltages || op.display.currents {
                    for card in cards(text) {
                        let center = self.positions[card.name];
                        if op.display.currents {
                            let name = format!("i({})", card.name.to_ascii_lowercase());
                            if !annotations.contains_key(&format!("-{name}")) {
                                annotations.entry(name).or_insert(center);
                            }
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
                let mut hovered_signal = None;
                for (signal, anchor) in annotations {
                    let Some(&value) = op.values.get(&signal).filter(|v| v.is_finite()) else {
                        continue;
                    };
                    let center = screen(anchor);
                    if !rect.expand(30.).contains(center) {
                        continue;
                    }
                    let text = format_operating_value(
                        value,
                        crate::waveform::current_trace(&signal).is_some(),
                    );
                    let galley = p.layout_no_wrap(text, FontId::proportional(22.), accent);
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
                    if r.hover_pos().is_some_and(|pos| bounds.contains(pos)) {
                        hovered_signal = Some(signal);
                    }
                    p.galley(bounds.min, galley, accent);
                }
                if let Some(signal) = hovered_signal {
                    let painter = p.with_clip_rect(rect);
                    if let Some((component, _)) = crate::waveform::current_trace(&signal) {
                        if let Some(card) = parts
                            .iter()
                            .find(|card| card.name.eq_ignore_ascii_case(component))
                        {
                            draw(
                                &painter,
                                screen(self.positions[card.name]),
                                (card.part, card.value),
                                self.turns(card.name),
                                scale,
                                self.american_resistor,
                                (accent, paper),
                            );
                        }
                    } else if let Some(nodes) = signal
                        .strip_prefix("v(")
                        .and_then(|signal| signal.strip_suffix(')'))
                    {
                        let matches_node = |node: &str| {
                            nodes.split(',').any(|part| part.eq_ignore_ascii_case(node))
                        };
                        let mut pins: HashSet<_> = parts
                            .iter()
                            .flat_map(|card| {
                                card.part
                                    .terminals(self.positions[card.name], self.turns(card.name))
                                    .into_iter()
                                    .zip(card.nodes.iter().copied())
                            })
                            .filter_map(|(pin, node)| matches_node(node).then_some(pin))
                            .collect();
                        pins.extend(self.powers.iter().copied().filter(|&point| {
                            self.power_node(point, text).is_some_and(matches_node)
                        }));
                        if matches_node("0") {
                            pins.extend(self.grounds.iter().copied());
                        }
                        let mut remaining: Vec<_> = self.wires.iter().copied().collect();
                        // Junctions split wire segments at their shared endpoints.
                        // Seed with terminal positions to handle pins on imported segments too.
                        loop {
                            let mut found = false;
                            remaining.retain(|&(a, b)| {
                                if pins.contains(&a)
                                    || pins.contains(&b)
                                    || pins.iter().any(|&pin| on_wire(pin, (a, b)))
                                {
                                    pins.extend([a, b]);
                                    if egui::Rect::from_two_pos(screen(a), screen(b))
                                        .intersects(rect)
                                    {
                                        painter.line(
                                            vec![screen(a), screen(b)],
                                            Stroke::new(SCHEMATIC_STROKE + 1.5, accent),
                                        );
                                    }
                                    found = true;
                                    false
                                } else {
                                    true
                                }
                            });
                            if !found {
                                break;
                            }
                        }
                    }
                }
            }
            if let Some(target) = keyboard_delete {
                // Keep this frame complete. The next frame draws the edited schematic.
                let deleted = self.delete_objects(text, target);
                ui.ctx().request_repaint();
                return deleted;
            }
            let edit_power =
                r.secondary_clicked() && self.tool == Tool::Power && hovered_power.is_some();
            if self.new_comment.is_none()
                && (edit_power
                    || (self.tool == Tool::Cursor
                        && ((!probe_clicked && r.double_clicked()) || r.secondary_clicked())))
            {
                let name = hovered_label.or_else(|| match self.symbol_at(point) {
                    Some(Object::Part(n)) => {
                        self.positions.get_key_value(&n).map(|(n, _)| n.as_str())
                    }
                    _ => None,
                });
                if let Some(index) = mouse.and_then(comment_at) {
                    self.editing_comment = Some(CommentEdit {
                        index,
                        draft: self.comments[index].clone(),
                        focus: true,
                    });
                } else if let Some(point) = hovered_power {
                    self.editing_power = Some((
                        point,
                        self.power_voltages
                            .get(&point)
                            .map(ToString::to_string)
                            .unwrap_or_default(),
                    ));
                } else if let Some(c) = cards(text).into_iter().find(|c| Some(c.name) == name) {
                    self.cancel();
                    self.editing = Some(Properties {
                        part: c.part,
                        parasitics: parasitic_fields(c.part)
                            .iter()
                            .map(|(key, _)| {
                                parasitic_value(split_parasitics(c.part, c.value).1, key).to_owned()
                            })
                            .collect(),
                        winding_polarity: transformer_model(
                            c.value.split_whitespace().next().unwrap_or(""),
                        )
                        .1,
                        parameters: c
                            .part
                            .parameters()
                            .iter()
                            .map(|(key, _)| {
                                c.value
                                    .split_whitespace()
                                    .chain(c.part.parameter_defaults().split_whitespace())
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
                crate::ui::schematic_tool_cursor(ui, pos, rect, self.tool, self.light_canvas);
            }
            let mut starting_comment = false;
            match self.tool {
                _ if self.new_comment.is_some() => {}
                Tool::Comment if r.clicked() => {
                    self.new_comment = Some(Comment {
                        position: point,
                        text: String::new(),
                    });
                    self.tool = Tool::Cursor;
                    starting_comment = true;
                }
                Tool::Comment if r.hovered() => ui.ctx().set_cursor_icon(egui::CursorIcon::Text),
                Tool::Place(part) if inside => {
                    let (prefix, _, default_value) = part.spec();
                    let custom_value;
                    let default_value = if let Part::CustomTransformer { inputs, outputs } = part {
                        custom_value = format!("os_xfmr_multi_{inputs}_{outputs} ratio=1");
                        &custom_value
                    } else {
                        default_value
                    };
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
                        nodes: vec!["?"; part.pin_count()],
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
                        (accent, paper),
                    );
                    for pin in part.terminals(point, self.preview_turns) {
                        if !degree.contains_key(&pin)
                            && !self.wires.iter().any(|&wire| on_wire(pin, wire))
                        {
                            draw_open_pin(&p, screen(pin), scale, accent, paper);
                        }
                    }
                    obstacles.push(symbol_bounds(
                        part,
                        screen(point),
                        self.preview_turns,
                        scale,
                    ));
                    draw_labels(
                        &p,
                        screen(point),
                        &c,
                        scale,
                        &segments,
                        &mut obstacles,
                        label,
                    );
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
                            Stroke::new(SCHEMATIC_STROKE, accent),
                        ));
                        let mut route = self.routed_segments((a, elbow));
                        route.extend(self.routed_segments((elbow, point)));
                        for junction in self.preview_junctions(&route) {
                            p.circle(screen(junction), 5., accent, Stroke::new(1.5, paper));
                        }
                    }
                    if r.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::None);
                        let center = screen(point);
                        for axis in [vec2(11., 0.), vec2(0., 11.)] {
                            let line = [center - axis, center + axis];
                            p.line_segment(line, Stroke::new(3., paper));
                            p.line_segment(line, Stroke::new(1., accent));
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
                    power(point, self.preview_voltage, self.preview_turns, accent);
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
                    ground(point, accent);
                    if r.clicked() && !self.grounds.contains(&point) {
                        self.grounds.push(point);
                        changed = true;
                    }
                }
                Tool::Copy if let Some(anchor) = self.copy_anchor => {
                    if inside {
                        let delta = (point.0 - anchor.0, point.1 - anchor.1);
                        let offset = |(x, y): Point| (x + delta.0, y + delta.1);
                        for card in &parts {
                            if self.selected.contains(&Object::Part(card.name.into())) {
                                draw(
                                    &p,
                                    screen(offset(self.positions[card.name])),
                                    (card.part, card.value),
                                    self.turns(card.name),
                                    scale,
                                    self.american_resistor,
                                    (accent, paper),
                                );
                                if show_labels && let Some(bounds) = self.label_bounds(card.name) {
                                    let label_position =
                                        screen(delta) + bounds.min.to_vec2() * scale;
                                    let (name, value) = label_text(&p, card, scale, label);
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
                                        Stroke::new(SCHEMATIC_STROKE, accent),
                                    );
                                }
                                Object::Ground(index) => {
                                    ground(offset(self.grounds[index]), accent)
                                }
                                Object::Power(index) => {
                                    let point = self.powers[index];
                                    power(
                                        offset(point),
                                        self.power_voltages.get(&point).copied(),
                                        self.power_turns.get(&point).copied().unwrap_or(0),
                                        accent,
                                    );
                                }
                                Object::Comment(index) => {
                                    let (_, galley) = &comments[index];
                                    p.galley(
                                        screen(offset(self.comments[index].position)),
                                        galley.clone(),
                                        comment_color,
                                    );
                                }
                                Object::Part(_) | Object::Label(_) => {}
                            }
                        }
                        if r.clicked() {
                            annotations_only = self.selected.iter().all(Object::is_annotation);
                            self.selected
                                .retain(|object| !matches!(object, Object::Label(_)));
                            changed = self.place_copy(text, delta);
                        }
                    }
                }
                Tool::Erase | Tool::Copy if r.clicked() => {
                    if let Some(hit) = mouse.and_then(|pos| hit_at(self, pos)) {
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
                            annotations_only = self.selected.iter().all(Object::is_annotation);
                            if let Some(origin) = self.drag_origin.take() {
                                changed = origin != self.geometry();
                                self.translate_moved_labels(&origin);
                                keep_label_layout = true;
                            }
                            self.gesture = None;
                        }
                    } else if r.clicked()
                        && let Some(hit) = mouse.and_then(|pos| hit_at(self, pos))
                    {
                        if !self.selected.contains(&hit) {
                            self.selected.clear();
                            self.selected.insert(hit);
                        }
                        self.drag_origin = Some(self.geometry());
                        self.gesture = Some(Gesture::Move(point));
                    }
                }
                Tool::Cursor if captured_voltage_drag => {}
                Tool::Cursor | Tool::Select | Tool::Erase | Tool::Copy => {
                    if r.drag_started_by(egui::PointerButton::Primary) || r.clicked() {
                        let pos = ui
                            .input(|i| i.pointer.press_origin())
                            .or(mouse)
                            .unwrap_or(screen(point));
                        let q = snap(pos);
                        let hit = hit_at(self, pos);
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
                                Stroke::new(1., accent),
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
                                for (index, (bounds, _)) in comments.iter().enumerate() {
                                    if selection.intersects(*bounds) {
                                        self.selected.insert(Object::Comment(index));
                                    }
                                }
                                if self.tool != Tool::Copy {
                                    for (card, bounds) in &label_bounds {
                                        if selection.intersects(*bounds) {
                                            self.selected.insert(Object::Label(card.name.into()));
                                        }
                                    }
                                }
                                for (i, &(a, b)) in self.wires.iter().enumerate() {
                                    if contains(a) && contains(b) {
                                        self.selected.insert(Object::Wire(i));
                                    }
                                }
                            } else {
                                annotations_only = self.selected.iter().all(Object::is_annotation);
                                self.wires.append(&mut self.drag_leads);
                                if let Some(origin) = self.drag_origin.take() {
                                    changed = origin != self.geometry();
                                    self.translate_moved_labels(&origin);
                                }
                                keep_label_layout = true;
                            }
                            self.drag_origin = None;
                            self.gesture = None;
                            if self.tool == Tool::Erase {
                                return self.delete_objects(text, None);
                            }
                            if self.tool == Tool::Copy
                                && self
                                    .selected
                                    .iter()
                                    .any(|object| !matches!(object, Object::Label(_)))
                            {
                                self.copy_anchor = Some(point);
                                ui.ctx().request_repaint();
                            }
                        }
                    }
                }
                _ => {}
            }
            if let Some(mut draft) = self.new_comment.take() {
                use egui::{Event, Key, Modifiers};
                let (mut commit, mut cancel) = (false, false);
                ui.input_mut(|input| {
                    input.events.retain(|event| {
                        if let Event::Key {
                            key,
                            pressed: true,
                            modifiers,
                            ..
                        } = event
                        {
                            if *key == Key::Escape {
                                cancel = true;
                                return false;
                            }
                            if *key == Key::Enter && modifiers.matches_exact(Modifiers::NONE) {
                                commit = true;
                                return false;
                            }
                        }
                        true
                    });
                });
                let anchor = screen(draft.position);
                let mut editor =
                    ui.new_child(egui::UiBuilder::new().id_salt("inline comment").max_rect(
                        egui::Rect::from_min_max(anchor, rect.max.max(anchor + vec2(1., 1.))),
                    ));
                editor.set_clip_rect(rect);
                let font = FontId::proportional(COMMENT_FONT_SIZE * scale);
                let mut layout_comment = |ui: &Ui, text: &dyn egui::TextBuffer, _: f32| {
                    unwrapped_comment_layout(ui, text.as_str(), font.clone(), comment_color)
                };
                let response = editor.add(
                    egui::TextEdit::multiline(&mut draft.text)
                        .desired_rows(1)
                        .desired_width(180.)
                        .font(font.clone())
                        .layouter(&mut layout_comment)
                        .clip_text(true)
                        .text_color(comment_color)
                        .background_color(paper)
                        .margin(0)
                        .hint_text("Type a comment"),
                );
                if starting_comment {
                    response.request_focus();
                }
                if cancel || commit {
                    response.surrender_focus();
                }
                if !cancel {
                    if commit || response.lost_focus() {
                        let text = draft.text.trim();
                        if !text.is_empty() {
                            draft.text = text.to_owned();
                            self.comments.push(draft);
                            annotations_only = !changed;
                            changed = true;
                            ui.ctx().request_repaint();
                        }
                    } else {
                        self.new_comment = Some(draft);
                    }
                }
            }
        }

        if let Some((mut inputs, mut outputs)) = self.transformer_setup.take() {
            let dialog =
                crate::ui::dialog(ui.ctx(), "custom transformer", "Custom transformer", |ui| {
                    egui::Grid::new("transformer windings").show(ui, |ui| {
                        ui.label("Primary windings");
                        ui.add(
                            egui::DragValue::new(&mut inputs).range(1..=MAX_TRANSFORMER_WINDINGS),
                        );
                        ui.end_row();
                        ui.label("Secondary windings");
                        ui.add(
                            egui::DragValue::new(&mut outputs).range(1..=MAX_TRANSFORMER_WINDINGS),
                        );
                        ui.end_row();
                    });
                    crate::ui::dialog_actions(ui, "Place", true, true)
                });
            if dialog.inner {
                self.begin_placement(Part::CustomTransformer { inputs, outputs }, false);
            } else if !dialog.should_close() {
                self.transformer_setup = Some((inputs, outputs));
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

        if changed && !annotations_only {
            let layout = if keep_label_layout {
                self.label_layout.take()
            } else {
                None
            };
            self.sync(text);
            self.label_layout = layout;
        }
        if let Some(mut edit) = self.editing_comment.take() {
            let dialog =
                crate::ui::dialog(ui.ctx(), "schematic comment", "Schematic comment", |ui| {
                    let font = egui::TextStyle::Body.resolve(ui.style());
                    let color = ui.visuals().text_color();
                    let mut layout_comment = |ui: &Ui, text: &dyn egui::TextBuffer, _: f32| {
                        unwrapped_comment_layout(ui, text.as_str(), font.clone(), color)
                    };
                    let response = ui.add(
                        egui::TextEdit::multiline(&mut edit.draft.text)
                            .layouter(&mut layout_comment)
                            .clip_text(true)
                            .hint_text("Comment text")
                            .desired_width(ui.available_width())
                            .desired_rows(3),
                    );
                    if std::mem::take(&mut edit.focus) {
                        response.request_focus();
                    }
                    crate::ui::dialog_actions(ui, "Apply", !edit.draft.text.trim().is_empty(), true)
                });
            if dialog.inner {
                edit.draft.text = edit.draft.text.trim().to_owned();
                changed |= self.comments[edit.index] != edit.draft;
                self.comments[edit.index] = edit.draft;
            } else if !dialog.should_close() {
                self.editing_comment = Some(edit);
            }
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

fn probe_cursor(
    ui: &Ui,
    tip: Pos2,
    current: Option<egui::Vec2>,
    light: bool,
    voltage_color: Option<Color32>,
) {
    ui.ctx().set_cursor_icon(egui::CursorIcon::None);
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::unique("schematic probe cursor"),
    ));
    let ink = Color32::from_gray(if light { 35 } else { 228 });
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
    if let Some(direction) = current {
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
        let arrow = direction * 13.;
        let offset = if direction.x != 0. {
            vec2(0., 24.)
        } else {
            vec2(15., 7.)
        };
        painter.arrow(tip + offset - arrow * 0.5, arrow, Stroke::new(1.8, ink));
    } else {
        // The pointed end of the voltage probe is the measurement point.
        fill(&[(0., 0.), (2.5, -5.5), (5.5, -2.5)]);
        path(&[(5., -5.), (12., -12.)], 3.);
        let grip = [tip + vec2(14., -14.), tip + vec2(24., -24.)];
        path(&[(14., -14.), (24., -24.)], 8.);
        for end in grip {
            painter.circle_filled(end, 4., ink);
        }
        let accent = voltage_color.unwrap_or(Color32::from_rgb(112, 192, 211));
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

fn transformer_half_height(part: Part, inputs: u8, outputs: u8) -> f32 {
    let half_steps = if part == Part::CenterTappedTransformer {
        2 * WINDING_HALF_SPAN
    } else {
        3 * i32::from(inputs.max(outputs) - 1) + WINDING_HALF_SPAN
    };
    half_steps as f32 * GRID
}

fn symbol_bounds(part: Part, center: Pos2, turns: u8, scale: f32) -> egui::Rect {
    if let Some((inputs, outputs)) = part.transformer_windings() {
        let half_height = transformer_half_height(part, inputs, outputs);
        let size = vec2(2. * PIN_OFFSET + 5., 2. * half_height + 6.);
        let size = if turns.is_multiple_of(2) {
            size
        } else {
            vec2(size.y, size.x)
        };
        return egui::Rect::from_center_size(center, size * scale).expand(SCHEMATIC_STROKE / 2.);
    }
    let size = if part.pin_count() > 2 {
        vec2(2. * PIN_OFFSET + 5., 2. * PIN_OFFSET + 5.)
    } else if turns.is_multiple_of(2) {
        vec2(2. * PIN_OFFSET, 36.)
    } else {
        vec2(36., 2. * PIN_OFFSET)
    };
    egui::Rect::from_center_size(center, size * scale).expand(SCHEMATIC_STROKE / 2.)
}

fn unwrapped_comment_layout(
    ui: &Ui,
    text: &str,
    font: FontId,
    color: Color32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), font, color, f32::INFINITY);
    job.keep_trailing_whitespace = true;
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

fn label_move_offset(
    name: &str,
    positions: &HashMap<String, Point>,
    origin: &Geometry,
) -> egui::Vec2 {
    let (x, y) = positions[name];
    origin
        .positions
        .get(name)
        .map_or(egui::Vec2::ZERO, |&(old_x, old_y)| {
            vec2((x - old_x) as f32, (y - old_y) as f32) * GRID
        })
}

fn world_position((x, y): Point) -> Pos2 {
    egui::pos2(x as f32 * GRID, y as f32 * GRID)
}

fn draw_labels(
    p: &egui::Painter,
    center: Pos2,
    card: &Card<'_>,
    scale: f32,
    wires: &[[Pos2; 2]],
    occupied: &mut Vec<egui::Rect>,
    label: Color32,
) -> egui::Rect {
    let (name, value) = label_text(p, card, scale, label);
    let rect = place_label(
        center,
        scale,
        label_size(&name, &value),
        LABEL_CLEARANCE,
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
    color: Color32,
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
    let name = p.layout_no_wrap(
        card.name.into(),
        FontId::proportional(LABEL_NAME_SIZE * scale),
        color,
    );
    let value = p.layout_no_wrap(
        value.into(),
        FontId::proportional(LABEL_VALUE_SIZE * scale),
        color,
    );
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
    p.galley(pos, name, Color32::WHITE);
    p.galley(value_pos, value, Color32::WHITE);
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

fn draw_open_pin(p: &egui::Painter, pin: Pos2, scale: f32, color: Color32, paper: Color32) {
    let radius = 2. * scale;
    let width = SCHEMATIC_STROKE.min(radius);
    p.rect(
        egui::Rect::from_center_size(pin, vec2(radius * 2., radius * 2.)),
        0,
        paper,
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
    (color, paper): (Color32, Color32),
) {
    let transform = |x: f32, y: f32| {
        let (u, v) = orient_offset((x, y), turns);
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
                paper,
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
        Part::Transformer | Part::CenterTappedTransformer | Part::CustomTransformer { .. } => {
            let (inputs, outputs) = part.transformer_windings().unwrap();
            let tapped = part == Part::CenterTappedTransformer;
            let half_height = WINDING_HALF_SPAN as f32 * GRID;
            let core = transformer_half_height(part, inputs, outputs) + 2.;
            let polarity = transformer_model(value.split_whitespace().next().unwrap_or("")).1;
            let mut index = 0;
            for (sign, count) in [(-1., inputs), (1., outputs)] {
                let pin_x = sign * PIN_OFFSET;
                let tapped_secondary = tapped && sign > 0.;
                let (count, half_pitch) = if tapped_secondary {
                    (2, WINDING_HALF_SPAN)
                } else {
                    (count, 3)
                };
                let coil_x = sign * 13.;
                let dot_x = sign * 20.;
                for winding in winding_centers(count, half_pitch) {
                    let y = winding as f32 * GRID;
                    for end in [-1., 1.] {
                        let pin_y = y + end * half_height;
                        if !tapped_secondary || pin_y != 0. || winding < 0 {
                            path(&[(pin_x, pin_y), (coil_x, pin_y)]);
                        }
                    }
                    let coil: Vec<_> = (0..4)
                        .flat_map(|turn| {
                            (0..=10).map(move |i| {
                                let angle = i as f32 * std::f32::consts::PI / 10.;
                                (
                                    coil_x - sign * 6. * angle.sin(),
                                    y - half_height
                                        + half_height / 2.
                                            * (turn as f32 + (1. - angle.cos()) / 2.),
                                )
                            })
                        })
                        .collect();
                    path(&coil);
                    let dot_end = if polarity & (1 << index) == 0 {
                        -1.
                    } else {
                        1.
                    };
                    p.circle_filled(
                        transform(dot_x, y + dot_end * (half_height - 6.)),
                        2. * scale,
                        color,
                    );
                    index += 1;
                }
                body(&[(sign * 3., -core), (sign * 3., core)]);
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
                let fill = if paper == Color32::WHITE {
                    Color32::from_rgb(255, 250, 194)
                } else {
                    Color32::from_rgb(98, 82, 41)
                };
                p.circle(center, 14. * scale, fill, pen);
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
    fn restoring_older_transformer_layout_moves_connected_pins() {
        let deck = "Transformer\nX1 input 0 a tap b os_xfmr_ct ratio=1\n.end\n";
        for (marker, primary_x, primary_y) in [
            ("", 7, 4),
            ("extended_transformer_primaries:true,", 7, 3),
            ("wide_transformer_windings:true,", 8, 4),
        ] {
            let legacy: Geometry = ron::from_str(&format!(
                r#"({marker}rotations:{{"X1":1}},positions:{{"X1":(6,6)}},wires:[(({primary_x},{primary_y}),(9,{primary_y})),(({primary_x},8),(13,8))],grounds:[({},{primary_y})],powers:[])"#,
                12 - primary_x,
            ))
            .unwrap();
            let mut schematic = Schematic::default();
            schematic.restore(legacy, deck);

            assert_eq!(
                schematic.kinds["X1"].terminals((6, 6), 1),
                [(8, 4), (4, 4), (10, 8), (6, 8), (2, 8)]
            );
            assert_eq!(
                schematic.wires,
                [((8, 4), (9, primary_y)), ((10, 8), (13, 8))]
            );
            assert_eq!(schematic.grounds, [(4, 4)]);
            let migrated = schematic.geometry();
            schematic.restore(migrated.clone(), deck);
            assert!(schematic.geometry() == migrated);
        }
    }

    #[test]
    #[ignore = "requires ngspice on PATH"]
    fn reversed_transformer_dots_change_polarity_independently() {
        let mut deck = concat!(
            "Reversed transformer windings\nV1 in 0 4\n",
            "X1 in 0 out 0 os_xfmr_pol_1 ratio=2\n",
            "X2 in 0 high 0 low os_xfmr_ct_pol_4 ratio=2\n",
            "X3 in 0 a 0 b 0 os_xfmr_multi_1_2_pol_2 ratio=2\n",
            ".op\n.end\n",
        )
        .to_owned();
        for model in [
            "os_xfmr_pol_1",
            "os_xfmr_ct_pol_4",
            "os_xfmr_multi_1_2_pol_2",
        ] {
            ensure_model(&mut deck, model);
        }
        assert_eq!(cards(&deck).len(), 4);
        let run = crate::simulation::simulate(&deck);
        let wave = run.waveform.expect(&run.log);
        for (node, expected) in [
            ("out", -8.),
            ("high", 4.),
            ("low", 4.),
            ("a", -8.),
            ("b", 8.),
        ] {
            let voltage = format!("v({node})");
            let result = wave
                .traces
                .iter()
                .find(|trace| trace.name.eq_ignore_ascii_case(&voltage))
                .unwrap();
            assert!((result.samples[0] - expected).abs() < 1e-8, "{node}");
        }
    }

    #[test]
    #[ignore = "requires ngspice on PATH"]
    fn transformer_windings_preserve_polarity_and_reflect_load() {
        let mut deck = concat!(
            "Transformer windings\n",
            "V1 in 0 4\n",
            "X1 in 0 high 0 low os_xfmr_ct ratio=2\n",
            "R1 high low 1k\n",
            "V2 drive 0 3\n",
            "X2 open_a open_b drive 0 out_a 0 out_b 0 spare_a spare_b os_xfmr_multi_2_3 ratio=2\n",
            "R2 out_a 0 1k\nR3 out_b 0 1k\n",
            ".op\n.end\n",
        )
        .to_owned();
        ensure_model(&mut deck, "os_xfmr_ct");
        ensure_model(&mut deck, "os_xfmr_multi_2_3");
        let parts = cards(&deck);
        let custom = parts.iter().find(|card| card.name == "X2").unwrap();
        assert_eq!(custom.part.terminals((0, 0), 0).len(), 10);
        assert_eq!(custom.part.terminals((0, 0), 1)[0], (5, -2));

        let run = crate::simulation::simulate(&deck);
        let wave = run.waveform.expect(&run.log);
        for (name, expected) in [
            ("V(high)", 4.),
            ("V(low)", -4.),
            ("I(v1)", -0.016),
            ("V(out_a)", 6.),
            ("V(out_b)", 6.),
            ("I(v2)", -0.024),
        ] {
            let trace = wave
                .traces
                .iter()
                .find(|trace| trace.name.eq_ignore_ascii_case(name))
                .unwrap();
            assert!(
                (trace.samples[0] - expected).abs() < 1e-8,
                "{name}: {}",
                trace.samples[0]
            );
        }
    }

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
                    schematic.show(ui, &mut text, None, None);
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
                    schematic.show(ui, &mut deck, None, None);
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
            assert!(schematic.change_orientation(OrientationEdit::Rotate));
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
    fn mirrors_connected_transistor_and_preserves_terminal_order() {
        let mut text =
            "Mirror test\nX1 collector base 0 os_npn\nR1 collector base 1k\n.end\n".to_owned();
        let mut schematic = Schematic::default();
        schematic.import(&text);
        schematic.selected.insert(Object::Part("X1".into()));
        let (x, y) = schematic.positions["X1"];
        assert!(schematic.change_orientation(OrientationEdit::MirrorHorizontal));
        assert_eq!(
            Part::Npn.terminals((x, y), schematic.turns("X1")),
            [(x - 1, y - 2), (x + 2, y), (x - 1, y + 2)]
        );
        schematic.sync(&mut text);
        assert_eq!(cards(&text)[0].nodes, ["collector", "base", "0"]);

        assert!(schematic.change_orientation(OrientationEdit::MirrorVertical));
        schematic.sync(&mut text);
        assert_eq!(cards(&text)[0].nodes, ["collector", "base", "0"]);
        let geometry = ron::to_string(&schematic.geometry()).unwrap();
        let mut restored = Schematic::default();
        restored.restore(ron::from_str(&geometry).unwrap(), &text);
        assert_eq!(
            Part::Npn.terminals((x, y), restored.turns("X1")),
            [(x - 1, y + 2), (x + 2, y), (x - 1, y - 2)]
        );
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
