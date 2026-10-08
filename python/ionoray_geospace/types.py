"""Public serialized Rust shapes. Scalars use SI; solar flux uses sfu."""
from typing import Literal, TypedDict

DataPolicy = Literal["ensure", "offline", "refresh"]
Activity = Literal["quiet", "disturbed"]

class Position(TypedDict):
    latitude: float  # radians
    longitude: float  # radians
    altitude: float  # metres

class QueryPoint(TypedDict):
    epoch: str
    position: Position

class IgrfInput(TypedDict):
    query: QueryPoint

class MagneticField(TypedDict):
    east: float  # tesla
    north: float
    up: float
    magnitude: float
    declination: float  # radians
    inclination: float

class IgrfProvenance(TypedDict):
    version: Literal["Igrf14"]
    coefficient_sha256: str
    implementation_version: str
    max_degree: int

class IgrfResult(TypedDict):
    field: MagneticField
    provenance: IgrfProvenance

class IriDrivers(TypedDict):
    sunspot_number_12_month: float
    ionospheric_index_12_month: float
    f107_daily: float
    f107_81_day: float

class IriInput(TypedDict):
    query: QueryPoint
    drivers: IriDrivers

class IonComposition(TypedDict):
    oxygen_m3: float | None
    hydrogen_m3: float | None
    helium_m3: float | None
    molecular_oxygen_m3: float | None
    nitric_oxide_m3: float | None
    cluster_m3: float | None
    nitrogen_m3: float | None

class IonospherePoint(TypedDict):
    electron_density_m3: float | None
    neutral_temperature: float | None  # kelvin
    ion_temperature: float | None
    electron_temperature: float | None
    ions: IonComposition
    solar_zenith_angle: float
    magnetic_dip_angle: float
    modified_dip_latitude: float

class IriLayerPeak(TypedDict):
    electron_density_m3: float
    height: float  # metres

class IriLayerPeaks(TypedDict):
    f2: IriLayerPeak | None
    f1: IriLayerPeak | None
    e: IriLayerPeak | None

class AssetProvenance(TypedDict):
    release_sha256: str
    build_source: str
    implementation_version: str

class IriProvenance(AssetProvenance):
    version: Literal["Iri2020"]
    asset_set_sha256: str

class IriResult(TypedDict):
    point: IonospherePoint
    peaks: IriLayerPeaks
    provenance: IriProvenance

class CurrentAp(TypedDict):
    current_ap: float

class Disturbed(TypedDict):
    Disturbed: CurrentAp

class HwmInput(TypedDict):
    query: QueryPoint
    geomagnetic_activity: Literal["Quiet"] | Disturbed

class HorizontalWind(TypedDict):
    northward: float  # metres per second
    eastward: float

class HwmProvenance(AssetProvenance):
    version: Literal["Hwm14"]
    asset_set_sha256: str

class HwmResult(TypedDict):
    wind: HorizontalWind
    provenance: HwmProvenance

class ApHistory(TypedDict):
    daily: float
    current: float
    three_hours_ago: float
    six_hours_ago: float
    nine_hours_ago: float
    average_12_to_33_hours: float
    average_36_to_57_hours: float

class DailyActivity(TypedDict):
    Daily: float

class StormActivity(TypedDict):
    StormTime: ApHistory

class MsisDrivers(TypedDict):
    f107a: float
    f107_previous_day: float
    geomagnetic_activity: DailyActivity | StormActivity

class MsisInput(TypedDict):
    query: QueryPoint
    drivers: MsisDrivers

class NeutralAtmosphere(TypedDict):
    temperature: float
    exospheric_temperature: float
    mass_density_kg_m3: float | None
    n2_number_density_m3: float | None
    o2_number_density_m3: float | None
    o_number_density_m3: float | None
    he_number_density_m3: float | None
    h_number_density_m3: float | None
    ar_number_density_m3: float | None
    n_number_density_m3: float | None
    anomalous_o_number_density_m3: float | None
    no_number_density_m3: float | None

class MsisProvenance(AssetProvenance):
    version: Literal["Nrlmsis21"]
    parameter_sha256: str

class MsisResult(TypedDict):
    atmosphere: NeutralAtmosphere
    provenance: MsisProvenance

class TimeInterval(TypedDict):
    start: str
    end: str

class SourceDerivation(TypedDict):
    method: Literal["source"]

class LinearInterpolation(TypedDict):
    method: Literal["linear_interpolation"]
    gap_days: int

class CenteredMean(TypedDict):
    method: Literal["centered_mean"]
    window_days: int
    interpolated_input_count: int

class IndexSample(TypedDict):
    value: float
    interval: TimeInterval
    quality: Literal["Final", "Provisional", "Predicted", "Unknown"]
    derivation: SourceDerivation | LinearInterpolation | CenteredMean
    release_id: str
    artifact: str
    snapshot: str

class IriIndexEvidence(TypedDict):
    rz12: IndexSample | None
    ig12: IndexSample | None
    f107_daily: IndexSample | None
    f107_81_day: IndexSample | None

class MsisIndexEvidence(TypedDict):
    f107a: IndexSample | None
    f107_previous_day: IndexSample | None
    ap_daily: IndexSample | None
    ap_three_hourly: list[IndexSample]

class IriEvaluation(TypedDict):
    input: IriInput
    indices: IriIndexEvidence
    result: IriResult

class HwmEvaluation(TypedDict):
    input: HwmInput
    ap_index: IndexSample | None
    result: HwmResult

class MsisEvaluation(TypedDict):
    input: MsisInput
    indices: MsisIndexEvidence
    result: MsisResult
