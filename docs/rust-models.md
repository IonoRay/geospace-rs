# Rust 模型与示例

`ionoray-geospace` 有三条模型使用路径：显式输入、自动补齐驱动、固定基线情景。
所有命令从仓库根目录运行，并使用 `flake.nix` 固定的工具链。

## 按用途选择示例

| 用途 | Cargo example | feature | VS Code launch |
|---|---|---|---|
| 已有完整驱动，直接计算 | [`direct_models`](../crates/geospace/examples/direct_models.rs) | `igrf,iri,hwm,msis` | `Geospace Rust: direct models` |
| 从本地指数准备缺少的驱动 | [`automatic_drivers`](../crates/geospace/examples/automatic_drivers.rs) | `standard` | `Geospace Rust: automatic drivers (offline)` |
| 从同一基线修改驱动并比较 | [`driver_scenarios`](../crates/geospace/examples/driver_scenarios.rs) | `standard` | `Geospace Rust: driver scenarios (offline)` |

三个示例都调用生产 API。后两个使用隔离临时 home 和内置快照，不读取用户数据，
也不联网。注释中的三个 `Breakpoint` 依次对应输入、计算、输出，适合先运行，再从
VS Code 单步，最后修改固定输入或情景列表。

```bash
nix develop .#default --command env IONORAY_OFFLINE=1 \
  cargo run --locked --offline -p ionoray-geospace --no-default-features \
  --features igrf,iri,hwm,msis --example direct_models

nix develop .#default --command env IONORAY_OFFLINE=1 \
  cargo run --locked --offline -p ionoray-geospace --no-default-features \
  --features standard --example automatic_drivers

nix develop .#default --command env IONORAY_OFFLINE=1 \
  cargo run --locked --offline -p ionoray-geospace --no-default-features \
  --features standard --example driver_scenarios
```

`IONORAY_OFFLINE=1` 约束模型构建资产，Cargo `--offline` 禁止下载依赖，
示例中的 `DataPolicy::Offline` 禁止运行时指数下载；三者不是同一个开关。

## 三条调用路径

显式路径构造模型自己的 `Input`，再调用 `Model::evaluate(&input)`。模型 crate
不访问指数库、网络或 `IONORAY_HOME`。IGRF 只提供这条路径，因为它不需要外部驱动。

### IGRF-14 独立计算与调试

在仓库根目录运行：

```bash
nix develop .#default --command env IONORAY_OFFLINE=1 cargo run --locked --offline -p ionoray-igrf --example igrf14_evaluate
nix develop .#default --command env IONORAY_OFFLINE=1 cargo test --locked --offline -p ionoray-igrf --lib
```

入口是 [igrf14_evaluate.rs](../crates/models/igrf/examples/igrf14_evaluate.rs)：
`IgrfInput → Igrf::evaluate → field/provenance`。示例输入为 UTC 2025-01-01、
WGS84 纬度 30°、东经 120°、椭球高 300 km；无需 Session 或指数数据。
stdout JSON 保留输入、nT 分量、角度和系数 hash，日志在 stderr。
输入的序列化单位遵循 core 的 SI 量（角度 rad、高度 m），
构造函数 `from_degrees_kilometers` 则明确接收度与 km。

返回的强类型磁场以 T 为 SI 基础，使用 `.get::<nanotesla>()` 取得 nT（1 nT = 10⁻⁹ T）。
官方 X/Y/Z 是北/东/下，本接口是东/北/上，故 `up = -Z`；
偏角向东为正，倾角向下为正。该点约为 north=29180、east=-2703、up=-29642 nT，
这是整数舍入参考，实际 JSON 保留小数。
十进制年由该 UTC 年内经过秒数/全年秒数转换；支持闭区间 [1900.0, 2030.0]，
2030-01-01 之后不属于支持区间。1995 前使用 10 阶、之后 13 阶，
2025–2030 使用 2025 系数加世俗变化。
参考来源、舍入容差、两极坐标约定和复算脚本见
[参考说明](../crates/models/igrf/data/README.md)。

VS Code 选择 `Geospace Rust: IGRF-14`；它仅构建本模型 example。
断点放在 example 的 `model.evaluate(&input)`（观察 input）、
[model.rs](../crates/models/igrf/src/model.rs) 的 `match resolved.mode`
（观察 year、resolved.mode、resolved.max_degree），以及 example 的 `println!`
（观察 result.field 和 result.provenance）。也可从该文件 `#[test]` 的 CodeLens 进入测试。
终端测试与编辑器断点是两项独立检查；请按上述位置核对 VS Code 断点是否命中。

运行前先预测：将示例经度 120° 改为 -240°，结果是否改变？
运行后对照 `longitude_normalization_is_model_invariant`，并指出输入高度与输出磁场的单位。

### HWM14 显式、自动驱动与情景

在仓库根目录运行：

```bash
nix develop .#default --command env IONORAY_OFFLINE=1 cargo run --locked --offline -p ionoray-hwm --example hwm14_explicit_input
nix develop .#default --command env IONORAY_OFFLINE=1 cargo test --locked --offline -p ionoray-geospace --no-default-features --features indices,hwm --test hwm_workflow
```

[显式 example](../crates/models/hwm/examples/hwm14_explicit_input.rs) 输入为
1995-05-30 12:00 UTC、纬度 -45°/经度 -85°、椭球高 250 km、三小时 ap=80。
`evaluate` 自动初始化，`initialize()` 仅是可选预检。输出北向/东向风，正方向为北/东，
单位 m/s；该例官方总风参考约为 north=40.408、east=-87.560 m/s。
构造输入用度/km，序列化位置用 rad/m；JSON 同时保留 input 与模型 provenance。

自动路径是 `HwmRequest::new(query) → prepare_hwm → PreparedHwm → evaluate`：
缺省活动使用包含查询时刻的 `[start, end)` 三小时 ap。`Quiet` 或显式 `Disturbed`
在 Offline/Ensure/Refresh 下均跳过指数准备与读取；Quiet 不是 ap=0 的同义词。
固定快照 2020-07-01 的 09–12 UTC ap=4、12–15 UTC ap=2，来源见
[GFZ cache](../crates/indices/cache/README.md) 第 32365 行。
`ap_index()` 保留时间区间、quality、release、artifact 与 snapshot；当前 reader 将
rolling edition 映射为 Provisional，这是 reader 对数据版本的映射，不代表独立质量评估。

`with_activity` 复制基线，只改变活动并清除副本的 ap 证据；即使覆盖为相同值也清除。
基线的 input/evidence 不变，模型系数 provenance 保留。Prepared 在 store 关闭后仍可计算，
负数、NaN/无穷 ap 在 evaluate 时显式报错；缺失自动 ap 不会填零。
使用既有 `automatic_drivers` / `driver_scenarios` example 和对应离线 launch 观察完整 JSON。
这些示例也包含其他模型；HWM 的定向测试入口为
[hwm_workflow.rs](../crates/geospace/tests/hwm_workflow.rs)，仅需 `indices,hwm`。

VS Code 显式入口选 `Geospace Rust: HWM14`，断点放在 example 的 `model.evaluate`
和 `println!`，观察 input/result.wind/provenance。自动/情景可从上述 test 的 CodeLens
进入：在 `let sample = prepared.ap_index()` 观察时间/quality，在 `home.close()` 后观察
baseline，在 `scenario.evaluate()` 观察覆盖后 input/evidence。也可在正式
[prepare_hwm.rs](../crates/geospace/src/prepare_hwm.rs) 的 `ap_at` 返回后单步查看 sample。
终端与 VS Code GUI 断点须分别核对，测试通过不证明编辑器调试成功。

36 点官方参考、复算方法及适用范围见 [参考说明](../crates/models/hwm/data/README.md)。
复现建议：固定时空点，将 Disturbed 切为 Quiet，运行前预测 ap 证据是否保留；
运行后指出改变的输入与来源。再切回原活动检查基线是否保持，不预设风速单调增减。

自动路径由 `Geospace::prepare_iri/hwm/msis(request, policy)` 补齐缺省驱动，返回
`Prepared*`。`Prepared*::input()` 和来源访问器用于检查实际输入，`evaluate()`
同步运行模型；`Geospace::evaluate_*` 是 prepare 后立即 evaluate 的便捷组合。
完整显式驱动不会触发指数查询。

情景路径从一个 `Prepared*` 基线调用 `with_overrides` 或 `with_activity`。操作返回
新对象，不修改基线、不查库、不计算。显式覆盖的字段会清除对应观测 evidence；
`None` 表示继承。输出是受控敏感性结果，不能自动解释为物理因果。

对应的生产集成测试：

```bash
nix develop .#default --command env IONORAY_OFFLINE=1 \
  cargo test --locked --offline -p ionoray-geospace --no-default-features \
  --features standard --test automatic_drivers --test driver_scenarios
```

## 输入、输出与修改建议

- 输入位置是 WGS84 大地纬度、经度和椭球高；示例构造器使用度/公里。
- F10.7 使用 sfu；Kp/Rz12/IG12 是指数值。MSIS/HWM 的 ap/Ap 传递 GFZ 发布的
  指数数值（GFZ 说明乘 2 nT 才是 50° 地磁纬度的代表扰动）；Dst/AE 使用 nT。
- 序列化结果使用 SI：位置 rad/m、磁场 T、风速 m/s、温度 K、数密度 m^-3、
  质量密度 kg/m^3。
- MSIS storm-time 活动必须提供完整七分量历史，不能从 daily Ap 猜测。
- 修改 `direct_models` 时先替换一个输入并观察模型校验；修改 `automatic_drivers`
  时比较显式覆盖前后的 evidence；修改 `driver_scenarios` 时保持同一 baseline，
  为每个情景保留稳定 ID，并让单项错误继续返回。

固定离线快照的关键检查值为：IRI IG12 `-5.526666666666667`、HWM ap `2`、
MSIS 七分量 `[4, 2, 4, 3, 4, 3.375, 2.5]`。它们用于检查数据接线；模型科学
数值仍以各模型 crate 的官方参考测试为准。

Python、进程协议和指数维护分别见 [Python](python-models.md)、[CLI](cli-models.md)
和[数据维护](data-maintenance.md)。模型适用范围、资产与许可证见各模型 README。

## IRI 四驱动气候态

`IriInput` 仍只接收 Rz12、IG12、adjusted daily/81-day F10.7。内部 NRLMSIS00
磁活动响应显式关闭（`SWMI(9)=0`），不隐式补 Ap；这不是 Ap=0 的情景，不能用于
解释磁暴造成的温度变化。顶侧开关实际选择 IRI-cor2。
构建补丁只修改 Cargo OUT_DIR 中的 Fortran 副本，官方 archive/hash 保持原样。

```bash
nix develop .#default --command env IONORAY_OFFLINE=1 cargo run --locked --offline -p ionoray-iri --example iri2020_explicit_input
nix develop .#default --command python3 scripts/verify_iri2020_reference.py
nix develop .#default --command env IONORAY_OFFLINE=1 cargo test --locked --offline -p ionoray-iri --lib
```

显式例子输入为 2020-03-20 12 UTC、0°N/0°E、300 km，Rz12=IG12=10、两项 flux=70 sfu。
预期 Ne≈`1.138361761792e12 m^-3`、Tn≈`797.0247802734375 K`、
Te=Ti≈`1890.3697509765625 K`、hmF2≈`333.84674072265625 km`；cluster 离子密度为 `None`。
JSON 中 uom 高度序列化为米；参考 Fortran 的峰高单位为 km。原版无磁响应分支的
20 字段参考、开关和验证边界见 [IRI README](../crates/models/iri/README.md)。

VS Code 选 `Geospace Rust: IRI-2020`，在 example 的 `model.evaluate(&input)`
及打印前断点观察 `input`、`result.point`、`result.peaks`。若查看覆盖行为，复用
`Geospace Rust: driver scenarios (offline)`，在 `iri_scenarios` 的
`baseline.with_overrides(...)` 后观察 input/evidence；store 此时已关闭。
这些入口已配置；请在当前编辑器环境核对断点是否命中。

复现建议：在运行前预测，仅覆盖 `ig12` 时哪一项 evidence 会清除；运行后确认另外
三项证据及原始 baseline 保留。不要预设电子密度或温度随该项驱动单调变化。

### NRLMSIS 2.1 显式、自动驱动与情景

在仓库根目录运行独立模型、官方源码重放和专用装配测试：

```bash
nix develop .#default --command env IONORAY_OFFLINE=1 cargo run --locked --offline -p ionoray-msis --example nrlmsis21_explicit_input
nix develop .#default --command python3 scripts/verify_nrlmsis21_reference.py
nix develop .#default --command env IONORAY_OFFLINE=1 cargo test --locked --offline -p ionoray-msis --lib
nix develop .#default --command env IONORAY_OFFLINE=1 cargo test --locked --offline -p ionoray-geospace --no-default-features --features indices,msis --test msis_workflow
```

[显式 example](../crates/models/msis/examples/nrlmsis21_explicit_input.rs) 使用官方 Daily
参考点：1974-08-01 10:58:30 UTC、-24.4°/119.1°、399.1 km，F10.7a=86.5 sfu、
前一日 F10.7=84.8 sfu、daily Ap=6。stdout 的 `atmosphere_si` 保留温度、外逸层温度、
质量密度和九种粒子数密度；约为 T=`818.1225069303923 K`、
rho=`9.45728520123642e-13 kg/m3`。上游缺失密度哨兵转换成 `None`，不会填零。

MSIS 使用 Sun-Earth 实际距离的 observed F10.7，不使用 IRI 的 1 AU adjusted F10.7。
自动路径在查询日读取 centered 81-day observed F10.7a，在前一 UTC 日读取 observed F10.7；
固定离线点的两项值分别为 `69.9543209876543` 和 `68.1 sfu`。活动缺省为 StormTime，
必须读取 daily Ap、当前/前 3/6/9 小时 ap、12–33 小时八槽均值、36–57 小时八槽均值。
2020-07-01 12 UTC 的七项为 `[4, 2, 4, 3, 4, 3.375, 2.5]`。
缺任一历史会明确失败；`Daily(x)` 是调用者明确选择的另一种官方模式，不能由 daily Ap
推断或伪造 StormTime 历史。跨 UTC 年测试从 2020-01-01 00 UTC 回读到
2019-12-29 15 UTC，验证 20 个三小时槽没有在分区边界丢失。

官方归档只提供 Daily 输出表。StormTime 独立参考由
[重放脚本](../scripts/verify_nrlmsis21_reference.py) 编译未修改、固定 hash 的官方源码，
以 `switch_legacy(9)=-1` 直接调用 `MSISCALC`，不调用 Rust 或自动准备路径。两种模式均核对
12 个输出，double-precision 相对容差为 `5e-12`；来源、单位和适用边界见
[参考说明](../crates/models/msis/data/README.md)。这证明指定输入下的实现重放，不是实际大气观测验收。

`PreparedMsis::with_overrides` 复制基线：只覆盖 F10.7a 时只清除该 evidence，前一日 flux、
daily Ap 和 20 个三小时样本仍保留；覆盖活动则同时清除两类 Ap evidence。Prepared 在 store
关闭并移除临时 home 后仍可重复计算；负值、NaN 和无穷驱动在 evaluate 时显式报错。

VS Code 选 `Geospace Rust: NRLMSIS 2.1`，在 example 第 25 行 `evaluate` 前观察 `input`，
第 26 行观察 `result.atmosphere/provenance`。自动/情景可从
[msis_workflow.rs](../crates/geospace/tests/msis_workflow.rs) 的 CodeLens 进入，在
`prepare_msis` 后观察 `prepared.input()/indices()`，在 `home.close()` 后观察 baseline 与
scenario。终端检查通过不证明 VS Code GUI 断点已命中。

复现建议：把自动查询改到 2020-01-01 00 UTC，运行前预测前一日 flux 属于哪一年、
最早 ap 槽的 UTC；运行后核对七项历史及其 evidence。不要预设 StormTime 相比 Daily
会让温度或密度单调增大。
