# CLI 模型记录

`geospace model run --input PATH` 读取一个 JSON 对象；`geospace model batch --input PATH` 顺序读取 JSONL。`-` 表示标准输入，`--home` 仍为全局选项。stdout 只含每行一个 JSON 结果，日志在 stderr。

每个请求必须有 `model`、`mode`、`at`、`latitude_deg`、`longitude_deg`、`altitude_km`，可选 `id`、`drivers` 与 auto 的 `data_policy`。IGRF 不需数据目录；其他模型在 `direct` 下必须给完整驱动，在 `auto` 下只补缺省驱动。

批量完整读完但存在记录失败时退出 3；命令级读取/输出错误退出 1；Clap 用法错误退出 2。每条输出保留物理 `line` 与可解析的 `id`/`model`，成功为 `succeeded`，失败为 `failed`。

`direct` 必须给模型完整驱动；`auto` 只补缺省驱动并可给
`data_policy=ensure|offline|refresh`。坐标输入为 WGS84 度/公里；JSON evaluation
中的位置为 rad/m，模型结果为 SI，F10.7 为 sfu。失败不以零或伪造值代替。

四模型文件入口是仓库根目录的 [`examples/model_batch.jsonl`](../examples/model_batch.jsonl)。
13 个物理行包含四模型 direct、IRI/HWM/MSIS 的 `offline` auto、未知字段、错误形状、
超域坐标、不完整驱动、快照之外的缺失 Ap，以及失败后的成功行。auto 行明确设置
`data_policy=offline`，共用一次打开的隔离数据 home；内置离线快照只为本夹具提供驱动，
不是实际事件或实时数据验收。运行前可预测：第 12 行缺 2030 年 Ap，第 13 行 IGRF
仍成功；结果的 `line`/`id`/`model` 可逐行对回输入。

```bash
BATCH_DIR=$(mktemp -d /tmp/geospace-batch.XXXXXX)
nix develop .#default --command env IONORAY_OFFLINE=1 \
  cargo run --locked --offline -p ionoray-geospace --no-default-features \
  --features cli-standard --bin geospace -- \
  --home "$BATCH_DIR/home" model batch --input examples/model_batch.jsonl \
  > "$BATCH_DIR/results.jsonl"
```

这条命令预期退出 **3**，完整结果在新建临时目录的 `results.jsonl`，日志在 stderr。
每次先新建目录，不覆盖已有结果。单条文件可用 `model run --input PATH`，其输入是一个
JSON 对象；真实二进制测试同时核对该路径、stdout/stderr 分离、退出码和功能裁剪时的
`model_unavailable`。

集成测试将 CLI 的完整 `input`/数据来源严格对照同输入的 Rust 模型与离线准备接口，
对 `result` 数值使用相对容差 `1e-12`（跨进程 Fortran 浮点末位可能不同）；独立科学
数值参考及其科学边界见 [validation](validation.md)。VS Code 的 `Geospace CLI: record processing (offline)`
仍是小型 `cli_records` example 入口；正式 13 行文件批量用上述命令，调试可在
`crates/geospace/src/cli/model.rs` 的 `evaluate_line`、`process` 设置断点，观察
`line`、`request`、`session` 与 `record`。

离线小夹具和断点入口见 `crates/geospace/examples/cli_records.rs`：第一个和第三个
IGRF 记录成功，中间纬度 91 的记录失败，输出仍保留三行。运行或在 VS Code 选择
`Geospace CLI: record processing (offline)`：

```bash
nix develop .#default --command env IONORAY_OFFLINE=1 \
  cargo run --locked --offline -p ionoray-geospace --no-default-features \
  --features cli-standard --example cli_records
```

example 的三个断点为 `RECORDS`、`status`、`output`。继续进入生产处理可观察
`model.rs` 的 request 与单批 Session，以及 `model_evaluate.rs` 的显式/自动分支。
生产单元测试在 `model_tests.rs`，真实二进制测试在 `tests/cli_records.rs`。


单点允许格式化的多行 JSON；batch 每个物理行都产生一条结果，空白行、坏 JSON、
非 UTF-8 行为 `invalid_json`，结构/驱动/UTC 错误为 `invalid_request`，之后继续。
拒绝重复键、未知字段以及其他模型的驱动字段（即使值为 null）。
完整显式输入不打开 home；需要缺省驱动的行共享一个 store，打开失败也只尝试一次。
IGRF 不需要 store。未编译模型为 `model_unavailable`；已编译模型缺 indices 且
需要自动驱动时为 `data_unavailable`，全部显式仍可计算。

```bash
nix develop .#default --command env IONORAY_OFFLINE=1 cargo test --locked --offline -p ionoray-geospace --no-default-features --features cli-standard --lib cli::model::tests
nix develop .#default --command env IONORAY_OFFLINE=1 cargo test --locked --offline -p ionoray-geospace --no-default-features --features cli-standard --test cli_records
nix develop .#default --command env IONORAY_OFFLINE=1 cargo test --locked --offline -p ionoray-geospace --no-default-features --features cli,igrf --test cli_records
```

example 本身以进程退出 0 结束并断言处理状态 `RecordFailures`；真实 CLI 同样三行
则退出 **3**。不能用 example 的进程退出码代替 CLI 退出码验收。
