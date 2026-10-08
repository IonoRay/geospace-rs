#[cfg(any(feature = "iri", feature = "msis"))]
use super::model::direct_only;
use super::model::{Failure, Session};
#[cfg(any(feature = "igrf", feature = "iri", feature = "hwm", feature = "msis"))]
use super::model::{failure, json};
#[cfg(feature = "hwm")]
use super::model_types::Activity;
#[cfg(any(feature = "iri", feature = "hwm", feature = "msis"))]
use super::model_types::Mode;
use super::model_types::{Drivers, Policy, Request};
use ionoray_core::QueryPoint;

#[cfg(feature = "igrf")]
pub(super) fn evaluate_igrf(query: QueryPoint) -> Result<serde_json::Value, Failure> {
    use crate::igrf::{Igrf, IgrfInput, IgrfVersion};
    let input = IgrfInput { query };
    let result = Igrf::new(IgrfVersion::Igrf14)
        .and_then(|model| model.evaluate(&input))
        .map_err(|e| failure(&e.into()))?;
    json(&serde_json::json!({"input": input, "result": result})).map_err(|e| ("internal_error", e))
}
#[cfg(not(feature = "igrf"))]
pub(super) fn evaluate_igrf(_: QueryPoint) -> Result<serde_json::Value, Failure> {
    Err((
        "model_unavailable",
        "IGRF-14 was not compiled into this binary".into(),
    ))
}

#[cfg(feature = "iri")]
#[cfg_attr(
    not(feature = "indices"),
    expect(
        clippy::unused_async,
        reason = "the same dispatch awaits index preparation when indices is enabled"
    )
)]
pub(super) async fn evaluate_iri(
    session: &mut Session<'_>,
    request: &Request,
    query: QueryPoint,
    policy: Policy,
    d: &Drivers,
) -> Result<serde_json::Value, (&'static str, String)> {
    if request.mode == Mode::Direct {
        direct_only(Some(d), &[d.rz12, d.ig12, d.f107_daily, d.f107_81_day])
            .map_err(|e| ("invalid_request", e))?;
    }
    if let (Some(rz12), Some(ig12), Some(f107_daily), Some(f107_81_day)) =
        (d.rz12, d.ig12, d.f107_daily, d.f107_81_day)
    {
        let input = crate::iri::IriInput {
            query,
            drivers: crate::iri::IriDrivers {
                sunspot_number_12_month: rz12,
                ionospheric_index_12_month: ig12,
                f107_daily,
                f107_81_day,
            },
        };
        let result = crate::iri::Iri::new(crate::iri::IriVersion::Iri2020)
            .evaluate(&input)
            .map_err(|e| failure(&e.into()))?;
        return json(&serde_json::json!({"input":input,"indices":{"rz12":null,"ig12":null,"f107_daily":null,"f107_81_day":null},"result":result}))
            .map_err(|e| ("internal_error", e));
    }
    #[cfg(feature = "indices")]
    {
        let request = crate::IriRequest {
            query,
            overrides: crate::IriDriverOverrides {
                rz12: d.rz12,
                ig12: d.ig12,
                f107_daily: d.f107_daily,
                f107_81_day: d.f107_81_day,
            },
        };
        let result = session
            .get()
            .await?
            .evaluate_iri(request, policy.into())
            .await
            .map_err(|e| failure(&e))?;
        json(&result).map_err(|message| ("internal_error", message))
    }
    #[cfg(not(feature = "indices"))]
    {
        let _ = (session, policy);
        Err((
            "data_unavailable",
            "automatic drivers require the indices feature".into(),
        ))
    }
}
#[cfg(not(feature = "iri"))]
#[expect(
    clippy::unused_async,
    reason = "feature-disabled branch keeps the asynchronous dispatch signature"
)]
pub(super) async fn evaluate_iri(
    _: &mut Session<'_>,
    _: &Request,
    _: QueryPoint,
    _: Policy,
    _: &Drivers,
) -> Result<serde_json::Value, (&'static str, String)> {
    Err((
        "model_unavailable",
        "IRI-2020 support was not compiled into this binary".into(),
    ))
}

#[cfg(feature = "hwm")]
#[cfg_attr(
    not(feature = "indices"),
    expect(
        clippy::unused_async,
        reason = "the same dispatch awaits index preparation when indices is enabled"
    )
)]
pub(super) async fn evaluate_hwm(
    session: &mut Session<'_>,
    request: &Request,
    query: QueryPoint,
    policy: Policy,
    d: &Drivers,
) -> Result<serde_json::Value, (&'static str, String)> {
    use crate::hwm::HwmGeomagneticActivity;
    let activity = match d.activity {
        Some(Activity::Quiet) if d.current_ap.is_none() => Some(HwmGeomagneticActivity::Quiet),
        Some(Activity::Disturbed) if d.current_ap.is_some() => {
            Some(HwmGeomagneticActivity::Disturbed {
                current_ap: d.current_ap.unwrap(),
            })
        }
        Some(_) => {
            return Err((
                "invalid_request",
                "quiet forbids current_ap and disturbed requires it".into(),
            ));
        }
        None if d.current_ap.is_none() => None,
        None => return Err(("invalid_request", "current_ap requires activity".into())),
    };
    if request.mode == Mode::Direct && activity.is_none() {
        return Err(("invalid_request", "direct mode requires activity".into()));
    }
    if let Some(activity) = activity {
        let input = crate::hwm::HwmInput {
            query,
            geomagnetic_activity: activity,
        };
        let result = crate::hwm::Hwm::new(crate::hwm::HwmVersion::Hwm14)
            .evaluate(&input)
            .map_err(|e| failure(&e.into()))?;
        return json(&serde_json::json!({"input":input,"ap_index":null,"result":result}))
            .map_err(|e| ("internal_error", e));
    }
    #[cfg(feature = "indices")]
    {
        let request = crate::HwmRequest::new(query);
        let result = session
            .get()
            .await?
            .evaluate_hwm(request, policy.into())
            .await
            .map_err(|e| failure(&e))?;
        json(&result).map_err(|message| ("internal_error", message))
    }
    #[cfg(not(feature = "indices"))]
    {
        let _ = (session, policy);
        Err((
            "data_unavailable",
            "automatic drivers require the indices feature".into(),
        ))
    }
}
#[cfg(not(feature = "hwm"))]
#[expect(
    clippy::unused_async,
    reason = "feature-disabled branch keeps the asynchronous dispatch signature"
)]
pub(super) async fn evaluate_hwm(
    _: &mut Session<'_>,
    _: &Request,
    _: QueryPoint,
    _: Policy,
    _: &Drivers,
) -> Result<serde_json::Value, (&'static str, String)> {
    Err((
        "model_unavailable",
        "HWM14 support was not compiled into this binary".into(),
    ))
}

#[cfg(feature = "msis")]
#[cfg_attr(
    not(feature = "indices"),
    expect(
        clippy::unused_async,
        reason = "the same dispatch awaits index preparation when indices is enabled"
    )
)]
pub(super) async fn evaluate_msis(
    session: &mut Session<'_>,
    request: &Request,
    query: QueryPoint,
    policy: Policy,
    d: &Drivers,
) -> Result<serde_json::Value, (&'static str, String)> {
    use crate::msis::{
        Msis, MsisApHistory, MsisDrivers, MsisGeomagneticActivity, MsisInput, MsisVersion,
    };
    let activity = match (d.ap_daily, d.ap_history) {
        (Some(value), None) => Some(MsisGeomagneticActivity::Daily(value)),
        (None, Some(h)) => Some(MsisGeomagneticActivity::StormTime(MsisApHistory {
            daily: h.daily,
            current: h.current,
            three_hours_ago: h.three_hours_ago,
            six_hours_ago: h.six_hours_ago,
            nine_hours_ago: h.nine_hours_ago,
            average_12_to_33_hours: h.average_12_to_33_hours,
            average_36_to_57_hours: h.average_36_to_57_hours,
        })),
        (None, None) => None,
        _ => {
            return Err((
                "invalid_request",
                "provide exactly one of ap_daily or ap_history".into(),
            ));
        }
    };
    if request.mode == Mode::Direct {
        direct_only(Some(d), &[d.f107a, d.f107_previous_day])
            .map_err(|e| ("invalid_request", e))?;
        if activity.is_none() {
            return Err(("invalid_request", "direct mode requires Ap activity".into()));
        }
    }
    if let (Some(activity), Some(f107a), Some(f107_previous_day)) =
        (activity, d.f107a, d.f107_previous_day)
    {
        let input = MsisInput {
            query,
            drivers: MsisDrivers {
                f107a,
                f107_previous_day,
                geomagnetic_activity: activity,
            },
        };
        let result = Msis::new(MsisVersion::Nrlmsis21)
            .evaluate(&input)
            .map_err(|e| failure(&e.into()))?;
        return json(&serde_json::json!({"input":input,"indices":{"f107a":null,"f107_previous_day":null,"ap_daily":null,"ap_three_hourly":[]},"result":result}))
            .map_err(|e| ("internal_error", e));
    }
    #[cfg(feature = "indices")]
    {
        let request = crate::MsisRequest {
            query,
            overrides: crate::MsisDriverOverrides {
                f107a: d.f107a,
                f107_previous_day: d.f107_previous_day,
                geomagnetic_activity: activity,
            },
        };
        let result = session
            .get()
            .await?
            .evaluate_msis(request, policy.into())
            .await
            .map_err(|e| failure(&e))?;
        json(&result).map_err(|message| ("internal_error", message))
    }
    #[cfg(not(feature = "indices"))]
    {
        let _ = (session, policy);
        Err((
            "data_unavailable",
            "automatic drivers require the indices feature".into(),
        ))
    }
}
#[cfg(not(feature = "msis"))]
#[expect(
    clippy::unused_async,
    reason = "feature-disabled branch keeps the asynchronous dispatch signature"
)]
pub(super) async fn evaluate_msis(
    _: &mut Session<'_>,
    _: &Request,
    _: QueryPoint,
    _: Policy,
    _: &Drivers,
) -> Result<serde_json::Value, (&'static str, String)> {
    Err((
        "model_unavailable",
        "NRLMSIS 2.1 support was not compiled into this binary".into(),
    ))
}
