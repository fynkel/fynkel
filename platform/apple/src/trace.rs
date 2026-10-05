//! Opt-in, bounded single-viewport diagnostic. Off by default; no ABI changes.
use std::{
    fs::File,
    io::Write,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) struct FrameTrace(Mutex<(File, usize)>);
impl FrameTrace {
    pub(crate) fn from_env() -> Option<Self> {
        let path = std::env::var_os("FYNKEL_TRACE_FRAME_FILE")?;
        let mut path = std::path::PathBuf::from(path);
        if path.is_relative() {
            path = std::path::PathBuf::from(std::env::var_os("HOME")?).join(path);
        }
        let mut file = File::create(path).ok()?;
        writeln!(
            file,
            "time_us,event,generation,seq,slot,free,rendering,ready,presented,retiring,a,b,c"
        )
        .ok()?;
        Some(Self(Mutex::new((file, 0))))
    }

    pub(crate) fn record(
        &self,
        event: &str,
        generation: u64,
        seq: u64,
        slot: i32,
        counts: [usize; 5],
        values: [u64; 3],
    ) {
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros();
        let mut state = super::ring::lock(&self.0);
        if state.1 >= 16384 {
            return;
        }
        if writeln!(
            state.0,
            "{time},{event},{generation},{seq},{slot},{},{},{},{},{},{},{},{}",
            counts[0], counts[1], counts[2], counts[3], counts[4], values[0], values[1], values[2]
        )
        .is_err()
        {
            state.1 = 16384;
        } else {
            state.1 += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_trace_is_bounded_and_has_complete_records() {
        let path = std::env::temp_dir().join(format!(
            "fynkel-frame-trace-{}-{}.csv",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let trace = FrameTrace(Mutex::new((File::create(&path).unwrap(), 0)));
        for seq in 0..16390 {
            trace.record("ready", 2, seq, 1, [1, 1, 1, 1, 0], [10, 20, 30]);
        }
        let data = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<_> = data.lines().collect();
        assert_eq!(lines.len(), 16384);
        assert!(lines.iter().all(|s| s.split(',').count() == 13));
        assert_eq!(lines.last().unwrap().split(',').nth(3), Some("16383"));
        drop(trace);
        std::fs::remove_file(path).unwrap();
    }
}
