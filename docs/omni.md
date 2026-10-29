# OMNI CDF 下载与离线读取

目前支持 NASA SPDF 的 **1 分钟月 CDF**。默认每次在线下载都逐月检查 HRO2；没有可用 HRO2 时再尝试 HRO。已接受本地 HRO2 时，上游暂时缺文件不会使默认选择降级。`--product hro2` 或 `--product hro` 可固定产品，禁止产品回退。

HRO2 使用 Wind/SWE definitive 等离子体数据，HRO 使用另一套处理数据；两者覆盖和数值可能不同。HRO2 文件存在但变量缺测时，保留缺测，不从 HRO 拼接样本。网络、服务器错误和 CDF 解析错误不触发产品回退。官方说明：
https://omniweb.gsfc.nasa.gov/form/omni_min_def.html

## 正式入口

工作目录为仓库根目录。以下命令使用临时独立数据目录；实际使用可以去掉 `--home`，按 `IONORAY_HOME` 或项目默认目录解析，也可显式提供已有数据根目录。

```bash
nix develop .#default --command \
  cargo run --locked --offline -p ionoray-geospace --features cli,omni -- \
  --home /tmp/geospace-omni-cli-validation-20261028 omni download \
  --start 2024-05-10T00:00:00Z --end 2024-05-13T00:00:00Z
```

请求区间为 UTC 半开区间 `[start,end)`，这里只有一个相交月，下载整个 5 月文件。`--force` 强制重新获取月文件并比较内容。Cargo 的 `--offline` 仅禁止拉取构建依赖，`download` 仍是明确联网操作。首次构建尚无 `cdf` 依赖时，先运行不带 Cargo `--offline` 的相同命令获取锁定依赖。

```bash
nix develop .#default --command \
  cargo run --locked --offline -p ionoray-geospace --features cli,omni -- \
  --home /tmp/geospace-omni-cli-validation-20261028 omni read \
  --start 2024-05-10T00:00:00Z --end 2024-05-13T00:00:00Z \
  > /tmp/omni-may10-12.jsonl
```

`read` 只读取本地已接受文件，不联网；默认优先本地 HRO2，其次本地 HRO。若本地只有 HRO，之后需重新运行 `download` 才会检查官方 HRO2 是否已发布。缺少月份、文件损坏、解析和写出失败均以非零状态结束。物理变量缺测保留为数据状态，不当作下载失败。

输出约定：

- `download`：stdout 为单个 JSON 报告，包含所选产品、来源 URL、原始文件名、SHA-256、文件路径、变量属性和回退原因。
- `read`：stdout 为逐样本 JSONL；stderr 为一行覆盖／文件来源报告及可选 tracing。设置 `IONORAY_LOG_CONSOLE=off` 可只保留覆盖报告。
- 时间区间字段 `start_utc_ms`、`end_utc_ms` 为 UTC Unix 毫秒。
- 全部月份先检查本地文件是否存在；后续解析或写出失败仍可能留下部分输出，只有命令成功退出才表示读取完成。

## 产品标记与自动升级

再次执行相同 `download` 命令，会检查 HRO2 是否出现，即使本地 HRO 已完整覆盖请求窗口。HRO2 下载及校验成功后，后续默认读取切换到 HRO2。原 HRO 及其历史字节继续保留，已导出的 JSONL 不自动重写。

下载和离线读取使用同一个按月选择函数：自动策略为 HRO2 优先、HRO 次之；固定产品策略只选指定产品。HRO2 v01 的优先级高于 HRO v99。没有第二套 `current.json` 或当前版本数据库，选择由目录内的已接受文件和本次策略确定。

每个 `files` 项保留原有 `product`、`source_url`、`artifact` 等字段，新增明确身份字段：

| 字段 | 含义 |
|---|---|
| `product`, `cadence`, `partition` | 实际产品、`1min`、UTC 年月 |
| `file_version`, `filename`, `sha256` | 上游文件版本、所选请求文件名、内容身份 |
| `source_id`, `source_url` | 稳定产品／年月来源 ID、该已接受文件的请求 URL |
| `downloaded_at_utc_ms`, `accepted_at_utc_ms` | 内容下载时间、该来源接受这些字节的时间 |

文件名和版本取自已接受文件对应的成功请求，而不是后来可能失败的新版本请求。内容相同时下载和接受时间可能沿用原值，不能用它们判断是否升级。

`download` 的 `selections` 数组逐月报告：

- `previous_selected`、`selected`：同步前、后的完整文件身份。
- `selection_reason`：`preferred_product`、`fallback_product` 或 `explicit_product`。
- `change`：`first_acquired`、`product_upgraded`、`product_revised` 或 `unchanged`。同产品版本变化或 SHA-256 变化均为修订；单纯重查不会报升级。
- `source_check`：`checked` 表示已检查所选产品；`upstream_missing` 表示上游缺文件、保留了已接受的本地文件。优先候选缺失记录另见 `unavailable`。

例如，原 HRO 更新到 HRO2 后，`change` 为 `product_upgraded`，前后身份分别标记 `hro` 和 `hro2`。完整报告可以从 stdout 保存为 JSON。

| 情况 | 本次结果 |
|---|---|
| 当前 HRO，官方新增 HRO2 且校验通过 | 自动升级，后续默认读 HRO2 |
| 当前 HRO2，官方目录／文件暂时缺失 | 保留 HRO2；报告 `unchanged` + `upstream_missing`，不尝试降级 |
| 当前 HRO，上游两种产品均缺文件 | 保留本地 HRO，标记上游缺失 |
| 网络、服务器或 CDF 错误 | 同步返回错误；旧文件和选择保留，不输出成功同步报告 |
| 固定 `--product hro` | 本次选择 HRO，不改变以后自动读取时 HRO2 优先的规则 |

`read` 的 `selections` 标记 `source_check: offline_not_checked`，`previous_selected` 和 `change` 为 null，不推断历史升级。逐样本 JSONL 另有明确的 `product`、`cadence`、`partition`、`file_version`。离线读取本身不会发现上游新产品。

## 保存位置与更新

```text
<IONORAY_HOME>/
└── omni/
    ├── objects/omni-1min/
    │   ├── catalog.db
    │   ├── sha256/<prefix>/<digest>                 原始不可变 CDF / 目录响应
    │   └── cdaweb.gsfc.nasa.gov/.../hro2_1min/.../  已验证 CDF 浏览链接
    └── tmp/omni-1min/
```

共享存储层还会初始化空的 `omni/indices/` 目录；OMNI 不向其中写入指数数据库。下载报告中的 `browse_path` 是带原始 `.cdf` 文件名的可打开链接；离线报告的 `artifact.path` 是内容地址路径。解析时间序列不重复存入 Turso。

同月从官方目录选择数值最大的文件版本，完整版本保留在文件名中。HRO2/HRO 是科学产品区别，`v01/v02` 是文件版本区别，SHA-256 是内容身份；即使 URL 和版本号没变，上游内容更新也保留为新的 CAS 对象。仅在 CDF 的产品身份、时间、必要变量和记录结构验证通过后接受为当前文件。失败候选不会替换已接受文件。多月操作按月接受，后续月份失败不会删除已完成月份。

## 字段与科学语义

目前导出以下 CDF 原始变量名，并在文件报告中保留其全部变量属性（包括上游提供的 `UNITS`、`FILLVAL`、`VALIDMIN/MAX`、`DEPEND_0`）：

| 类型 | 变量 |
|---|---|
| IMF | `F`, `BX_GSE`, `BY_GSE`, `BZ_GSE`, `BY_GSM`, `BZ_GSM` |
| 太阳风 | `flow_speed`, `Vx`, `Vy`, `Vz`, `proton_density`, `T`, `Pressure` |
| 源卫星与统计 | `IMF`, `PLS`, `IMF_PTS`, `PLS_PTS`, `percent_interp` |
| 传播与不确定度信息 | `Timeshift`, `RMS_Timeshift`, `RMS_phase`, `Time_btwn_obs`, `RMS_SD_B`, `RMS_SD_fld_vec` |
| 地面分钟指数 | `AE_INDEX`, `AL_INDEX`, `AU_INDEX`, `SYM_D`, `SYM_H`, `ASY_D`, `ASY_H` |

物理变量的单位直接来自 CDF，例如速度 km/s、密度 cm⁻³、磁场 nT、动压 nPa。源卫星编号等属性可能没有 `UNITS`，不为其虚构单位。速度分量使用源产品的 GSE 约定；不做坐标变换，也不重新计算动压。

每行包含实际产品、原始文件名、SHA-256、零起始 `record_index`、原始 `cdf_epoch_ms` 与 UTC。`raw` 保留有限原始数值，包括填充值；`value` 仅在填充值、非有限值或无记录时为 null，并提供 `missing` 原因。超出上游有效范围的有限非填充值仍保留，同时标记 `outside_valid_range`。原始文件保存非有限值及全部未导出变量。

高分辨率 OMNI 的太阳风／IMF 已传播至地球弓激波鼻端，不再次平移。七个地磁指数保留地面 UTC，变量元数据明确标记 `time_reference: ground_utc`；太阳风等字段标记 `omni_bow_shock_nose`。HRO2 产品名不代表地面指数为最终值，指数成熟度不从该名称推断，原始变量属性保留。保留上游质量信息，不新增筛选、插值、平滑或重采样。覆盖报告区分 UTC 分钟记录缺口、各变量缺测及有效范围异常；记录完整不代表太阳风连续有效。

分钟 AE 使用 `AE_INDEX`，与现有 `indices` 小时 AE 接口分开；SYM-H 与 Dst 分别保留名称和来源，不互相替换。Hpo 和 PCN 的其他原生源见[活动指数指南](indices-activity.md)。已有月 CDF 无需为这些新增变量重新下载，更新程序后重新 `omni read` 即可。

## cdf-rs 能力边界

`cdf` 固定为 0.1.4。使用它的 `Cdf::read_cdf_file` 和公开 `get_record_vec` 读取接口；后者用于完整展开便捷接口留下的嵌套 VXR 后续链。业务层仅选择 OMNI 字段、遍历已解码记录并解释属性，不维护通用 CDF 封装、二进制解码、依赖 fork 或替代后端。

已验证的 2024 年 5 月 HRO/HRO2 文件采用 `CDF_EPOCH`、未压缩标量记录。当前对其他时间类型、压缩及稀疏变量明确报错，不把未验证格式报告为成功。未来官方文件格式变化需要新样本核验后再扩展。

非稀疏变量必须解码出全部索引记录；索引链缺失按结构错误拒绝，不伪装成上游物理缺测。物理缺测由原文件中的填充值及属性判断。

## 测试与调试

普通测试不访问 NASA，使用本地 HTTP 服务验证目录选择、回退限制、错误拒绝和离线无请求。

```bash
nix develop .#default --command \
  cargo test --locked --offline -p ionoray-omni
```

真实文件测试需要提供原始 CDF。默认寻找 `/tmp/geospace-omni-hro2-202405.cdf`、`/tmp/geospace-omni-hro-202405.cdf`，也可分别通过 `OMNI_TEST_CDF`、`OMNI_TEST_HRO_CDF` 显式指定。文件不存在时明确报告未执行；访问错误和断言失败正常失败。

```bash
nix develop .#default --command \
  cargo test --locked --offline -p ionoray-omni -- --ignored --nocapture
```

可单独运行或使用 CodeLens 调试：

- `parse::tests::official_may_2024`：完整 HRO2 月份、时间端点、缺测与原值保留。
- `parse::tests::official_hro_matches_independent_ascii`：与[官方 ASCII 三条原始记录](../crates/omni/tests/fixtures/README.md)独立比较。
- `store::tests::real_cdf_download_fallback_and_offline_read`：真实文件经本地 HTTP 下载、成功回退、服务关闭后的离线读取和损坏检测。
- `store::upgrade_tests::hro_upgrades_to_hro2_and_never_downgrades_on_absence`：同一目录连续运行，HRO 自动升级、重复检查、上游缺失不降级及固定 HRO。
- `store::upgrade_tests::failed_hro2_upgrade_keeps_accepted_hro_readable`：无效 CDF 和网络／服务器失败后仍可离线读原 HRO。
- `store::upgrade_tests::revisions_keep_exact_version_and_accepted_request_after_failure`：同字节新版本、同版本新字节和失败新版本的身份保留。这里仅修改真实 CDF 的版权文本字节模拟内容修订，不生成科学样本或科学结论。

只运行本轮升级场景（仍需前述两份真实 CDF）：

```bash
nix develop .#default --command \
  cargo test --locked --offline -p ionoray-omni upgrade_tests \
  -- --ignored --nocapture --test-threads=1
```

升级断点建议：`store.rs` 的 `OmniStore::download`（`previous`、`candidate`、`selected`、`source_check`），`selection.rs` 的 `change`（前后产品、版本与 SHA-256）。

VS Code 提供 `Geospace OMNI: read May 2024 (offline)`，输入包含已下载 `omni/` 子目录的父数据根目录。建议断点：`parse.rs` 的 `collect`（`index.first_vec`、`block.records`、`values`），`read.rs` 的 `OmniStore::read`（`record_index`、`product`、`sample`）。CLI 检查与编辑器断点检查分别记录，配置存在不代表断点已验证。

在线下载统一使用 `Geospace Data: sync all indices for year (online)`，输入数据根目录和 UTC 年份；OMNI 同步整个日历年，默认 `auto`（HRO2 优先）及一分钟分辨率。独立在线下载 launch 已移除，离线读取 launch 保留。日期范围和显式产品选择仍使用本指南的 `omni download` CLI。数据写入所选父目录的 `omni/` 子目录。

分辨率当前仅提供 `1min`，对应 CLI `--cadence 1min`；尚未接入 5 分钟或小时数据，CLI 会拒绝这些值。强制选项对应 `--force=false` / `--force=true`，原有不带值的 `--force` 仍表示强制重下。`false` 仍会检查 HRO2 是否出现或更新。

## 2026-10-28 工程验证记录

正式 CLI 从 NASA 下载并接受 `omni_hro2_1min_20240501_v01.cdf`，SHA-256：
`838f9d951ddd70d60c28ce3ac498b39c943a0f4ab9919a32bb7790bb72e127c3`。
验证目录为 `/tmp/geospace-omni-cli-validation-20261028/omni/`。

离线读取 `[2024-05-10T00:00:00Z,2024-05-13T00:00:00Z)` 得到 4,320 条分钟记录，无时间记录缺口；`flow_speed`、`proton_density`、`Pressure` 各缺测 1,832 条，`BZ_GSM` 缺测 226 条。`Vz` 有 96 条超出上游有效范围，原值保留。这些计数属于所选文件与窗口，不代表其他产品或全历史覆盖。

真实文件测试另外核验 HRO2/HRO 完整 44,640 条月记录、本地 HTTP 下的成功产品回退、关闭服务后的离线读取、文件损坏检测；HRO 的三条时间记录与八个物理变量对照官方 ASCII（含缺测），按上游输出精度比较。实际线上下载选择了 HRO2，没有触发线上 HRO 回退。

产品升级补充验证：三项新增升级测试使用已有真实月文件和本地 HTTP 服务，验证 HRO→HRO2、重复检查、目录／文件缺失不降级、固定 HRO、网络／解析失败保留旧输入，以及同产品文件版本和字节修订。版本修订场景中的版权字节修改仅用于模拟上游内容变化。

正式 CLI 对上述既有临时目录执行在线重查，报告 `hro2`、`v01`、`unchanged`、`checked`；随后按 `IONORAY_HOME` 离线读取三条分钟记录，所选 SHA-256 与在线报告一致，状态为 `offline_not_checked`，未推断历史变化。在线报告位于 `/tmp/geospace-omni-selection-download.json`，离线样本位于 `/tmp/geospace-omni-selection-read.jsonl`。共享存储 36 项库测试及相关 OMNI／CLI 测试、格式、Clippy 和文档检查通过。线上未模拟产品发布先后，自动升级证据来自本地服务承载的真实文件测试。

这些属于格式、工程与样本数值对照证据，不是科学分析验收。VS Code 断点和本人实际运行尚待确认。
