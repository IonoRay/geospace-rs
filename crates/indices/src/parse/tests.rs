use super::*;

#[test]
fn packaged_2021_gaps_produce_traceable_msis_drivers() {
    let source = crate::download::cache::for_dataset(IndexDataset::KpApF107, 2021).unwrap();
    let content = std::str::from_utf8(source.bytes).unwrap();
    let ParsedFile::Gfz { daily, .. } = parse_gfz(content, 2021).unwrap() else {
        panic!("expected GFZ records")
    };
    let record = |month, day| {
        daily
            .iter()
            .find(|record| record.epoch_ms == epoch_ms(2021, month, day, 0).unwrap())
            .unwrap()
    };

    let june_16 = record(6, 16);
    assert_close(june_16.f107_observed.unwrap(), 80.25);
    assert_eq!(june_16.f107_observed_interpolation_gap_days, 1);

    let june_17 = record(6, 17);
    assert_close(june_17.f107a.unwrap(), 79.374_691_358_024_69);
    assert_eq!(june_17.f107a_interpolated_input_count, 2);

    let july_3 = record(7, 3);
    assert_close(july_3.f107a.unwrap(), 79.283_333_333_333_33);
    assert_eq!(july_3.f107a_interpolated_input_count, 1);
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1.0e-10,
        "{actual} != {expected}"
    );
}

#[test]
fn parses_dst_base_and_24_hourly_residuals() {
    let mut line = vec![b' '; 120];
    line[0..3].copy_from_slice(b"DST");
    line[3..5].copy_from_slice(b"20");
    line[5..7].copy_from_slice(b"01");
    line[8..10].copy_from_slice(b"01");
    line[14..16].copy_from_slice(b"20");
    line[16..20].copy_from_slice(b"   1");
    for hour in 0..24 {
        let field = format!("{hour:>4}");
        let start = 20 + hour * 4;
        line[start..start + 4].copy_from_slice(field.as_bytes());
    }
    let parsed = parse_dst(std::str::from_utf8(&line).unwrap(), 2020, 1).unwrap();
    let ParsedFile::Dst(values) = parsed else {
        panic!("expected Dst")
    };
    assert_eq!(values.len(), 24);
    assert!((values[0].value.unwrap() - 100.0).abs() < f64::EPSILON);
    assert!((values[23].value.unwrap() - 123.0).abs() < f64::EPSILON);
}

#[test]
fn parses_ae_hourly_mean() {
    let mut line = vec![b' '; 400];
    line[0..8].copy_from_slice(b"AEALAOAU");
    line[12..14].copy_from_slice(b"20");
    line[14..16].copy_from_slice(b"01");
    line[16..18].copy_from_slice(b"01");
    line[18] = b'E';
    line[19..21].copy_from_slice(b"00");
    for offset in (34..400).step_by(6) {
        line[offset..offset + 6].copy_from_slice(b"     0");
    }
    line[394..400].copy_from_slice(b"   123");
    let parsed = parse_ae(std::str::from_utf8(&line).unwrap(), 2020, 1).unwrap();
    let ParsedFile::Ae(values) = parsed else {
        panic!("expected AE")
    };
    assert_eq!(values.len(), 1);
    assert!((values[0].value.unwrap() - 123.0).abs() < f64::EPSILON);
}

#[test]
fn retains_declared_dst_missing_hour() {
    let mut line = vec![b' '; 120];
    line[0..3].copy_from_slice(b"DST");
    line[3..5].copy_from_slice(b"20");
    line[5..7].copy_from_slice(b"01");
    line[8..10].copy_from_slice(b"01");
    line[14..16].copy_from_slice(b"20");
    line[16..20].copy_from_slice(b"   0");
    for hour in 0..24 {
        let value = if hour == 7 { 9_999 } else { hour };
        line[20 + hour * 4..24 + hour * 4].copy_from_slice(format!("{value:>4}").as_bytes());
    }
    let ParsedFile::Dst(values) = parse_dst(std::str::from_utf8(&line).unwrap(), 2020, 1).unwrap()
    else {
        panic!("expected Dst")
    };
    assert_eq!(values.len(), 24);
    assert_eq!(values[7].value, None);
}

#[test]
fn accepts_partial_month_but_rejects_duplicate_and_wrong_period_rows() {
    let mut line = vec![b' '; 120];
    line[0..3].copy_from_slice(b"DST");
    line[3..5].copy_from_slice(b"20");
    line[5..7].copy_from_slice(b"01");
    line[8..10].copy_from_slice(b"01");
    line[14..16].copy_from_slice(b"20");
    line[16..20].copy_from_slice(b"   0");
    for hour in 0..24 {
        line[20 + hour * 4..24 + hour * 4].copy_from_slice(b"   1");
    }
    let row = std::str::from_utf8(&line).unwrap();
    let ParsedFile::Dst(values) = parse_dst(row, 2020, 1).unwrap() else {
        panic!("expected Dst")
    };
    assert_eq!(values.len(), 24);
    assert!(parse_dst(&format!("{row}\n{row}"), 2020, 1).is_err());
    assert!(parse_dst(row, 2020, 2).is_err());
}

#[test]
fn rejects_non_finite_gfz_values() {
    let row = "2020 01 01 0 0 0 0 NaN 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 70 70 final";
    assert!(parse_gfz(row, 2020).is_err());
}

#[test]
fn retains_gfz_declared_missing_fields_as_null_samples() {
    let row = "2020 01 01 0 0 0 0 -1 0 0 0 0 0 0 0 -1 0 0 0 0 0 0 0 -1 0 70 70 final";
    let ParsedFile::Gfz { geomagnetic, daily } = parse_gfz(row, 2020).unwrap() else {
        panic!("expected GFZ")
    };
    assert_eq!(geomagnetic.len(), 8);
    assert_eq!(geomagnetic[0].kp_thirds, None);
    assert_eq!(geomagnetic[0].ap, None);
    assert_eq!(daily.len(), 1);
    assert_eq!(daily[0].ap_daily, None);
}
