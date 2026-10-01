//! Backups of the data file: copies in `<app data>/backups/`, named
//! `fin-<YYYYMMDDTHHMMSSZ>-<reason>.json`. The name carries the timestamp and reason,
//! so listing needs no index file.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use fin_shared::BackupInfo;

pub const DAILY: &str = "dagelijks";
pub const MANUAL: &str = "handmatig";
pub const BEFORE_IMPORT: &str = "voor-import";
pub const BEFORE_RESTORE: &str = "voor-herstel";

/// Automatic backups (everything but manual ones) kept per data file.
const KEEP_AUTOMATIC: usize = 30;

pub fn dir_for(data_file: &Path) -> PathBuf {
    data_file.with_file_name("backups")
}

/// UTC civil date and time from a Unix timestamp (Howard Hinnant's days-from-civil, inverted).
fn civil(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, (rem / 3600) as u32, (rem % 3600 / 60) as u32, (rem % 60) as u32)
}

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// `2026-09-30T14:35:00Z`
pub fn rfc3339(secs: u64) -> String {
    let (y, mo, d, h, mi, s) = civil(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// `20260930T143500Z`, safe in file names.
fn compact(secs: u64) -> String {
    let (y, mo, d, h, mi, s) = civil(secs);
    format!("{y:04}{mo:02}{d:02}T{h:02}{mi:02}{s:02}Z")
}

fn parse_name(name: &str) -> Option<(String, String)> {
    let rest = name.strip_prefix("fin-")?.strip_suffix(".json")?;
    let (stamp, reason) = rest.split_once('-')?;
    let b = stamp.as_bytes();
    if b.len() != 16 || b[8] != b'T' || b[15] != b'Z' {
        return None;
    }
    let created = format!(
        "{}-{}-{}T{}:{}:{}Z",
        &stamp[0..4],
        &stamp[4..6],
        &stamp[6..8],
        &stamp[9..11],
        &stamp[11..13],
        &stamp[13..15]
    );
    Some((created, reason.to_string()))
}

/// A backup name as the UI sends it back: only names this module produces, never a path.
pub fn is_valid_name(name: &str) -> bool {
    !name.contains(['/', '\\']) && !name.contains("..") && parse_name(name).is_some()
}

/// Copies the data file into the backups dir. `Ok(None)` when there is no data file yet.
pub fn create(data_file: &Path, reason: &str, secs: u64) -> Result<Option<BackupInfo>, String> {
    if !data_file.exists() {
        return Ok(None);
    }
    let dir = dir_for(data_file);
    fs::create_dir_all(&dir).map_err(|e| format!("Could not create the backup folder: {e}"))?;
    let name = format!("fin-{}-{reason}.json", compact(secs));
    let target = dir.join(&name);
    fs::copy(data_file, &target).map_err(|e| format!("Backup failed: {e}"))?;
    prune(&dir)?;
    let size_bytes = fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
    Ok(Some(BackupInfo { name, created: rfc3339(secs), reason: reason.into(), size_bytes }))
}

/// Newest first.
pub fn list(data_file: &Path) -> Result<Vec<BackupInfo>, String> {
    let dir = dir_for(data_file);
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("Could not read the backups: {e}")),
    };
    let mut out: Vec<BackupInfo> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let (created, reason) = parse_name(&name)?;
            let size_bytes = e.metadata().map(|m| m.len()).unwrap_or(0);
            Some(BackupInfo { name, created, reason, size_bytes })
        })
        .collect();
    out.sort_by(|a, b| b.created.cmp(&a.created).then(b.name.cmp(&a.name)));
    Ok(out)
}

/// True when there is already a daily backup from this UTC day.
pub fn has_daily_for(data_file: &Path, secs: u64) -> bool {
    let day = &rfc3339(secs)[..10];
    list(data_file)
        .map(|l| l.iter().any(|b| b.reason == DAILY && b.created.starts_with(day)))
        .unwrap_or(false)
}

pub fn read(data_file: &Path, name: &str) -> Result<Vec<u8>, String> {
    if !is_valid_name(name) {
        return Err("Invalid backup name".into());
    }
    fs::read(dir_for(data_file).join(name)).map_err(|e| format!("Could not read the backup: {e}"))
}

fn prune(dir: &Path) -> Result<(), String> {
    let fake_data_file = dir.with_file_name("fin.json");
    let automatic: Vec<BackupInfo> =
        list(&fake_data_file)?.into_iter().filter(|b| b.reason != MANUAL).collect();
    for old in automatic.iter().skip(KEEP_AUTOMATIC) {
        let _ = fs::remove_file(dir.join(&old.name));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_timestamps() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_790_778_540), "2026-09-30T14:29:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(compact(1_790_778_540), "20260930T142900Z");
    }

    #[test]
    fn names_round_trip_and_reject_paths() {
        let name = format!("fin-{}-{}.json", compact(1_790_778_540), BEFORE_IMPORT);
        assert_eq!(parse_name(&name), Some(("2026-09-30T14:29:00Z".into(), BEFORE_IMPORT.into())));
        assert!(is_valid_name(&name));
        for bad in ["../fin.json", "fin-x.json", "/etc/passwd", "fin-20260930T142900Z-a/../../b.json", "fin.json"] {
            assert!(!is_valid_name(bad), "{bad}");
        }
    }

    #[test]
    fn create_list_prune() {
        let dir = std::env::temp_dir().join(format!("fin-backup-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("fin.json");
        assert_eq!(create(&file, MANUAL, 1).unwrap(), None, "no data file, no backup");

        fs::write(&file, b"{}").unwrap();
        create(&file, MANUAL, 100).unwrap().unwrap();
        for i in 0..35 {
            create(&file, DAILY, 1_000 + i * 86_400).unwrap();
        }
        let all = list(&file).unwrap();
        assert_eq!(all.iter().filter(|b| b.reason == DAILY).count(), KEEP_AUTOMATIC);
        assert_eq!(all.iter().filter(|b| b.reason == MANUAL).count(), 1, "manual ones are kept");
        assert!(all.windows(2).all(|w| w[0].created >= w[1].created), "newest first");
        assert!(has_daily_for(&file, 1_000 + 34 * 86_400));
        assert!(!has_daily_for(&file, 1_000 + 40 * 86_400));
        assert_eq!(read(&file, &all[0].name).unwrap(), b"{}");
        assert!(read(&file, "../fin.json").is_err());
    }
}
