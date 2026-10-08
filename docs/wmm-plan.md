# WMM2025 接入计划

状态：未实施。本文是未来工作范围，不表示已有 WMM 代码、系数资产或数值验收。
当前 [模型路线](model-roadmap.md) 的下一里程碑仍是 T89d；只有用户明确选择 WMM 后
才按本页推进。开始实施前必须重新核对官方版本、许可和当前仓库接口。

## 目标与边界

首版实现独立的 `ionoray-wmm` 原生 Rust 模型，仅支持标准 WMM2025。输入沿用
`QueryPoint` 的 UTC 时间、WGS84 大地经纬度和椭球高；输出提供 ENU 磁场、总强度、
水平强度、磁偏角、磁倾角及 provenance。

不包含 WMM2020、WMMHR、MSL 高程转换、自动换版、用户自定义系数、磁力线追踪、
批量框架或性能优化。WMM 是显式输入模型，不读取指数、Turso、网络或数据 home。

## 开始前必须固定的合同

- 官方 WMM2025 系数、技术报告、软件和许可证的版本、来源 URL 与 SHA-256；
- 有效时间和高度范围，端点包含关系，以及范围外错误；
- 球谐正规化、WGS84 到地心坐标转换、ENU 方向、D/I 符号和 SV 单位；
- 水平场过小时磁偏角的无定义或警戒表达，不输出伪零或无穷；
- 官方计算器或软件提供的独立测试向量与容差。

来源优先使用 [NOAA NCEI WMM 页面](https://www.ncei.noaa.gov/products/world-magnetic-model)
和随正式发布附带的报告、系数、软件与许可。实现前重新下载并记录固定资产；网页说明
不能替代提交资产的哈希和许可审查。

## 实施顺序

1. 固定上述科学合同和参考向量，记录现有 IGRF 回归基线。
2. 评估 IGRF 球谐内核是否值得提取为小型共享 crate。只有 IGRF 对照保持不变且
   WMM 确实复用时才提取；不要把内核抽象与 WMM 科学实现一次性混在一起。
3. 实现 `Wmm`, `WmmVersion::Wmm2025`, `WmmInput`, `WmmResult`, `WmmError`，并
   嵌入、校验固定系数。错误必须显式，不返回占位结果。
4. 先完成独立模型参考测试，再接入 geospace feature/re-export、显式 Rust example、
   Python 直接函数和 CLI `direct` 模式。WMM 无自动驱动和 Session prepare 方法。
5. 更新 README、[Rust 示例](rust-models.md)、[Python](python-models.md)、
   [CLI](cli-models.md) 与[架构](architecture.md)，随后完成格式、定向测试、严格
   Clippy、rustdoc 和示例运行。

## 验收

- 官方向量覆盖赤道、中纬、高纬/近极、不同经度、高度、有效期起止和 SV；
- 与参考实现逐字段比较，容差有来源，不从本实现输出反造预期；
- IGRF 迁移前后参考结果不变；WMM-only facade 可编译运行；
- Rust/Python/CLI 同输入结果、单位和 provenance 一致；
- 模型 crate 没有 indices/store/network/home 依赖，生产 Rust 文件均少于 500 行；
- GUI、平台、科学适用性与自动检查分别记录，不以编译通过代替科学验收。

本计划不授权下载资产、修改代码、发布或迁移现有 IGRF。用户确认开始实现后，再以
当前现场为准制定首个可运行功能。
