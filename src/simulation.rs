use crate::waveform::{Waveform, parse_raw};
use std::{fs, io::ErrorKind, process::Command};

pub const EXAMPLE: &str = include_str!("../examples/rlc-low-pass.cir");

pub struct Run {
    pub waveform: Result<Waveform, String>,
    pub log: String,
}

#[cfg(test)]
pub fn simulate(netlist: &str) -> Run {
    controlled(netlist, &Control::default(), |_, _| {})
}

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
#[derive(Default)]
pub struct Control {
    pub mode: AtomicU8,
    pub live: AtomicBool,
}

pub fn controlled(
    netlist: &str,
    control: &Control,
    mut update: impl FnMut(f64, Option<Waveform>),
) -> Run {
    let mut log = String::new();
    let waveform = run(netlist, &mut log, control, &mut update);
    Run { waveform, log }
}

fn ngspice_command() -> Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;

        let bundled = std::env::current_exe()
            .ok()
            .and_then(|exe| {
                Some(
                    exe.parent()?
                        .join("Spice64")
                        .join("bin")
                        .join("ngspice_con.exe"),
                )
            })
            .filter(|exe| exe.is_file());
        let mut command = match bundled {
            Some(exe) => Command::new(exe),
            None => Command::new("ngspice_con.exe"),
        };
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        command
    }
    #[cfg(not(target_os = "windows"))]
    Command::new("ngspice")
}

fn run(
    netlist: &str,
    log: &mut String,
    control: &Control,
    update: &mut impl FnMut(f64, Option<Waveform>),
) -> Result<Waveform, String> {
    let dir = tempfile::tempdir().map_err(|e| format!("Cannot create run directory: {e}"))?;
    let (deck, sensors) = crate::schematic::instrument_currents(netlist);
    fs::write(dir.path().join("circuit.cir"), deck)
        .map_err(|e| format!("Cannot write netlist: {e}"))?;
    let output_path = dir.path().join("run.log");
    let output_file = fs::File::create(&output_path).map_err(|e| e.to_string())?;
    let raw = dir.path().join("wave.raw");
    let mut child = ngspice_command()
        .stderr(output_file.try_clone().map_err(|e| e.to_string())?)
        .stdout(output_file)
        .current_dir(dir.path())
        .args(["-n", "-b", "-r", "wave.raw", "circuit.cir"])
        .spawn()
        .map_err(|e| match e.kind() {
            ErrorKind::NotFound => {
                if cfg!(target_os = "windows") {
                    "ngspice_con.exe was not found. Put the extracted Spice64 folder next to OpenSpice.exe or add its bin folder to PATH.".into()
                } else {
                    "ngspice was not found on PATH. On Arch, install it with sudo pacman -S ngspice.".into()
                }
            }
            _ => format!("Cannot start ngspice: {e}"),
        })?;
    let mut paused = false;
    let mut last = std::time::Instant::now();
    let mut stopped = false;
    let stop_time = netlist
        .lines()
        .map(str::split_whitespace)
        .find_map(|mut f| {
            f.next()?
                .eq_ignore_ascii_case(".tran")
                .then(|| crate::analysis::number(f.nth(1)?))
                .flatten()
        });
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        let mode = control.mode.load(Ordering::Relaxed);
        if mode == 2 {
            stopped = true;
            let _ = child.kill();
            break child.wait().map_err(|e| e.to_string())?;
        }
        let transition = paused != (mode == 1);
        if transition {
            suspend(&child, mode == 1)?;
            paused = mode == 1;
        }
        if (transition || !paused && last.elapsed().as_millis() >= 450)
            && let Ok(bytes) = fs::read(&raw)
            && let Ok(mut wave) = crate::waveform::parse_samples(&bytes, true)
        {
            let progress = stop_time
                .zip(wave.time.last())
                .map_or(0., |(end, t)| (t / end).clamp(0., 1.));
            normalize(&mut wave, &sensors, netlist);
            update(
                progress,
                (paused || control.live.load(Ordering::Relaxed)).then_some(wave),
            );
            last = std::time::Instant::now();
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    *log = fs::read_to_string(output_path).unwrap_or_default();
    if stopped {
        let bytes = fs::read(raw).map_err(|_| "Stopped before the first sample")?;
        let mut wave = crate::waveform::parse_samples(&bytes, true)?;
        normalize(&mut wave, &sensors, netlist);
        return Ok(wave);
    }
    if !status.success() {
        return Err(format!("ngspice exited with {status}"));
    }
    if log.lines().any(|line| {
        let line = line.trim().to_ascii_lowercase();
        line.starts_with("error:")
            || line.contains("simulation(s) aborted")
            || line.contains("simulation interrupted")
    }) {
        return Err("ngspice reported a simulation error. See the output below.".into());
    }
    let bytes = fs::read(dir.path().join("wave.raw"))
        .map_err(|e| format!("Cannot read waveform data: {e}. Check the analysis directive."))?;
    let mut wave = parse_raw(&bytes)?;
    if let Some(end) = stop_time
        && wave.time.last().is_none_or(|t| *t < end * (1. - 1e-8))
    {
        return Err(format!(
            "Transient analysis ended before the requested {end} s. See ngspice output."
        ));
    }
    normalize(&mut wave, &sensors, netlist);
    Ok(wave)
}

fn normalize(wave: &mut Waveform, sensors: &[(String, String)], netlist: &str) {
    for (node, component) in sensors {
        let internal_voltages = [
            format!("V({node})"),
            format!("V({node}_rs)"),
            format!("V({node}_ls)"),
        ];
        wave.traces
            .retain(|trace| !internal_voltages.contains(&trace.name));
        let sensor = format!("I(v{node})");
        if let Some(trace) = wave.traces.iter_mut().find(|trace| trace.name == sensor) {
            trace.name = format!("I({component})");
        }
    }
    wave.logarithmic_frequency = crate::analysis::Settings::read(netlist).logarithmic_frequency();
}

#[cfg(unix)]
fn suspend(child: &std::process::Child, pause: bool) -> Result<(), String> {
    let status = Command::new("kill")
        .args([
            if pause { "-STOP" } else { "-CONT" },
            &child.id().to_string(),
        ])
        .status()
        .map_err(|e| e.to_string())?;
    status
        .success()
        .then_some(())
        .ok_or("Could not change simulation state".into())
}
#[cfg(windows)]
fn suspend(child: &std::process::Child, pause: bool) -> Result<(), String> {
    use std::os::windows::io::AsRawHandle;
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtSuspendProcess(handle: *mut std::ffi::c_void) -> i32;
        fn NtResumeProcess(handle: *mut std::ffi::c_void) -> i32;
    }
    let status = unsafe {
        if pause {
            NtSuspendProcess(child.as_raw_handle())
        } else {
            NtResumeProcess(child.as_raw_handle())
        }
    };
    (status >= 0)
        .then_some(())
        .ok_or("Could not change simulation state".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires ngspice on PATH"]
    fn transistor_parameters_apply_per_instance() {
        let library = include_str!("../assets/components.lib");
        for (kind, supply, bias, parameter) in [
            ("npn", 5., 0.65, "is=2e-16"),
            ("pnp", -5., -0.65, "is=2e-16"),
            ("nmos", 5., 3., "kp=2m"),
            ("pmos", -5., -3., "kp=2m"),
            ("njfet", 5., 0., "beta=2m"),
            ("pjfet", -5., 0., "vto=-2"),
        ] {
            let deck = format!(
                "Transistor parameters
V1 d 0 {supply}
V2 g 0 {bias}
                 X1 d g 0 os_{kind}
X2 d g 0 os_{kind}_adjust
                 X3 d g 0 os_{kind}_adjust {parameter}
{library}
.op
.end"
            );
            let run = simulate(&deck);
            let wave = run.waveform.expect(&run.log);
            let current = |name| {
                wave.traces
                    .iter()
                    .find(|trace| trace.name == name)
                    .unwrap()
                    .samples[0]
            };
            let default = current("I(x1)");
            if kind == "pjfet" {
                assert!((current("I(x2)") - default).abs() < 1e-12);
                assert!(current("I(x3)").abs() > 1e-3);
                continue;
            }
            assert!(default.abs() > 1e-9, "{kind}");
            assert!((current("I(x2)") / default - 1.).abs() < 1e-6, "{kind}");
            assert!((current("I(x3)") / default - 2.).abs() < 0.01, "{kind}");
        }
    }

    #[test]
    #[ignore = "requires ngspice on PATH"]
    fn single_pole_bandwidth_and_slew() {
        let library = include_str!("../assets/components.lib");
        let ac = format!(
            "Bandwidth\nV1 in 0 AC 1\nVhi hi 0 15\nVlo lo 0 -15\nX1 in 0 out hi lo os_pole_ext gain=2000 gbw=2meg slew=4meg\n{library}\n.ac lin 3 1k 2meg\n.end"
        );
        let run = simulate(&ac);
        let wave = run.waveform.expect(&run.log);
        let output = &wave
            .traces
            .iter()
            .find(|t| t.name == "V(out)")
            .unwrap()
            .samples;
        assert!((output[0] - 2000. / 2_f64.sqrt()).abs() < 0.01);
        assert!((output[2] - 1.).abs() < 1e-5);

        let transient = format!(
            "Slew\nV1 in 0 PULSE(0 1 1u 1n 1n 10u 20u)\nVhi hi 0 15\nVlo lo 0 -15\nX1 in out out hi lo os_pole_ext gain=2000 gbw=2meg slew=500k\n{library}\n.tran 10n 5u\n.end"
        );
        let run = simulate(&transient);
        let wave = run.waveform.expect(&run.log);
        let output = &wave
            .traces
            .iter()
            .find(|t| t.name == "V(out)")
            .unwrap()
            .samples;
        let a = wave.time.partition_point(|&t| t < 1.2e-6);
        let b = wave.time.partition_point(|&t| t < 1.8e-6);
        let slope = (output[b] - output[a]) / (wave.time[b] - wave.time[a]);
        assert!((slope / 500_000. - 1.).abs() < 0.01);
    }

    #[test]
    #[ignore = "requires ngspice on PATH"]
    fn component_library_operating_points() {
        let deck = format!(
            "Library devices
V1 in 0 1
V2 hi 0 5
V3 lo 0 -5
X1 in diode os_diode
R1 diode 0 1k
X2 hi 0 0 os_npn
X3 lo 0 0 os_pnp
X4 hi 0 0 os_nmos
X5 lo 0 0 os_pmos
X6 hi 0 0 os_njfet
X7 lo 0 0 os_pjfet
X8 in 0 os_cap cap=47u
X9 in 0 ideal os_op_adjust vpos=8 vneg=-2
X10 0 in rail hi lo os_oprail
X11 in feedback feedback hi lo os_pole_ext gain=1000 gbw=2meg slew=3meg
X12 in 0 secondary 0 os_xfmr ratio=3
R2 secondary 0 1k
{}
.op
.end",
            include_str!("../assets/components.lib")
        );
        let run = simulate(&deck);
        let wave = run.waveform.expect(&run.log);
        let sample = |name: &str| wave.traces.iter().find(|t| t.name == name).unwrap().samples[0];
        for (name, target) in [
            ("V(ideal)", 8.),
            ("V(rail)", -5.),
            ("V(feedback)", 1000. / 1001.),
            ("V(secondary)", 3.),
            ("I(x12)", 0.009),
        ] {
            assert!((sample(name) - target).abs() < 1e-6, "{name}");
        }
        assert!(sample("I(x1)") > 0.);
        for n in 1..=12 {
            assert!(sample(&format!("I(x{n})")).is_finite());
        }
    }

    #[test]
    #[ignore = "requires ngspice on PATH"]
    fn rc_time_constant() {
        let run = simulate(include_str!("../examples/rc-low-pass.cir"));
        let wave = run.waveform.expect(&run.log);
        let values = |name| &wave.traces.iter().find(|s| s.name == name).unwrap().samples;
        let input = values("V(in)");
        let output = values("V(out)");
        let edge = input.iter().position(|&v| v >= 2.5).unwrap();
        let target = wave.time[edge] + 0.001;
        let i = wave.time.partition_point(|&t| t < target);
        let f = (target - wave.time[i - 1]) / (wave.time[i] - wave.time[i - 1]);
        let fraction = (output[i - 1] + f * (output[i] - output[i - 1])) / input[i];
        println!(
            "Output at one time constant: {:.2}% of step",
            fraction * 100.0
        );
        assert!((fraction - (1.0 - (-1.0_f64).exp())).abs() < 0.01);
    }

    #[test]
    #[ignore = "requires ngspice on PATH"]
    fn rc_frequency_response_and_operating_point() {
        let deck =
            "RC filter\nV1 in 0 DC 3 AC 2 45\nR1 in out 1k\nC1 out 0 1u\n.ac lin 3 100 200\n.end\n";
        for sweep in ["lin 3", "oct 8", "dec 20"] {
            let run = simulate(&deck.replace("lin 3", sweep));
            let wave = run.waveform.expect(&run.log);
            let output = wave.traces.iter().find(|t| t.name == "V(out)").unwrap();
            assert!(wave.time.len() >= 3);
            let current = wave.traces.iter().find(|t| t.name == "I(c1)").unwrap();
            for (&freq, (&v, &amps)) in wave
                .time
                .iter()
                .zip(output.samples.iter().zip(&current.samples))
            {
                assert!((amps - std::f64::consts::TAU * freq * 1e-6 * v).abs() < 1e-10);
            }
            for (&freq, &v) in wave.time.iter().zip(&output.samples) {
                let expected = 2. / (1. + (std::f64::consts::TAU * freq * 0.001).powi(2)).sqrt();
                assert!((v - expected).abs() < 1e-8);
            }
        }
        let run = simulate(&deck.replace(".ac lin 3 100 200", ".op"));
        let wave = run.waveform.expect(&run.log);
        assert_eq!(wave.analysis, crate::analysis::Analysis::Op);
        let trace = |name: &str| wave.traces.iter().find(|t| t.name == name).unwrap().samples[0];
        assert!((trace("V(out)") - 3.).abs() < 1e-9);
        assert!(trace("I(r1)").abs() < 1e-9);
    }
}
