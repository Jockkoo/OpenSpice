use eframe::egui;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Analysis {
    #[default]
    Transient,
    Ac,
    Op,
}

fn directive(line: &str) -> Option<Analysis> {
    match line
        .split_whitespace()
        .next()?
        .to_ascii_lowercase()
        .as_str()
    {
        ".tran" => Some(Analysis::Transient),
        ".ac" => Some(Analysis::Ac),
        ".op" => Some(Analysis::Op),
        _ => None,
    }
}

pub fn number(token: &str) -> Option<f64> {
    let token = token.trim().to_ascii_lowercase();
    let split = token
        .find(|c: char| c.is_ascii_alphabetic() && c != 'e')
        .unwrap_or(token.len());
    let (base, suffix) = token.split_at(split);
    let scale = match suffix {
        "" => 1.,
        "t" => 1e12,
        "g" => 1e9,
        "meg" => 1e6,
        "k" => 1e3,
        "m" => 1e-3,
        "u" => 1e-6,
        "n" => 1e-9,
        "p" => 1e-12,
        "f" => 1e-15,
        _ => return None,
    };
    let n = base.parse::<f64>().ok()? * scale;
    n.is_finite().then_some(n)
}

const OPERATING_POINT_DISPLAY: &str = "* OpenSpice op-display ";

#[derive(Clone, Copy, Default)]
pub struct OperatingPointDisplay {
    pub voltages: bool,
    pub currents: bool,
}

pub struct Settings {
    pub analysis: Analysis,
    pub op_display: OperatingPointDisplay,
    step: String,
    stop: String,
    start: String,
    max_step: String,
    uic: bool,
    sweep: String,
    points: String,
    low: String,
    high: String,
}

impl Settings {
    pub fn read(deck: &str) -> Self {
        let mut settings = Self {
            analysis: Analysis::Transient,
            op_display: deck
                .lines()
                .skip(1)
                .find_map(|line| line.strip_prefix(OPERATING_POINT_DISPLAY))
                .map(|flags| OperatingPointDisplay {
                    voltages: flags.split_whitespace().any(|flag| flag == "voltages"),
                    currents: flags.split_whitespace().any(|flag| flag == "currents"),
                })
                .unwrap_or_default(),
            step: "5u".into(),
            stop: "21m".into(),
            start: String::new(),
            max_step: String::new(),
            uic: false,
            sweep: "dec".into(),
            points: "50".into(),
            low: "10".into(),
            high: "100k".into(),
        };
        for line in deck.lines().skip(1) {
            let Some(kind) = directive(line) else {
                continue;
            };
            settings.analysis = kind;
            let fields: Vec<_> = line.split_whitespace().skip(1).collect();
            match (kind, fields.as_slice()) {
                (Analysis::Transient, [step, stop, extra @ ..]) => {
                    settings.step = step.to_string();
                    settings.stop = stop.to_string();
                    let mut times = extra.iter().filter(|s| !s.eq_ignore_ascii_case("uic"));
                    settings.start = times.next().copied().unwrap_or("").into();
                    settings.max_step = times.next().copied().unwrap_or("").into();
                    settings.uic = extra.iter().any(|s| s.eq_ignore_ascii_case("uic"));
                }
                (Analysis::Ac, [sweep, points, low, high]) => {
                    settings.sweep = sweep.to_ascii_lowercase();
                    settings.points = points.to_string();
                    settings.low = low.to_string();
                    settings.high = high.to_string();
                }
                _ => {}
            }
            break;
        }
        settings
    }

    pub fn logarithmic_frequency(&self) -> bool {
        self.analysis == Analysis::Ac && matches!(self.sweep.as_str(), "dec" | "oct")
    }

    fn command(&self) -> Result<String, &'static str> {
        let positive = |s: &str| number(s).filter(|v| *v > 0.);
        match self.analysis {
            Analysis::Transient => {
                let (_, stop) = positive(&self.step)
                    .zip(positive(&self.stop))
                    .ok_or("Time step and stop time must be positive.")?;
                let mut fields = vec![".tran", self.step.trim(), self.stop.trim()];
                if !self.start.trim().is_empty() {
                    number(&self.start)
                        .filter(|v| *v >= 0. && *v < stop)
                        .ok_or("Start time must be between zero and stop time.")?;
                    fields.push(self.start.trim());
                } else if !self.max_step.trim().is_empty() {
                    fields.push("0");
                }
                if !self.max_step.trim().is_empty() {
                    positive(&self.max_step).ok_or("Maximum time step must be positive.")?;
                    fields.push(self.max_step.trim());
                }
                if self.uic {
                    fields.push("uic");
                }
                Ok(fields.join(" "))
            }
            Analysis::Ac => {
                if !matches!(self.sweep.as_str(), "lin" | "dec" | "oct") {
                    return Err("Choose a sweep type.");
                }
                self.points
                    .trim()
                    .parse::<u32>()
                    .ok()
                    .filter(|v| *v > 0)
                    .ok_or("Enter a positive whole number of points.")?;
                positive(&self.low)
                    .zip(positive(&self.high))
                    .filter(|(a, b)| a < b)
                    .ok_or("Enter positive frequencies with stop above start.")?;
                Ok(format!(
                    ".ac {} {} {} {}",
                    self.sweep,
                    self.points.trim(),
                    self.low.trim(),
                    self.high.trim()
                ))
            }
            Analysis::Op => Ok(".op".into()),
        }
    }

    pub fn apply(&self, deck: &str) -> Result<String, &'static str> {
        let command = self.command()?;
        let flags = format!(
            "{OPERATING_POINT_DISPLAY}{}{}",
            if self.op_display.voltages {
                "voltages "
            } else {
                ""
            },
            if self.op_display.currents {
                "currents"
            } else {
                ""
            }
        );
        let mut result = Vec::new();
        let mut inserted = false;
        for (index, line) in deck.lines().enumerate() {
            if index > 0 && (directive(line).is_some() || line.starts_with(OPERATING_POINT_DISPLAY))
            {
                continue;
            }
            if index > 0 && line.trim().eq_ignore_ascii_case(".end") && !inserted {
                result.push(command.as_str());
                inserted = true;
            }
            result.push(line);
            if index == 0 && (self.op_display.voltages || self.op_display.currents) {
                result.push(flags.trim_end());
            }
        }
        if !inserted {
            result.push(&command);
            result.push(".end");
        }
        Ok(result.join("\n") + "\n")
    }

    pub fn show(&mut self, ui: &mut egui::Ui) -> bool {
        ui.horizontal_wrapped(|ui| {
            for (kind, label) in [
                (Analysis::Transient, "Transient Analysis"),
                (Analysis::Ac, "AC Analysis"),
                (Analysis::Op, "DC Operating Point"),
            ] {
                ui.selectable_value(&mut self.analysis, kind, label);
            }
        });
        ui.separator();
        egui::Grid::new("analysis fields")
            .min_col_width(132.)
            .show(ui, |ui| match self.analysis {
                Analysis::Transient => {
                    crate::ui::dialog_field(ui, "Time step (s)", &mut self.step);
                    crate::ui::dialog_field(ui, "Stop time (s)", &mut self.stop);
                    crate::ui::dialog_field(ui, "Start saving at (s, optional)", &mut self.start);
                    crate::ui::dialog_field(ui, "Maximum step (s, optional)", &mut self.max_step);
                    ui.checkbox(&mut self.uic, "Use initial conditions");
                    ui.end_row();
                }
                Analysis::Ac => {
                    ui.label("Sweep type");
                    ui.horizontal(|ui| {
                        for (v, label) in [("lin", "Linear"), ("dec", "Decade"), ("oct", "Octave")]
                        {
                            ui.selectable_value(&mut self.sweep, v.into(), label);
                        }
                    });
                    ui.end_row();
                    crate::ui::dialog_field(
                        ui,
                        match self.sweep.as_str() {
                            "dec" => "Points per decade",
                            "oct" => "Points per octave",
                            _ => "Points",
                        },
                        &mut self.points,
                    );
                    crate::ui::dialog_field(ui, "Start frequency (Hz)", &mut self.low);
                    crate::ui::dialog_field(ui, "Stop frequency (Hz)", &mut self.high);
                }
                Analysis::Op => {
                    ui.checkbox(&mut self.op_display.voltages, "Show all node voltages");
                    ui.end_row();
                    ui.checkbox(&mut self.op_display.currents, "Show all component currents");
                    ui.end_row();
                }
            });
        let command = self.command();
        if let Err(error) = command {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        crate::ui::dialog_actions(ui, "Apply", command.is_ok(), true)
    }
}
