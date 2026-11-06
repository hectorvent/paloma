//! Meta replaces the whole `call_hours` object on each write, and calls outside these hours
//! can't reach the business, so a schedule is checked locally and every change confirms.

use clap::{Args, Subcommand};
use paloma_core::{Error, Result, limits, validate};
use paloma_graph::api::calling;
use serde_json::{Value, json};

use crate::app::{App, no_id};
use crate::output::Output;

const DAYS: [&str; 7] = [
    "MONDAY",
    "TUESDAY",
    "WEDNESDAY",
    "THURSDAY",
    "FRIDAY",
    "SATURDAY",
    "SUNDAY",
];

#[derive(Debug, Subcommand)]
pub enum Hours {
    /// Show the call hours.
    Get,
    /// Replace the call hours. A holiday left out is deleted.
    Set(Set),
    /// Accept calls at any time, keeping the saved schedule.
    Disable,
}

#[derive(Debug, Args)]
pub struct Set {
    /// Timezone, such as America/Santo_Domingo.
    #[arg(long)]
    timezone: String,
    /// Weekly hours as DAY=HHMM-HHMM, such as MONDAY=0900-1700; repeatable, 2 per day at most.
    #[arg(long = "open", required = true, value_name = "DAY=HHMM-HHMM")]
    open: Vec<String>,
    /// Holiday override as YYYY-MM-DD=HHMM-HHMM; repeatable, 20 at most.
    #[arg(long = "holiday", value_name = "DATE=HHMM-HHMM")]
    holidays: Vec<String>,
}

fn weekly(specs: &[String]) -> Result<Vec<calling::OpeningHours>> {
    let mut entries: Vec<(usize, String, String)> = Vec::new();
    for spec in specs {
        let (day, times) = spec
            .split_once('=')
            .ok_or_else(|| Error::validation("--open must use DAY=HHMM-HHMM"))?;
        let day = day.to_ascii_uppercase();
        let index = DAYS
            .iter()
            .position(|d| *d == day)
            .ok_or_else(|| Error::validation(format!("--open has an unknown day: {day}")))?;
        let (open, close) = validate::opening_period(times, "--open")?;
        entries.push((index, open, close));
    }
    entries.sort();
    for (index, day) in DAYS.iter().enumerate() {
        let hours: Vec<_> = entries.iter().filter(|e| e.0 == index).collect();
        if hours.len() > limits::MAX_CALL_HOURS_PER_DAY {
            return Err(Error::validation(format!(
                "{day} has more than {} opening periods",
                limits::MAX_CALL_HOURS_PER_DAY
            )));
        }
        if hours.windows(2).any(|pair| pair[1].1 < pair[0].2) {
            return Err(Error::validation(format!("{day} has overlapping hours")));
        }
    }
    Ok(entries
        .into_iter()
        .map(|(index, open_time, close_time)| calling::OpeningHours {
            day_of_week: DAYS[index],
            open_time,
            close_time,
        })
        .collect())
}

fn holiday_schedule(specs: &[String]) -> Result<Vec<calling::Holiday>> {
    if specs.len() > limits::MAX_CALL_HOLIDAYS {
        return Err(Error::validation(format!(
            "at most {} holiday overrides",
            limits::MAX_CALL_HOLIDAYS
        )));
    }
    specs
        .iter()
        .map(|spec| {
            let invalid = || Error::validation("--holiday must use YYYY-MM-DD=HHMM-HHMM");
            let (date, times) = spec.split_once('=').ok_or_else(invalid)?;
            if date.len() != 10 || !date.contains('-') {
                return Err(invalid());
            }
            super::super::util::timestamp(date, "--holiday")?;
            let (start_time, end_time) = validate::opening_period(times, "--holiday")?;
            Ok(calling::Holiday {
                date: date.to_owned(),
                start_time,
                end_time,
            })
        })
        .collect()
}

impl Hours {
    pub async fn run(self, app: &App) -> Result<()> {
        match self {
            Self::Get => {
                let phone = app.ctx()?.require_phone_number_id()?.to_owned();
                let Some(response) = app.send(&calling::get(&phone, false)).await? else {
                    return Ok(());
                };
                let body = response.json()?;
                let hours = calling::call_hours(&body)
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let status = hours
                    .get("status")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                app.out.emit(&Output::new(hours).id(status));
                Ok(())
            }
            Self::Set(a) => {
                validate::not_blank(&a.timezone, "--timezone")?;
                let weekly_hours = weekly(&a.open)?;
                let holidays = holiday_schedule(&a.holidays)?;
                let phone = app.ctx()?.require_phone_number_id()?.to_owned();
                if !app.confirm_or_abort(&format!(
                    "replace the call hours of number {phone}; calls outside them can't reach you, and holidays left out are deleted"
                ))? {
                    return Ok(());
                }
                let req = calling::set_hours(&phone, &a.timezone, &weekly_hours, &holidays);
                app.send_and_emit(&req, no_id).await
            }
            Self::Disable => {
                let phone = app.ctx()?.require_phone_number_id()?.to_owned();
                // Meta requires the schedule even when disabling, and deletes holidays left out.
                let current = app.fetch(&calling::get(&phone, false)).await?.json()?;
                let Some(hours) = calling::call_hours(&current).cloned() else {
                    app.out.info("call hours aren't set on this number");
                    return Ok(());
                };
                if hours.get("status").and_then(Value::as_str)
                    == Some(calling::Toggle::Disabled.as_str())
                {
                    app.out.info("call hours are already disabled");
                    return Ok(());
                }
                if !app.confirm_or_abort(&format!(
                    "disable call hours on number {phone}, so calls are accepted at any time"
                ))? {
                    return Ok(());
                }
                app.send_and_emit(&calling::disable_hours(&phone, hours), no_id)
                    .await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn specs(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_owned()).collect()
    }

    #[test]
    fn weekly_hours_are_ordered_and_accept_any_case() {
        let hours = weekly(&specs(&[
            "tuesday=0900-1200",
            "MONDAY=1300-1700",
            "MONDAY=0800-1200",
        ]))
        .unwrap();
        let days: Vec<_> = hours.iter().map(|h| h.open_time.as_str()).collect();
        assert_eq!(days, ["0800", "1300", "0900"]);
        assert_eq!(hours[2].day_of_week, "TUESDAY");
    }

    #[test]
    fn bad_schedules_are_refused() {
        for bad in [
            vec!["MONDAY"],
            vec!["FUNDAY=0900-1700"],
            vec!["MONDAY=9-17"],
            vec!["MONDAY=2400-2410"],
            vec!["MONDAY=0960-1000"],
            vec!["MONDAY=1700-0900"],
            vec!["MONDAY=0900-0900"],
            vec!["MONDAY=0900-1200", "MONDAY=1100-1300"],
            vec!["MONDAY=0800-0900", "MONDAY=1000-1100", "MONDAY=1200-1300"],
        ] {
            assert!(weekly(&specs(&bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn holidays_need_a_real_date_and_a_range() {
        assert_eq!(
            holiday_schedule(&specs(&["2026-01-01=0000-2359"])).unwrap()[0],
            calling::Holiday {
                date: "2026-01-01".into(),
                start_time: "0000".into(),
                end_time: "2359".into(),
            }
        );
        for bad in ["2026-13-01=0000-2359", "1767225600=0000-2359", "2026-01-01"] {
            assert!(holiday_schedule(&specs(&[bad])).is_err(), "{bad}");
        }
        let too_many = vec!["2026-01-01=0000-2359".to_owned(); limits::MAX_CALL_HOLIDAYS + 1];
        assert!(holiday_schedule(&too_many).is_err());
    }
}
