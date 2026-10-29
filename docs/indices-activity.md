# 新增活动指数：下载、离线读取和核验

本轮完整接入 OMNI 的七个地面分钟指数，以及 GFZ Hpo/apo 和 DTU PCN。按用户决定，PCS 暂不接入，申请方式见文末。

| 数据 | 正式入口 | 原生尺度／单位 |
|---|---|---|
| AE、AL、AU、SYM-D/H、ASY-D/H | `omni download` / `omni read` | 1 分钟，nT；字段名见 [OMNI 指南](omni.md) |
| Hp30、ap30 | `indices activity download --dataset hp30` | 30 分钟，无量纲 |
| Hp60、ap60 | 同上，`--dataset hp60` | 1 小时，无量纲 |
| PCN | 同上，`--dataset pcn` | 1 分钟，mV/m |

实际语法是 `indices activity download --dataset ...` 或 `indices activity read --dataset ...`。当前旧的 Kp/ap/Ap/F10.7/Dst/小时 AE/IRI 接口继续使用，不改变其分辨率和模型驱动方法。

## 下载与读取

在仓库根目录运行，通过 `--home` 选择父数据根目录；省略时沿用 `IONORAY_HOME` 和项目默认解析规则。以下示例只写入独立临时目录。

```bash
nix develop .#default --command \
  cargo run --locked --offline -p ionoray-geospace --features cli,indices -- \
  --home /tmp/geospace-activity-cli-20261029 indices activity download \
  --dataset all --start 2024-05-10 --end 2024-05-13
```

`activity --dataset all` 指本组的 hp30、hp60、pcn 三个物理源产品，各自失败会分别报告，并继续其余产品；存在失败时最终返回非零状态。要同时同步核心指数与 OMNI，使用统一年度入口 `indices sync --year 2024`，见[年度同步](data-maintenance.md#online-year-sync)。OMNI 七个分钟指数使用已有 `omni` 读取入口，月文件已存在时可直接重新读取，无需重下。

日期形式表示午夜 UTC，区间为 `[start,end)`；示例涵盖 5 月 10—12 日。也接受完整 UTC 时间。按照原生区间起点筛选，不对跨越边界的半小时／小时观测做切割或重采样。分辨率由所选产品决定，不能把 Hp60 标成分钟数据。

```bash
nix develop .#default --command \
  cargo run --locked --offline -p ionoray-geospace --features cli,indices -- \
  --home /tmp/geospace-activity-cli-20261029 indices activity read \
  --dataset all --start 2024-05-10 --end 2024-05-13 \
  > /tmp/activity-may10-12.jsonl
```

下载的 stdout 为各产品的 JSON 报告；读取的 stdout 为 JSONL，stderr 为来源／覆盖报告及可选日志。`IONORAY_LOG_CONSOLE=off` 关闭程序 tracing。读取先校验原始 CAS 哈希，不联网；数据集按选择顺序输出，各产品内部按 UTC 递增。缺少原始输入、哈希错误、解析或写出错误返回非零状态。

`--force` 或 `--force=true` 强制重新下载并比较 SHA-256；默认 `false` 仍检查上游，包括更成熟的 PCN。Cargo `--offline` 仅约束依赖，不禁止明确的下载操作。

## 物理源、存储与更新

- GFZ 官方 Hp30/ap30 和 Hp60/ap60 是两个完整滚动 ASCII 文件，当前约 44 MB 和 22 MB；请求三天仍需首次取得整个物理文件，后续按元数据检查复用。每次运行同一物理文件只请求一次，不逐日重复下载。程序只输出请求区间。
- PCN 优先尝试请求年份的 definitive 年度 IAGA 文件（2024 文件约 21 MB），不存在时逐日尝试 provisional CSV，再尝试 quicklook CSV。年度源和每日源身份明确区分；每次在线运行都重新检查更成熟来源。
- 网络／服务器／解析错误不伪装成“没有文件”，不自动用较低成熟度掩盖错误。上游缺文件时保留已接受的同源输入，不把 definitive 降成 provisional／quicklook。
- 原始文件保存在 `<IONORAY_HOME>/objects/{hp30,hp60,pcn}/` 各自的目录数据库和不可变 CAS 下，浏览链接保留上游主机、路径和文件名。无新的全局数据库、无源文件覆盖删除，也不把时间序列再复制进新的年度数据库。
- 只有格式、原生时间网格、PCN 年度／每日来源身份验证通过后才接受新文件。同源修订不能缩短已接受的整体起止覆盖、减少记录总数，或删除当前请求窗口中已有的时间记录；显式填充值更正仍作为来源修订保留。旧 CAS 字节保留。

文件报告包含真实来源 URL、SHA-256、接受前哈希、`first_acquired/revised/unchanged` 和 `checked/upstream_missing/offline_not_checked`，并保留原始头部、许可证和出处。下载完成表示源文件已验证并落盘；是否满足分析输入还须检查覆盖报告。

## 缺测、质量和时间

Hp 的时刻是上游 `hh.h` 所示区间起点，`hh._m` 的中点标签保留在 `source_flags.mid_hour_utc`；程序核对重复时间列但不平移观测。Hp 可以超过 9，不裁剪。`-1.000`（Hp）与 `-1`（ap）是来源缺测标记；保留 `raw`，`value` 为 null。保留 `D` 列原文；官方目前将其保留为 0，不将其解释为 definitive 标志。

PCN 原生为分钟 UTC，负值有效。`99999.00` 是缺测标记；不将负 PCN 当作无效值。definitive 头部必须与目录分类一致，CSV 的成熟度来自实际选中的 provisional／quicklook 路径。质量标记只表示供应方出版阶段，不等于科学验收。

每条记录带数据集、原生秒数、区间起止 UTC、单位、质量、来源标志、源 ID、原文件 SHA-256 和一基行号。覆盖报告分别统计原生时间记录缺口与已有记录中的各变量缺测，不插值、平滑、裁剪、拼接半球或重采样。

OMNI 的七个地磁指数保留 `ground_utc`，太阳风／IMF 保留其弓激波鼻端传播约定；HRO2 名称不提升地面指数的成熟度。

## 测试与接管

真实微型样本及独立 GFZ API、ISGI 小时 PCN 对照见[fixture 记录](../crates/indices/tests/fixtures/activity/README.md)。后者只在测试中将 60 个真实分钟观测取均值核对，生产输出仍是分钟数据。文件缺测、负值和超过 9 的真实 Hpo 值均保留。

```bash
nix develop .#default --command \
  cargo test --locked --offline -p ionoray-indices activity -- --nocapture

nix develop .#default --command \
  cargo test --locked --offline -p ionoray-omni official_ -- --ignored --nocapture
```

独立调试测试：`activity::parse::tests::hp30_matches_official_api_and_preserves_open_ended_values`、`activity::parse::tests::pcn_matches_independent_hourly_reference_without_resampling_output`；存储路径测试在 `activity::store::tests`，覆盖重复源去重、保留修订、拒绝截断、HTTP/解析失败、离线读取与损坏检测。OMNI 的真实文件测试路径与缺文件处理规则见原指南。

VS Code 在线下载统一使用 `Geospace Data: sync all indices for year (online)`，独立 Hpo/PCN 在线下载 launch 已移除；`Geospace indices: read Hpo/PCN date range (offline)` 保留，可选择数据根目录、产品和 UTC 起止。预构建复用 Nix 的 `geospace: build indices CLI`。建议断点：`activity/store.rs` 的 `download_slots`（`candidate`、`accepted`）、`validate_candidate`（新旧覆盖），`activity/parse.rs` 的 `hpo` / `pc`（`time`、`values`），以及 `activity/read.rs` 的 `read_selected`（`row`、`source_id`、缺口）。实际编辑器断点仍待本人核验。

### 2026-10-29 工程核验

在独立临时 home `/tmp/geospace-activity-cli-20261029`，实际从官方源下载并离线读取 `[2024-05-10,2024-05-13)`：Hp30/ap30 为 144 条，Hp60/ap60 为 72 条，definitive PCN 为 4320 条；三者均无时间缺口及标量缺测。离线记录哈希与下载报告一致，原生单位、时间间隔、负 PCN 和超过 9 的 Hp 保留。OMNI 现有 HRO2 真文件读取 2024-05-10 18:00—18:03 UTC，输出三条记录及全部七个地面指数。

格式、Clippy、相关测试和文档构建通过；新增活动指数的 10 项测试和 CLI 产品选择测试通过，OMNI 两项官方真实文件对照测试通过。launch JSON、输入引用、构建任务及 CLI 实参绑定已核验；VS Code F5／断点与本人科学使用核验尚未执行。GFZ 原文 fixture 保留上游头部的四处行尾空格，因此全量 `git diff --check` 会报告这四处；排除这两个原文文件后检查通过。

## PCS：本轮不接入，后续申请方式

按用户决定，当前不提供 PCS 代码入口或模拟数据。PC 官方归档 `https://pcindex.org/archive` 本轮连接失败；ISGI 公共绘图返回的是小时均值，不能标成分钟 PCS。

ISGI 自动服务面向科研机构提供申请，官方流程：

1. 从[服务申请说明](https://isgi.unistra.fr/ws)进入[官方联系页](https://isgi.unistra.fr/locate_us.php)，向页面列出的联系人发送申请邮件，说明机构及科研数据使用需求。
2. ISGI 会发送简短问卷，登记申请机构；完成后提供该机构的访问流程与连接参数。
3. 后续取得服务用户名和真实 PCS 返回样本后，再核对格式、单位、版本／成熟度和缺口后接入。

[官方接口文档](https://isgi.unistra.fr/ws_documentation.php)说明 `user` 为 ISGI 分配的用户名；PC/PCN/PCS 每次请求最多一个月，可选 IAGA2002、JSON 等输出。使用和发表时按[供应方数据政策](https://isgi.unistra.fr/policy.php)处理质量、引用与致谢。这里仅提供操作方式，不代发邮件或申请账号。
