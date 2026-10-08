# Data maintenance

Operations resolve an explicit home, `IONORAY_HOME`, or
`~/.ionoray/geospace-rs` in that priority order.
Indices use independent catalogs and CAS pools under `objects/<scope>/`, with
scopes `dst`, `ae`, `kp-ap-f107`, `iri-ig-rz`, and `iri-apf107`.
Year databases remain under `indices/<provider>/<dataset>/<year>.db`.

When no explicit path or `IONORAY_HOME` is supplied, an existing legacy
`~/.ionoray/indices` or `~/.ionoray/objects` makes the implicit choice
ambiguous. Opening the store then fails and requires the caller to select either
the old root (`--home "$HOME/.ionoray"` or `IONORAY_HOME`) or the new root
(`--home "$HOME/.ionoray/geospace-rs"`). No automatic migration, deletion,
copy, or replacement download occurs.

## Three offline controls

Cargo `--offline` only prevents fetching build dependencies. `IONORAY_OFFLINE=1`
only selects packaged assets while Cargo builds model backends. Neither setting
changes index HTTP behavior at runtime. Runtime callers choose that behavior
with `DataPolicy`, `SyncMode` for a bounded `sync_range`, or `SyncPolicy` for
annual maintenance: `Offline` forbids HTTP, `AlwaysCheck` checks for changes,
and `ForceDownload` fetches every source body. Use the runtime policy to reason
about data effects, even when the build command contains both offline flags.

## Lifecycle

| Operation | Network | CAS bodies | Year database | Intended use |
|---|---:|---:|---:|---|
| `query_year` | HEAD | unchanged | unchanged | inspect upstream availability and metadata |
| `download_year` | HEAD/GET | update if needed | unchanged | maintain validated raw files |
| `sync_year` | HEAD/GET, then cache fallback | update if needed | import releases | normal upstream synchronization |
| `ensure_year` | only if incomplete | update if needed | import releases | application startup |
| `verify_year` | no | hash current/history and sample-source objects | verify actual field coverage | local integrity audit |
| `reindex_year` / offline sync | no | accepted objects first, then packaged model-driver fallback | rebuild/import | disconnected startup or recovery |
| `repair_year` | GET | force redownload | import releases | recover missing or corrupt content |

The CLI exposes the same operations under `geospace indices`. Stable results go
to stdout as JSON; trace events go to stderr as newline-delimited JSON.

`IndexStore::sync_range(RangeRequest)` accepts one dataset, required fields,
a UTC half-open interval `[start, end)`, `SyncMode`, and a `force` flag.
`Offline` permits only local recovery, `Ensure` fetches missing field coverage,
and `Refresh` also checks existing sources. Empty/reversed intervals, fields from
another dataset, and offline plus force are rejected before HTTP access.
`RangeSyncReport` separates `coverage_status` (usable local values) from
`source_check_status` (`not_requested`, `complete`, or `failed`). Its `status`
remains strict: a coverage gap or failed requested check yields `partial`.
The complete `attempts` list retains source identity, typed failure reason,
and operational outcome; `diagnostic()` gives a short summary. Physical
download counts and initial/append/backfill/revision counts remain available.
With full local coverage, `Ensure` skips upstream and reports
`source_check_status=not_requested`; `Refresh` checks upstream and can fail
while `coverage_status=complete`. `Offline` makes no HTTP request.

```bash
geospace indices sync-range --dataset dst \
  --start 2020-01-15T00:00:00Z --end 2020-04-10T00:00:00Z --mode ensure
geospace indices sync-range --dataset kp-ap-f107 --field ap3h,f107 \
  --start 2020-07-01T12:00:00Z --end 2020-07-01T15:00:00Z --mode offline
```

Omitting `--field` requests every field belonging to the selected dataset.
Range CLI output is JSON: complete exits 0; partial reports and execution or
input failures exit 1 (CLI syntax errors use Clap's exit code 2). A partial
report remains on stdout, with its short error summary on stderr. Invalid
requests fail before a range report can be produced.
Model JSONL failures distinguish `data_refresh_failed` from `data_unavailable`.
The annual management commands remain available. Pure `indices read` never contacts HTTP.

HWM prepares only its current ap sample. IRI prepares the requested day's driver
sources; NRLMSIS prepares one combined driver interval and then explicitly reads
every required sample. Automatic model preparation requests only those fields;
each `Prepared*` retains selected evidence. These operations remain in the
assembly/data layers; model crates receive only explicit inputs.

The crate packages SHA-256-pinned snapshots of the three rolling sources used by
the integrated models: GFZ Kp/Ap/F10.7, IRI IG/Rz, and IRI AP/F10.7. Normal and
forced synchronization fall back to a matching snapshot when remote download or
validation fails. `SyncPolicy::Offline` never sends an HTTP request: it first
recovers accepted local objects and their missing yearly applications, then uses
matching packaged snapshots when no accepted body was usable. Dst and AE are excluded because
packaging the complete Kyoto history would be disproportionate; offline access
to those datasets requires an existing local CAS.

## F10.7 gap handling

GFZ marks missing observed F10.7 values as `-1.0`. Model-ready normalization
preserves the raw artifact identity and only fills a gap when real observations
bound both sides and the gap is at most two consecutive calendar days. Each
filled value uses bounded linear interpolation. Longer or unbounded gaps remain
unavailable; they are never zero-filled, extrapolated, or silently omitted from
an average.

The yearly database stores the interpolation gap length for a filled daily
F10.7 value and the number of interpolated inputs used by each centered 81-day
mean. `IndexSample.derivation` exposes this information to every caller. Parsing
emits `indices.f107.gap.interpolated` or `indices.f107.gap.unfilled` at `warn`;
NRLMSIS preparation emits `geospace.msis.driver.interpolated` whenever a model
driver consumes an interpolated value. Repeated evaluations of the same driver
sample through one `Geospace` instance emit that consumption warning once, so a
height profile remains explicit without duplicating the same warning per point.

The GFZ schema and parser release identity include this normalization contract.
Opening an older yearly database detects the schema hash mismatch, rebuilds the
year database, and reimports the immutable CAS or packaged source rather than
reusing a release produced by the former all-or-nothing averaging algorithm.

## Idempotency and versions

The default synchronization checks the stable source identity and URL, then
compares ETag, Last-Modified, Content-Length, local size, and local mtime. A
matching check records a new fetch attempt without transferring the body. If the
metadata differs or cannot establish equality, the body is streamed and hashed.

SHA-256 identifies raw bytes. Downloads are candidates until domain validation
accepts them; a failed parse cannot replace the accepted HTTP comparison baseline.
Accepted objects have relative timestamped symlinks mirroring the canonical URL
under their scope. Local acceptance time names the link, while remote modified
time is recorded separately. The catalog is authoritative for rebuilding views.

Semantic comparison distinguishes initial imports, equivalent records, append,
historical backfill, and revision. Explicit missing values remain missing.
Active and retained-history pointers select each monthly or rolling partition;
readers do not scan history to fill gaps. Each selected sample keeps its source
provenance. Pure append preserves an existing historical snapshot; a revision
replaces that history with the previous current snapshot.

After every terminal fetch attempt and successful release import, Turso runs a
truncating WAL checkpoint. Checkpoint status, frame counts, and elapsed time are
part of the JSON trace. A busy checkpoint is treated as a failed durability step,
not as successful maintenance.

Use `SyncPolicy::ForceDownload`, `geospace indices sync --policy
force-download`, or `geospace indices repair` to redownload complete bodies at
any time. Forced mode hashes local content first and compares the downloaded
SHA-256, so it can confirm unchanged upstream bytes or replace a corrupt local
object without treating URL metadata as identity.

## 2020 reference workflow

```bash
geospace data init
geospace indices query --year 2020
geospace indices sync --year 2020
geospace indices verify --year 2020
geospace indices read --at "2020-07-01T12:00:00 UTC"
```

## Online year sync

VS Code 的 `Geospace Data: sync all indices for year (online)` 会先通过 Nix 构建仅含
CLI 和 indices 的 `target/debug/geospace`，再提示输入 UTC 年份并执行：

```text
geospace --home <输入目录> indices sync --year <输入年份>
```

该命令按顺序同步 `kp-ap-f107`、`dst`、`ae`、`iri-ig-rz` 和 `iri-apf107`，完成
远端检查、必要下载、校验和年度数据库导入。launch 会要求输入写入目录；该目录优先于
`IONORAY_HOME`，未设置两者时才使用 `~/.ionoray/geospace-rs`。它不会使用离线 example
的临时目录。运行完成后应检查 JSON 中的
下载、导入、变更和 `cache_files_used`，不能仅凭进程成功断言所有正文均来自远端。

这个 launch 明确允许联网并修改上述数据目录，因此 `stopOnEntry=false`，启动后直接执行。
需要只补缺口时改用 `indices ensure --year YEAR`；需要强制重下时使用
`indices sync --year YEAR --policy force-download` 或 `indices repair --year YEAR`。

The offline Rust entry point is `crates/geospace/examples/indices_offline.rs`.
It uses an isolated temporary home, prepares bounded ranges, then prints typed
values with quality and provenance; its missing Dst range remains explicit and
performs no HTTP requests.

## Indices examples

从仓库根运行。`indices_offline` 是离线学习入口：它使用独立 TempDir，不读取用户
home，四个 range report 都打印 `source_queries=0`，然后输出含 quality/provenance 的
typed sample。其固定输入为 `2020-07-01T12:00:00 UTC` 起的 1 ms 半开区间。`ap=2`
来自 GFZ fixture 第 32365 行的 12–15 UTC ap 槽；fixture 的来源和 SHA-256 见
[cache README](../crates/indices/cache/README.md)。没有 Dst fixture，因此 Dst report
保留 `partial` 缺口，而不是伪造样本。

`indices_maintenance` 是单独的在线年度维护入口。它在打开 store 前要求 `--home PATH`
和单年 `--year YEAR`，或闭区间 `--start-year YEAR --end-year YEAR`；范围端点均包含在内，
可显式覆盖 1958 到当前 UTC 年。它拒绝未来年份、反序范围、缺少端点或混用两种范围形式。
默认 `--policy always-check`，也接受 `force-download` 与 `offline`；只有后者禁止运行时
HTTP。它先打印解析后的 home、年份范围、策略和是否允许联网，再逐年调用 `sync_year`。

| main | 目的 / 真实输出预期 | VS Code launch / 三个断点 |
|---|---|---|
| [indices_offline.rs](../crates/geospace/examples/indices_offline.rs) | 四个 report 的 `source_queries=0`；typed read 输出 ap=`2`、observed F10.7=`68.9`、IG12=`-5.526666666666667`、Rz12=`5.940666666666667`、adjusted F10.7=`71.2`、81-day=`72.1`，均有 quality/provenance；Dst partial | `Geospace Data: inspect indices (offline)`；`sync_range(range)`、`print_sample` 的 ap、`print_report` 的 Dst 缺口 |
| [indices_maintenance.rs](../crates/geospace/examples/indices_maintenance.rs) | 显式 home、单年或闭区间的年度 `sync_year`；逐年输出 JSON，某年失败后继续并最终失败 | `Geospace Data: maintenance schedule (online)`；先检查回显的范围和联网许可，再检查 yearly report |

```bash
nix develop .#default --command env IONORAY_OFFLINE=1 cargo run --locked --offline -p ionoray-geospace --no-default-features --features indices --example indices_offline
# Online: select your existing home before running; execution does not pause for confirmation.
nix develop .#default --command cargo run --locked --offline -p ionoray-geospace --no-default-features --features indices --example indices_maintenance -- --home "$HOME/.ionoray/geospace-rs" --year 2020
# Optional historical range. This is an explicit maintenance request; do not run it merely to test the example.
nix develop .#default --command cargo run --locked --offline -p ionoray-geospace --no-default-features --features indices --example indices_maintenance -- --home "$HOME/.ionoray/geospace-rs" --start-year 1958 --end-year 2020
nix develop .#default --command env IONORAY_OFFLINE=1 cargo test --locked --offline -p ionoray-indices --lib store::tests::offline_range_prepares_and_typed_reads_keep_provenance -- --exact
nix develop .#default --command env IONORAY_OFFLINE=1 cargo test --locked --offline -p ionoray-geospace --no-default-features --features indices --test indices_maintenance
```

maintenance 的每条 JSON 保留 `operation`、`id`、原始 `request`（或 `year/policy`）、
`report` 和 `error`。Range 报告的 `partial` 是需检查的缺口状态，不能把返回 Ok 当成
数据齐全；参数或基础设施错误保留原 job，后续 job 继续。测试
`schedule_preserves_order_and_offline_partial_reports` 与
`schedule_retains_failed_request_and_runs_next_job` 共用 example 的普通循环，直接
调用生产维护 API。参数测试只解析参数并检查范围选择，**不代表年度维护已运行**。
每年调用 `sync_year`，输出报告或错误后继续下一年；存在失败年份时最终返回错误。
年度同步不保证上游每个时段都有数据，应检查报告和覆盖缺口。
Python 没有新增 store binding；外部调度可逐项调用 CLI 并检查退出码和 JSON：
`sync-range` 的 complete/partial 分别是 0/2，其他失败是 1，语法错误也可能是 2。
所以须同时核对 stdout 是否为维护报告，不以退出码 2 单独推断 coverage。

## Cache upkeep and diagnostics

构建/运行不需要刷新内置快照。明确需要更新资产时，现有入口是
`nix develop .#default --command python3 scripts/update-cache.py`；`--check` 为只读试查，
`--indices-only` 只处理滚动指数，可显式指定 `--remote-host`。
来源与固定哈希见 [cache README](../crates/indices/cache/README.md)。
CLI 初始化 tracing；离线学习 example 可不初始化。日志 sink 和环境变量见
[architecture](architecture.md#tracing-contract)，不要把 tracing 初始化当作数据 API 前置。
VS Code 的三个断点语句为 `sync_range(range)`、输出 ap 的 `print_sample` 和输出 Dst
缺口的 `print_report`。请在当前编辑器环境分别核对这些断点；
配置存在不证明调试成功，检查方式见 [contributing](../CONTRIBUTING.md#debugging-and-changes)。
