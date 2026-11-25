use eframe::egui;

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Dc,
    Sine,
    Pulse,
    Exp,
    Custom,
}

impl Kind {
    fn spec(
        self,
    ) -> (
        &'static str,
        &'static str,
        &'static [&'static str],
        &'static [&'static str],
    ) {
        match self {
            Self::Dc => ("DC", "DC", &["Voltage (V)"], &["5"]),
            Self::Sine => (
                "Sine",
                "SIN",
                &[
                    "Offset (V)",
                    "Amplitude (V)",
                    "Frequency (Hz)",
                    "Delay (s)",
                    "Damping (1/s)",
                    "Phase (°)",
                ],
                &["0", "1", "1k", "0", "0", "0"],
            ),
            Self::Pulse => (
                "Pulse",
                "PULSE",
                &[
                    "Initial voltage (V)",
                    "Pulsed voltage (V)",
                    "Delay (s)",
                    "Rise time (s)",
                    "Fall time (s)",
                    "Pulse width (s)",
                    "Period (s)",
                ],
                &["0", "5", "1m", "1u", "1u", "5m", "10m"],
            ),
            Self::Exp => (
                "Exponential",
                "EXP",
                &[
                    "Initial voltage (V)",
                    "Pulsed voltage (V)",
                    "Rise delay (s)",
                    "Rise time constant (s)",
                    "Fall delay (s)",
                    "Fall time constant (s)",
                ],
                &["0", "5", "1m", "100u", "5m", "200u"],
            ),
            Self::Custom => (
                "Custom SPICE",
                "",
                &["Source definition"],
                &["PWL(0 0 1m 5)"],
            ),
        }
    }

    pub fn detect(value: &str) -> Self {
        let value = value.trim();
        let head = value.split(['(', ' ', '\t']).next().unwrap_or("");
        for kind in [Self::Pulse, Self::Sine, Self::Exp, Self::Dc] {
            if head.eq_ignore_ascii_case(kind.spec().1) {
                return kind;
            }
        }
        if value.is_empty()
            || (value.split_whitespace().count() == 1
                && value.starts_with(|c: char| c.is_ascii_digit() || "+-.".contains(c)))
        {
            Self::Dc
        } else {
            Self::Custom
        }
    }
}

pub struct Source {
    kind: Kind,
    fields: Vec<String>,
    ac: String,
    phase: String,
}

impl Source {
    fn defaults(kind: Kind) -> Self {
        Self {
            kind,
            ac: String::new(),
            phase: String::new(),
            fields: kind.spec().3.iter().map(|v| v.to_string()).collect(),
        }
    }
    pub fn parse(value: &str) -> Self {
        let mut words: Vec<_> = value.split_whitespace().collect();
        let mut ac = String::new();
        let mut phase = String::new();
        if let Some(i) = words.iter().position(|s| s.eq_ignore_ascii_case("ac")) {
            let count = words[i + 1..]
                .iter()
                .take(2)
                .take_while(|s| crate::analysis::number(s).is_some())
                .count();
            ac = words
                .get(i + 1)
                .filter(|_| count > 0)
                .copied()
                .unwrap_or("1")
                .into();
            if count == 2 {
                phase = words[i + 2].into();
            }
            words.drain(i..=i + count);
        }
        let waveform = words.join(" ");
        let value = if waveform.is_empty() {
            "DC 0"
        } else {
            &waveform
        };
        let kind = Kind::detect(value);
        let value = value.trim();
        let args = if kind == Kind::Dc {
            Some(
                value
                    .split_whitespace()
                    .filter(|s| !s.eq_ignore_ascii_case("dc"))
                    .collect::<Vec<_>>(),
            )
        } else {
            value
                .split_once('(')
                .and_then(|(_, rest)| rest.strip_suffix(')'))
                .map(|s| {
                    s.split(|c: char| c == ',' || c.is_whitespace())
                        .filter(|v| !v.is_empty())
                        .collect()
                })
        };
        if kind != Kind::Custom
            && let Some(args) = args
            && !args.is_empty()
            && args.len() <= kind.spec().2.len()
            && args.iter().all(|a| !a.contains(['(', ')']))
        {
            let mut fields = args.into_iter().map(str::to_owned).collect::<Vec<_>>();
            fields.resize(kind.spec().2.len(), String::new());
            Self {
                kind,
                fields,
                ac,
                phase,
            }
        } else {
            Self {
                kind: Kind::Custom,
                ac,
                phase,
                fields: vec![value.into()],
            }
        }
    }
    pub fn definition(&self) -> String {
        let args = self
            .fields
            .iter()
            .map(|f| f.trim())
            .collect::<Vec<_>>()
            .join(" ");
        let mut definition = match self.kind {
            Kind::Custom => args,
            Kind::Dc => format!("DC {}", args.trim()),
            kind => format!("{}({})", kind.spec().1, args.trim_end()),
        };
        if !self.ac.trim().is_empty() {
            definition.push_str(&format!(" AC {}", self.ac.trim()));
            if !self.phase.trim().is_empty() {
                definition.push_str(&format!(" {}", self.phase.trim()));
            }
        }
        definition
    }
    pub fn valid(&self) -> bool {
        let count = self
            .fields
            .iter()
            .take_while(|f| !f.trim().is_empty())
            .count();
        (self.ac.trim().is_empty() || crate::analysis::number(&self.ac).is_some())
            && (self.phase.trim().is_empty()
                || (!self.ac.trim().is_empty() && crate::analysis::number(&self.phase).is_some()))
            && count > 0
            && self.fields[count..].iter().all(|f| f.trim().is_empty())
    }
    pub fn show(&mut self, ui: &mut egui::Ui, current: bool) {
        let mut kind = self.kind;
        crate::ui::component_field_label(ui, "Source type");
        egui::ComboBox::from_id_salt("source type")
            .width(crate::ui::COMPONENT_FIELD_WIDTH)
            .selected_text(kind.spec().0)
            .show_ui(ui, |ui| {
                for k in [Kind::Dc, Kind::Sine, Kind::Pulse, Kind::Exp, Kind::Custom] {
                    ui.selectable_value(&mut kind, k, k.spec().0);
                }
            });
        ui.end_row();
        if kind != self.kind {
            self.kind = kind;
            self.fields = Self::defaults(kind).fields;
        }
        for (label, field) in self.kind.spec().2.iter().zip(&mut self.fields) {
            let label = if current {
                label
                    .replace("Voltage", "Current")
                    .replace("voltage", "current")
                    .replace("(V)", "(A)")
            } else {
                (*label).into()
            };
            crate::ui::component_field(ui, &label, field);
        }
        let magnitude = if current {
            "AC magnitude (A)"
        } else {
            "AC magnitude (V)"
        };
        crate::ui::component_field(ui, magnitude, &mut self.ac);
        crate::ui::component_field(ui, "AC phase (°)", &mut self.phase);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires ngspice on PATH"]
    fn source_types_drive_rlc_filter() {
        let example = crate::simulation::EXAMPLE;
        for kind in [Kind::Dc, Kind::Sine, Kind::Pulse, Kind::Exp, Kind::Custom] {
            let definition = Source::defaults(kind).definition();
            assert_eq!(Source::parse(&definition).definition(), definition);
            let run = crate::simulation::simulate(
                &example.replace("PULSE(0 5 1m 1u 1u 5m 10m)", &definition),
            );
            let wave = run.waveform.expect(&run.log);
            let out = &wave
                .traces
                .iter()
                .find(|t| t.name == "V(out)")
                .unwrap()
                .samples;
            if kind == Kind::Pulse {
                assert!(out.iter().any(|&v| v > 6.));
            }
            if kind == Kind::Dc {
                assert!(out.iter().all(|&v| (v - 5.).abs() < 1e-6));
            }
        }
    }
}
