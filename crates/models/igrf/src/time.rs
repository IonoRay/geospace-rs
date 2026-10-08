use ionoray_core::Epoch;

pub(crate) fn decimal_year(epoch: Epoch) -> f64 {
    let (year, ..) = epoch.to_gregorian_utc();
    let start = Epoch::maybe_from_gregorian_utc(year, 1, 1, 0, 0, 0, 0)
        .expect("year returned by hifitime must form a valid epoch");
    let next = Epoch::maybe_from_gregorian_utc(year + 1, 1, 1, 0, 0, 0, 0)
        .expect("following year must form a valid epoch");
    f64::from(year) + (epoch - start).to_seconds() / (next - start).to_seconds()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_year_accounts_for_leap_years() {
        let epoch = Epoch::maybe_from_gregorian_utc(2020, 7, 2, 0, 0, 0, 0).unwrap();
        assert!((decimal_year(epoch) - 2020.5).abs() < 1.0e-12);
    }
}
