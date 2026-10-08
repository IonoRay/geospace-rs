> Python default: indices/Session, no model backend. Direct uncompiled model calls
> return `GeospaceError(code="model_unavailable")`; Prepared classes and Session
> model methods are exported only for enabled features. `capabilities()` reports
> the compiled set. Select `--features extension-module,igrf` or
> `--features extension-module,standard` with maturin. Debug preparation accepts
> `--features standard`. Full models require approved local build assets or
> explicit permission for build-time acquisition; see [releases](releases.md).

# Python 模型、Session 与 Prepared

Python 包名是 `ionoray_geospace`。绑定直接复用 Rust 模型和 `prepare_*`，不复制
模型或指数算法。完整签名见 [`__init__.pyi`](../python/ionoray_geospace/__init__.pyi)，
公开返回类型见 [`types.py`](../python/ionoray_geospace/types.py)。

## 构建与运行

首次使用先在仓库根目录执行 `nix develop .#default --command python3 -m venv .venv`。
下述离线构建要求 Nix 环境和 Cargo 依赖已缓存；首次 Rust 构建可去掉 Cargo 的
`--offline`，保留 `--locked` 与模型资产开关 `IONORAY_OFFLINE=1`。

```bash
nix develop .#default --command env IONORAY_OFFLINE=1 \
  cargo build --locked --offline -p ionoray-geospace \
  --no-default-features --features standard --example driver_scenarios
nix develop .#default --command python3 scripts/prepare_python_debug.py --features standard
nix develop .#default --command env IONORAY_OFFLINE=1 \
  .venv/bin/python -m unittest -v \
  python.tests.test_models python.tests.test_session python.tests.test_scenarios \
  python.tests.test_analysis
nix develop .#default --command env IONORAY_OFFLINE=1 \
  .venv/bin/python examples/driver_scenarios.py
```

准备脚本使用已有 Nix、maturin 和工作区 `.venv`，不安装网络依赖。测试和示例创建
隔离临时 home；运行时策略为 `offline`。VS Code 提供 `Geospace Python: model tests`
和 `Geospace Python: driver scenarios (offline)` 两个入口。

## 直接函数

参数全部是 keyword-only。共同位置参数为 `at`, `latitude_deg`, `longitude_deg`,
`altitude_km`；时间必须显式表示 UTC，位置为 WGS84 大地坐标和椭球高。

```python
import ionoray_geospace as gs

p = dict(
    at="2020-07-01T12:00:00Z",
    latitude_deg=30.0,
    longitude_deg=120.0,
    altitude_km=300.0,
)
field = gs.igrf(**p)
plasma = gs.iri(
    **p, rz12=0.0, ig12=-5.0, f107_daily=71.2, f107_81_day=72.1
)
wind = gs.hwm(**p, activity="disturbed", current_ap=2.0)
atmosphere = gs.msis(
    **p, f107a=69.9543209876543, f107_previous_day=68.1, ap_daily=4.0
)
```

IRI 四个驱动均必填。HWM 的 `quiet` 不接受 ap，`disturbed` 必须给 `current_ap`。
MSIS 的 `ap_daily` 与完整七字段 `ap_history` 必须恰选一个；不从 daily Ap 猜测历史。

## 自动驱动与情景

```python
import tempfile

with tempfile.TemporaryDirectory() as home:
    with gs.Session(home=home, data_policy="offline") as session:
        current = session.evaluate_iri(**p)
        baseline = session.prepare_iri(**p)

# Prepared 已拥有完整输入和来源，不依赖已关闭的 Session。
scenario = baseline.with_overrides(f107_daily=150.0)
result = scenario.evaluate()
```

`Session` 打开并复用一个 store/runtime，只能由创建它的线程调用。`prepare_iri/hwm/msis`
返回 Prepared；`evaluate_*` 是 prepare 与 evaluate 的组合。方法级 `data_policy=None`
继承 Session 策略。IGRF 无外部驱动，直接调用 `igrf`。

Prepared 的 `input`、`indices`（HWM 为 `ap_index`）返回字典副本。
`with_overrides`/`with_activity` 返回新对象，不修改基线、不查数据。显式覆盖会清除相应
evidence；科学输入域仍在 `evaluate()` 校验。

直接函数返回模型 Result；自动路径返回包含实际 input、来源和 result 的 Evaluation。
序列化单位为 T、rad、m、m/s、K、m^-3、kg/m^3，F10.7 为 sfu；缺失保持 `None`。

## 错误和生命周期

- 类型、缺少参数或未知参数：`TypeError`；非法值和模型输入域：`ValueError`。
- 数据或后端错误：`GeospaceError(RuntimeError)`，其 `code` 可用于稳定分类。
  数据覆盖缺口为 `data_unavailable`；明确请求刷新且来源检查失败为
  `data_refresh_failed`，即使本地覆盖完整也不静默视为成功。
- `close()` 幂等；关闭后的 Session 方法和跨线程调用抛 `RuntimeError`。
- 当前不提供 async Python、线程池、跨 fork 或 pickle。

## 批量分析记录

[`examples/driver_scenarios.py`](../examples/driver_scenarios.py) 顶部的 `DIRECT_CASES`
是可编辑的小输入列表：IGRF、IRI、HWM、MSIS 各有正常、非法、后续正常三条。
每条单独处理并输出一行 JSON。自动路径用一个离线 Session 获取 IRI/HWM/MSIS
基线；Session 关闭后，固定情景在 Prepared 上覆盖驱动并继续运行。示例使用隔离临时
home 和固定离线快照；更换时间/位置前应先核对该范围是否有数据。

记录的 `id` 在这组样例中固定且唯一，`model` 和 `mode` 标明调用方式。
直接成功记录保留 `request`、完整 `result`；自动成功记录保留完整
`evaluation`，其中含实际 `input`、`indices`（HWM 为 `ap_index`）和
`result`。失败记录含 `error.type/message`，数据错误另有 `error.code`；
情景失败仍保留已构建的输入和来源。下一条继续运行。
`analysis` 仅提取一个带单位的列和模型版本：`magnetic_magnitude_T`、
`electron_density_m3`、`northward_wind_m_s` 或 `mass_density_kg_m3`。
完整结果与来源仍在同一记录中，缺失值仍是 `null`。

调试入口为 `Geospace Python: driver scenarios (offline)`；在示例标出的三个
Breakpoint 位置依次查看关闭后的 Prepared、覆盖后的输入/来源、失败后的下一条。
Rust 同输入对照由 `python.tests.test_models` 与 `python.tests.test_analysis`
运行 `target/debug/examples/driver_scenarios` 完成；运行测试前需先构建该 Rust
example，完整命令见 [本指南的构建与运行命令](#构建与运行)。
