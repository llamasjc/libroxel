//! Help finding what's wrong in a game: the Output saved to a log file on
//! this computer, script errors grouped by where they happened, and a list
//! of the Roblox members and classes scripts asked for that Libroxel
//! doesn't have, with a report that can be copied in one click.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use libroxel_engine::{LogEntry, LogLevel};

/// How many session logs are kept; older ones are removed.
const KEEP_LOGS: usize = 20;

/// Where log files go.
pub fn logs_dir() -> PathBuf {
    crate::server::data_dir().join("logs")
}

/// The Output, saved as it grows to a file for this run of the app and to
/// `latest.log`.
#[derive(Default)]
pub struct LogFile {
    /// How many of the log's lines are written.
    written: usize,
    files: Vec<File>,
    path: Option<PathBuf>,
    failed: bool,
}

impl LogFile {
    /// This run's log file, once something has been written.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Writes the lines added to `log` since the last call.
    pub fn write_new(&mut self, log: &[LogEntry]) {
        if self.failed {
            return;
        }
        // The Output was cleared: carry on from its new start.
        if log.len() < self.written {
            self.written = 0;
            self.line("---- Output cleared ----");
        }
        if log.len() == self.written {
            return;
        }
        if self.files.is_empty() && !self.open() {
            self.failed = true;
            return;
        }
        let mut text = String::new();
        for entry in &log[self.written..] {
            text.push_str(&line_text(entry));
            text.push('\n');
        }
        self.written = log.len();
        for f in &mut self.files {
            let _ = f.write_all(text.as_bytes());
            let _ = f.flush();
        }
    }

    fn line(&mut self, text: &str) {
        for f in &mut self.files {
            let _ = writeln!(f, "{text}");
        }
    }

    fn open(&mut self) -> bool {
        let dir = logs_dir();
        if std::fs::create_dir_all(&dir).is_err() {
            return false;
        }
        tidy(&dir);
        let path = dir.join(format!("libroxel-{}.log", stamp()));
        let (Ok(own), Ok(latest)) = (File::create(&path), File::create(dir.join("latest.log"))) else {
            return false;
        };
        self.files = vec![own, latest];
        self.line(&format!("Libroxel {} log, started {}", env!("CARGO_PKG_VERSION"), stamp()));
        self.path = Some(path);
        true
    }
}

/// Removes the oldest session logs beyond [`KEEP_LOGS`].
fn tidy(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut logs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("libroxel-")))
        .collect();
    logs.sort();
    let extra = logs.len().saturating_sub(KEEP_LOGS - 1);
    for old in &logs[..extra] {
        let _ = std::fs::remove_file(old);
    }
}

/// The time now as `YYYY-MM-DD_HH-MM-SS` (UTC), sortable.
fn stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    // Days since 1970 to a calendar date (Howard Hinnant's method).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}_{:02}-{:02}-{:02}", rest / 3600, rest / 60 % 60, rest % 60)
}

/// One log line as shown in the Output.
pub fn line_text(entry: &LogEntry) -> String {
    let level = match entry.level {
        LogLevel::Error => "ERROR ",
        LogLevel::Warn => "WARN  ",
        _ => "",
    };
    format!("[{:>6.1}s] {level}{}", entry.time, entry.text)
}

/// One kind of error and how often it happened.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    /// The script it happened in, if the message says.
    pub script: Option<String>,
    /// The line, if the message says.
    pub line: Option<u32>,
    /// The first line of the message.
    pub message: String,
    /// The whole first message, with its stack trace.
    pub detail: String,
    pub count: usize,
    pub level: LogLevel,
}

/// Errors (and warnings) grouped by what they say, most frequent first.
pub fn problems(log: &[LogEntry]) -> Vec<Problem> {
    let mut found: Vec<Problem> = Vec::new();
    let mut index: BTreeMap<String, usize> = BTreeMap::new();
    for entry in log.iter().filter(|e| matches!(e.level, LogLevel::Error | LogLevel::Warn)) {
        let first = entry.text.lines().next().unwrap_or_default().trim().to_owned();
        match index.get(&first) {
            Some(&i) => found[i].count += 1,
            None => {
                let (script, line) = place_of(&entry.text);
                index.insert(first.clone(), found.len());
                found.push(Problem {
                    script,
                    line,
                    message: first,
                    detail: entry.text.clone(),
                    count: 1,
                    level: entry.level,
                });
            }
        }
    }
    // Errors before warnings, then the most frequent.
    found.sort_by_key(|p| (p.level != LogLevel::Error, std::cmp::Reverse(p.count)));
    found
}

/// The script and line an error message names: `Name: runtime error:
/// Name:12: …` or `Name:12: …`.
fn place_of(text: &str) -> (Option<String>, Option<u32>) {
    let first = text.lines().next().unwrap_or_default();
    // The script's own name comes first, before ": ".
    let script = first.split_once(": ").map(|(s, _)| s.trim().to_owned()).filter(|s| !s.contains(' '));
    // The first "Name:123:" anywhere in the message gives the line.
    let mut line = None;
    for word in text.split_whitespace() {
        let mut parts = word.split(':');
        if let (Some(name), Some(n)) = (parts.next(), parts.next())
            && !name.is_empty()
            && let Ok(n) = n.parse::<u32>()
            && script.as_deref().is_none_or(|s| s == name)
        {
            line = Some(n);
            break;
        }
    }
    (script, line)
}

/// A member or class scripts asked for that Libroxel doesn't have.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Missing {
    /// What it was asked of: a class or an object's name.
    pub on: String,
    pub member: String,
}

/// Every missing member or class the log names, with how many times.
pub fn missing(log: &[LogEntry]) -> BTreeMap<Missing, usize> {
    let mut out = BTreeMap::new();
    for entry in log.iter().filter(|e| e.level == LogLevel::Error) {
        for line in entry.text.lines() {
            if let Some(m) = missing_in(line) {
                *out.entry(m).or_default() += 1;
                break;
            }
        }
    }
    out
}

fn quoted(text: &str) -> Option<&str> {
    let start = text.find('\'')? + 1;
    let end = text[start..].find('\'')? + start;
    Some(&text[start..end])
}

fn missing_in(line: &str) -> Option<Missing> {
    if let Some(i) = line.find(" is not a valid member of ") {
        let member = line[..i].rsplit([' ', ':']).next()?.to_owned();
        // `Part "Workspace.Crate"`: the class is what matters.
        let rest = line[i + " is not a valid member of ".len()..].trim();
        let on = rest.split(" \"").next().unwrap_or(rest).to_owned();
        return Some(Missing { on, member });
    }
    if let Some(i) = line.find(" is not a valid class name") {
        let class = line[..i].rsplit([' ', ':', '"']).find(|s| !s.is_empty())?.to_owned();
        return Some(Missing { on: "Instance.new".into(), member: class });
    }
    if let Some(i) = line.find("attempt to call missing method ") {
        let rest = &line[i..];
        let member = quoted(rest)?.to_owned();
        let on = rest.rsplit(" of ").next()?.trim().to_owned();
        return Some(Missing { on, member });
    }
    if let Some(i) = line.find("attempt to set an unknown field ") {
        return Some(Missing { on: "(object)".into(), member: quoted(&line[i..])?.to_owned() });
    }
    if let Some(i) = line.find("attempt to index ") {
        let rest = &line[i + "attempt to index ".len()..];
        if let Some((on, member)) = rest.split_once(" with ") {
            if on == "nil" {
                return None;
            }
            return Some(Missing { on: on.to_owned(), member: member.trim().trim_matches('\'').to_owned() });
        }
    }
    None
}

/// A report to paste into a message: what's missing and every kind of
/// error, with the first one's stack trace.
pub fn report(log: &[LogEntry], game: &str, log_path: Option<&Path>) -> String {
    let mut out = format!("Libroxel {} report for {game}\n", env!("CARGO_PKG_VERSION"));
    if let Some(p) = log_path {
        out.push_str(&format!("Full log: {}\n", p.display()));
    }
    let problems = problems(log);
    let errors: usize = problems.iter().filter(|p| p.level == LogLevel::Error).map(|p| p.count).sum();
    let warnings: usize = problems.iter().filter(|p| p.level == LogLevel::Warn).map(|p| p.count).sum();
    out.push_str(&format!("{errors} errors, {warnings} warnings, {} kinds\n", problems.len()));
    let missing = missing(log);
    if !missing.is_empty() {
        out.push_str("\n== Missing in Libroxel ==\n");
        for (m, n) in &missing {
            out.push_str(&format!("{}.{}  (x{n})\n", m.on, m.member));
        }
    }
    if !problems.is_empty() {
        out.push_str("\n== Problems ==\n");
        for p in &problems {
            let at = match (&p.script, p.line) {
                (Some(s), Some(l)) => format!("{s}:{l}"),
                (Some(s), None) => s.clone(),
                _ => "-".into(),
            };
            let kind = if p.level == LogLevel::Error { "ERROR" } else { "WARN" };
            out.push_str(&format!("\n[{kind} x{}] at {at}\n{}\n", p.count, p.detail.trim_end()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(level: LogLevel, text: &str) -> LogEntry {
        LogEntry { level, time: 1.0, text: text.into() }
    }

    #[test]
    fn errors_are_grouped_and_placed() {
        let log = [
            entry(
                LogLevel::Error,
                "Bootstrap: runtime error: RunContext is not a valid member of Main\nstack traceback:\n\tBootstrap:48: in function 'x'",
            ),
            entry(
                LogLevel::Error,
                "Bootstrap: runtime error: RunContext is not a valid member of Main\nstack traceback:",
            ),
            entry(LogLevel::Error, "GroundServer: runtime error: GroundServer:29: require loop"),
            entry(LogLevel::Warn, "careful"),
            entry(LogLevel::Info, "hello"),
        ];
        let p = problems(&log);
        assert_eq!(p.len(), 3);
        assert_eq!((p[0].count, p[0].script.as_deref(), p[0].line), (2, Some("Bootstrap"), Some(48)));
        assert_eq!((p[1].script.as_deref(), p[1].line), (Some("GroundServer"), Some(29)));
        assert_eq!(p[2].level, LogLevel::Warn);
    }

    #[test]
    fn missing_members_are_named() {
        let log = [
            entry(
                LogLevel::Error,
                "A: runtime error: RunContext is not a valid member of Script \"Workspace.Main\"",
            ),
            entry(LogLevel::Error, "B: p86:1: attempt to call missing method 'ListKeysAsync' of DataStore"),
            entry(LogLevel::Error, "C: attempt to set an unknown field 'IgnoreWater'"),
            entry(LogLevel::Error, "D: attempt to index Mouse with 'TargetFilter'"),
            entry(LogLevel::Error, "E: attempt to index nil with 'Parent'"),
            entry(LogLevel::Error, "F: runtime error: Blob is not a valid class name"),
        ];
        let m: Vec<String> = missing(&log).keys().map(|m| format!("{}.{}", m.on, m.member)).collect();
        assert_eq!(
            m,
            [
                "(object).IgnoreWater",
                "DataStore.ListKeysAsync",
                "Instance.new.Blob",
                "Mouse.TargetFilter",
                "Script.RunContext"
            ]
        );
        let text = report(&log, "Test", None);
        assert!(text.contains("== Missing in Libroxel =="), "{text}");
    }

    #[test]
    fn stamps_sort_as_dates() {
        let s = stamp();
        assert_eq!(s.len(), 19, "{s}");
        assert!(s.starts_with("20"), "{s}");
    }
}
