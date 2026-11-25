//! Native-endian ngspice binary rawfiles.
use crate::analysis::Analysis;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quantity {
    Voltage,
    Current,
}

impl Quantity {
    pub fn unit(self) -> &'static str {
        match self {
            Self::Voltage => "V",
            Self::Current => "A",
        }
    }
}

#[derive(Debug)]
pub struct Trace {
    pub name: String,
    pub quantity: Quantity,
    pub samples: Vec<f64>,
}

#[derive(Debug)]
pub struct Waveform {
    pub analysis: Analysis,
    pub logarithmic_frequency: bool,
    pub time: Vec<f64>,
    pub traces: Vec<Trace>,
}

pub fn parse_raw(bytes: &[u8]) -> Result<Waveform, String> {
    let mut body = bytes;
    let mut analysis = None;
    let mut flags = "";
    let mut variables = None;
    let mut points = None;
    let mut in_table = false;
    let mut columns = Vec::new();
    loop {
        let end = body
            .iter()
            .position(|&b| b == b'\n')
            .ok_or("Missing binary rawfile header")?;
        let line = std::str::from_utf8(&body[..end])
            .map_err(|_| "Invalid rawfile header")?
            .trim();
        body = &body[end + 1..];
        if line == "Binary:" {
            break;
        }
        if in_table {
            let mut fields = line.split_whitespace();
            if fields.next().and_then(|v| v.parse::<usize>().ok()) != Some(columns.len()) {
                return Err("Invalid variable index in rawfile".into());
            }
            let name = fields.next().ok_or("Missing variable name")?;
            let kind = fields.next().ok_or("Missing variable type")?;
            columns.push((name, kind));
        } else if let Some((key, value)) = line.split_once(':') {
            let value = value.trim();
            match key {
                "Plotname" => {
                    analysis = match value.to_ascii_lowercase().as_str() {
                        "transient analysis" => Some(Analysis::Transient),
                        "ac analysis" => Some(Analysis::Ac),
                        "operating point" => Some(Analysis::Op),
                        _ => None,
                    }
                }
                "Flags" => flags = value,
                "No. Variables" => variables = value.parse().ok(),
                "No. Points" => points = value.parse::<usize>().ok(),
                "Variables" => in_table = true,
                _ => {}
            }
        }
    }
    let analysis = analysis.ok_or("Unsupported analysis in rawfile")?;
    let complex = analysis == Analysis::Ac;
    if !flags
        .split_whitespace()
        .any(|f| f == if complex { "complex" } else { "real" })
    {
        return Err("Unexpected rawfile encoding".into());
    }
    let n = points.filter(|&p| p > 0).ok_or("No samples in result")?;
    if columns.is_empty()
        || variables != Some(columns.len())
        || (analysis == Analysis::Transient && columns[0].1 != "time")
        || (complex && columns[0].1 != "frequency")
    {
        return Err("Invalid variable table".into());
    }
    let stride = if complex { 16 } else { 8 };
    let width = columns.len() * stride;
    if n.checked_mul(width) != Some(body.len()) {
        return Err("Unexpected rawfile size. Select one analysis per run.".into());
    }
    let mut channels: Vec<_> = columns
        .iter()
        .enumerate()
        .filter_map(|(index, (name, kind))| {
            let quantity = match *kind {
                "voltage" => Quantity::Voltage,
                "current" => Quantity::Current,
                _ => return None,
            };
            Some((
                index,
                Trace {
                    name: name
                        .strip_prefix("v(")
                        .map(|node| format!("V({node}"))
                        .or_else(|| name.strip_prefix("i(").map(|device| format!("I({device}")))
                        .unwrap_or_else(|| name.to_string()),
                    quantity,
                    samples: Vec::with_capacity(n),
                },
            ))
        })
        .collect();
    if channels.is_empty() {
        return Err("No voltage or current traces were saved".into());
    }
    let mut time = Vec::with_capacity(n);
    for row in body.chunks_exact(width) {
        // ngspice writes doubles in the local machine's byte order.
        let sample = |column: usize| -> Result<f64, String> {
            let value = f64::from_ne_bytes(row[column..column + 8].try_into().unwrap());
            if value.is_finite() {
                Ok(value)
            } else {
                Err("Non-finite waveform sample".into())
            }
        };
        let t = if analysis == Analysis::Op {
            0.
        } else {
            sample(0)?
        };
        if time.last().is_some_and(|prev| *prev > t) {
            return Err("Time vector is out of order".into());
        }
        time.push(t);
        for (index, trace) in &mut channels {
            let at = *index * stride;
            let real = sample(at)?;
            trace.samples.push(if complex {
                real.hypot(sample(at + 8)?)
            } else {
                real
            });
        }
    }
    Ok(Waveform {
        analysis,
        logarithmic_frequency: false,
        time,
        traces: channels.into_iter().map(|(_, trace)| trace).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw() -> Vec<u8> {
        let mut bytes = b"Plotname: Transient Analysis\nFlags: real\nNo. Variables: 3\nNo. Points: 2\nVariables:\n0 time time\n1 i(v1) current\n2 v(out) voltage\nBinary:\n".to_vec();
        for x in [0.0_f64, -0.1, 0.0, 0.001, -0.05, 3.16] {
            bytes.extend(x.to_ne_bytes());
        }
        bytes
    }

    #[test]
    fn reads_interleaved_time_and_voltages() {
        let data = parse_raw(&raw()).unwrap();
        assert_eq!(data.time, [0.0, 0.001]);
        assert_eq!(data.traces.len(), 2);
        assert_eq!(data.traces[0].name, "I(v1)");
        assert_eq!(data.traces[0].samples, [-0.1, -0.05]);
        assert_eq!(data.traces[1].name, "V(out)");
        assert_eq!(data.traces[1].samples, [0.0, 3.16]);
    }

    #[test]
    fn rejects_truncated_data() {
        let mut data = raw();
        data.pop();
        assert!(parse_raw(&data).is_err());
    }
}
