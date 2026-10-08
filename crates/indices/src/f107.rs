const CENTERED_WINDOW_RADIUS: usize = 40;
const CENTERED_WINDOW_DAYS: usize = CENTERED_WINDOW_RADIUS * 2 + 1;
const MAX_INTERPOLATION_GAP_DAYS: usize = 2;

#[derive(Clone, Copy, Debug)]
pub(crate) struct DailyF107Input {
    pub(crate) year: u16,
    pub(crate) month: u8,
    pub(crate) day: u8,
    pub(crate) observed: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NormalizedF107 {
    pub(crate) value: f64,
    pub(crate) interpolation_gap_days: u16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CenteredF107 {
    pub(crate) value: f64,
    pub(crate) interpolated_input_count: u16,
}

pub(crate) fn normalize(
    inputs: &[DailyF107Input],
    target_year: u16,
) -> (Vec<Option<NormalizedF107>>, Vec<Option<CenteredF107>>) {
    let mut normalized = inputs
        .iter()
        .map(|input| {
            input.observed.map(|value| NormalizedF107 {
                value,
                interpolation_gap_days: 0,
            })
        })
        .collect::<Vec<_>>();
    let target_range = target_range(inputs, target_year);
    let mut cursor = 0;
    while cursor < normalized.len() {
        if normalized[cursor].is_some() {
            cursor += 1;
            continue;
        }
        let gap_start = cursor;
        while cursor < normalized.len() && normalized[cursor].is_none() {
            cursor += 1;
        }
        let gap_end = cursor;
        normalize_gap(
            inputs,
            &mut normalized,
            target_year,
            target_range,
            gap_start,
            gap_end,
        );
    }
    let averages = centered_averages(&normalized);
    (normalized, averages)
}

fn normalize_gap(
    inputs: &[DailyF107Input],
    normalized: &mut [Option<NormalizedF107>],
    target_year: u16,
    target_range: Option<(usize, usize)>,
    gap_start: usize,
    gap_end: usize,
) {
    let gap_days = gap_end - gap_start;
    let relevant = target_range.is_some_and(|range| gap_relevant(gap_start, gap_end, range));
    let bounded = gap_start > 0 && gap_end < normalized.len();
    if !bounded || gap_days > MAX_INTERPOLATION_GAP_DAYS {
        if relevant {
            tracing::warn!(
                event = "indices.f107.gap.unfilled",
                dataset = "kp-ap-f107",
                target_year,
                gap_start = %date(inputs[gap_start]),
                gap_end = %date(inputs[gap_end - 1]),
                gap_days,
                max_interpolation_gap_days = MAX_INTERPOLATION_GAP_DAYS,
                bounded,
                "observed F10.7 gap exceeds the supported interpolation contract"
            );
        }
        return;
    }
    let left = normalized[gap_start - 1].expect("bounded gap has a left observation");
    let right = normalized[gap_end].expect("bounded gap has a right observation");
    let divisor = f64::from(u32::try_from(gap_days + 1).expect("F10.7 gap fits in u32"));
    let step = (right.value - left.value) / divisor;
    let gap_days_u16 = u16::try_from(gap_days).expect("bounded F10.7 gap fits in u16");
    for (offset, slot) in normalized[gap_start..gap_end].iter_mut().enumerate() {
        let distance = f64::from(u32::try_from(offset + 1).expect("F10.7 gap fits in u32"));
        *slot = Some(NormalizedF107 {
            value: left.value + step * distance,
            interpolation_gap_days: gap_days_u16,
        });
    }
    if relevant {
        tracing::warn!(
            event = "indices.f107.gap.interpolated",
            dataset = "kp-ap-f107",
            target_year,
            gap_start = %date(inputs[gap_start]),
            gap_end = %date(inputs[gap_end - 1]),
            gap_days,
            left_date = %date(inputs[gap_start - 1]),
            left_value_sfu = left.value,
            right_date = %date(inputs[gap_end]),
            right_value_sfu = right.value,
            method = "bounded_linear_interpolation",
            "linearly interpolated missing observed F10.7 values"
        );
    }
}

fn centered_averages(values: &[Option<NormalizedF107>]) -> Vec<Option<CenteredF107>> {
    (0..values.len())
        .map(|center| {
            let start = center.saturating_sub(CENTERED_WINDOW_RADIUS);
            let end = (center + CENTERED_WINDOW_RADIUS).min(values.len().saturating_sub(1));
            let window = &values[start..=end];
            if window.len() != CENTERED_WINDOW_DAYS || window.iter().any(Option::is_none) {
                return None;
            }
            let values = window.iter().flatten().collect::<Vec<_>>();
            let interpolated_input_count = u16::try_from(
                values
                    .iter()
                    .filter(|value| value.interpolation_gap_days > 0)
                    .count(),
            )
            .expect("centered F10.7 window fits in u16");
            let window_days =
                u16::try_from(window.len()).expect("centered F10.7 window fits in u16");
            Some(CenteredF107 {
                value: values.iter().map(|value| value.value).sum::<f64>() / f64::from(window_days),
                interpolated_input_count,
            })
        })
        .collect()
}

fn target_range(inputs: &[DailyF107Input], target_year: u16) -> Option<(usize, usize)> {
    let start = inputs.iter().position(|input| input.year == target_year)?;
    let end = inputs.iter().rposition(|input| input.year == target_year)?;
    Some((start, end))
}

fn gap_relevant(gap_start: usize, gap_end: usize, target_range: (usize, usize)) -> bool {
    let relevant_start = target_range.0.saturating_sub(CENTERED_WINDOW_RADIUS);
    let relevant_end = target_range
        .1
        .saturating_add(CENTERED_WINDOW_RADIUS)
        .saturating_add(1);
    gap_end > relevant_start && gap_start < relevant_end
}

fn date(input: DailyF107Input) -> String {
    format!("{:04}-{:02}-{:02}", input.year, input.month, input.day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(values: &[Option<f64>]) -> Vec<DailyF107Input> {
        values
            .iter()
            .enumerate()
            .map(|(index, value)| DailyF107Input {
                year: 2021,
                month: 1,
                day: u8::try_from(index + 1).unwrap_or(1),
                observed: *value,
            })
            .collect()
    }

    #[test]
    fn bounded_two_day_gap_is_interpolated_and_counted_in_average() {
        let mut values = vec![Some(100.0); CENTERED_WINDOW_DAYS];
        values[39] = Some(90.0);
        values[40] = None;
        values[41] = None;
        values[42] = Some(120.0);
        let (normalized, averages) = normalize(&inputs(&values), 2021);
        assert!((normalized[40].unwrap().value - 100.0).abs() < f64::EPSILON);
        assert!((normalized[41].unwrap().value - 110.0).abs() < f64::EPSILON);
        assert_eq!(
            normalized[40].unwrap().interpolation_gap_days,
            u16::try_from(2).unwrap()
        );
        assert_eq!(averages[40].unwrap().interpolated_input_count, 2);
    }

    #[test]
    fn longer_gap_remains_explicitly_unavailable() {
        let mut values = vec![Some(100.0); CENTERED_WINDOW_DAYS];
        values[39..42].fill(None);
        let (normalized, averages) = normalize(&inputs(&values), 2021);
        assert!(normalized[40].is_none());
        assert!(averages[40].is_none());
    }
}
