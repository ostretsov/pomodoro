//! `~/.config/pomodoro/settings`: a few `key = value` lines.

use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Length of a work period.
    pub work_minutes: u32,
    /// How long the "time's up" notification stays on screen.
    pub notice_seconds: u32,
    /// Look for a newer release on GitHub (installed package only).
    pub check_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            work_minutes: 20,
            notice_seconds: 15,
            check_updates: true,
        }
    }
}

impl Settings {
    pub fn work(&self) -> Duration {
        Duration::from_secs(u64::from(self.work_minutes) * 60)
    }

    pub fn notice(&self) -> Duration {
        Duration::from_secs(u64::from(self.notice_seconds))
    }

    /// Unknown keys are ignored (with a warning); a bad value is an error.
    pub fn parse(text: &str) -> Result<(Self, Vec<String>), String> {
        let mut s = Self::default();
        let mut warnings = Vec::new();
        for (i, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(format!("line {}: expected key = value: {raw}", i + 1));
            };
            let (key, value) = (key.trim(), value.trim());
            let number = |min: u32, max: u32| -> Result<u32, String> {
                match value.parse::<u32>() {
                    Ok(n) if (min..=max).contains(&n) => Ok(n),
                    _ => Err(format!(
                        "line {}: {key} must be a whole number from {min} to {max}, not {value:?}",
                        i + 1
                    )),
                }
            };
            match key {
                "work_minutes" => s.work_minutes = number(1, 600)?,
                "notice_seconds" => s.notice_seconds = number(1, 3600)?,
                "check_updates" => {
                    s.check_updates = match value {
                        "true" => true,
                        "false" => false,
                        _ => {
                            return Err(format!(
                                "line {}: check_updates must be true or false, not {value:?}",
                                i + 1
                            ));
                        }
                    }
                }
                _ => warnings.push(format!("line {}: unknown setting {key:?}", i + 1)),
            }
        }
        Ok((s, warnings))
    }

    pub fn to_file(&self) -> String {
        format!(
            "# Pomodoro settings. Restart the app (Quit, then start it again) after changes.\n\
             # Length of a work period, minutes.\n\
             work_minutes = {}\n\
             # How long the \"time's up\" notification stays on screen, seconds.\n\
             notice_seconds = {}\n\
             # Look for a newer version on GitHub twice a day and offer it in the menu.\n\
             check_updates = {}\n",
            self.work_minutes, self.notice_seconds, self.check_updates
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_20_minutes_and_15_seconds() {
        let s = Settings::default();
        assert_eq!(s.work(), Duration::from_secs(1200));
        assert_eq!(s.notice(), Duration::from_secs(15));
    }

    #[test]
    fn round_trips() {
        let s = Settings {
            work_minutes: 25,
            notice_seconds: 30,
            check_updates: false,
        };
        assert_eq!(Settings::parse(&s.to_file()).unwrap(), (s, vec![]));
    }

    #[test]
    fn parses_partial_files_with_comments() {
        let (s, w) = Settings::parse("  # hi\n\nwork_minutes=45  \n").unwrap();
        assert_eq!(s.work_minutes, 45);
        assert_eq!(s.notice_seconds, 15);
        assert!(w.is_empty());
    }

    #[test]
    fn warns_about_unknown_keys() {
        let (_, w) = Settings::parse("color = red\n").unwrap();
        assert_eq!(w, vec!["line 1: unknown setting \"color\"".to_string()]);
    }

    #[test]
    fn rejects_bad_values() {
        for bad in [
            "work_minutes = 0",
            "work_minutes = 601",
            "work_minutes = twenty",
            "notice_seconds = -1",
            "work_minutes",
            "check_updates = yes",
        ] {
            assert!(Settings::parse(bad).is_err(), "{bad}");
        }
    }
}
