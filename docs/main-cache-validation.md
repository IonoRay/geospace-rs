# 两分支实施与验证记录

实施日期：2026-10-08 至 2026-10-09，macOS arm64。工程实现、自动验证与本人验收分别记录。公开分发授权尚未完成，不能把此记录当作所有材料已经获准发行。

## 实施范围

`main` 使用无快照的新根 A，加一个共同修复提交 B；`cache-snapshots` 从 B 增加八项固定资产。旧根 `211d794` 与此前本地历史保留为备份引用，不进入新 main 的祖先链。共同代码、版本、科学输入选择、许可证、锁文件、脚本和资产清单相同。

共同实现已完成：模型构建期可选来源与固定哈希核验、indices 可选嵌入及原有离线/刷新语义、坐标 Deserialize 不变量、非 ASCII 摘要错误处理、随机且受 guard 管理的模型资产目录、Python 基座/四个模型/standard features、按实际编入接口报告 capabilities，以及独立同步与外部缓存准备入口。模型运行时不查网络、Turso 或缓存根。

八项资产保持原字节；指数缺失值、负值、来源、覆盖范围和固定模型版本保持不变。HWM 测试参考从已核验的官方 `Check/gfortran.txt` 取原有 3083 字节摘录。既有测试中的长度断言仅因新版 Clippy 修改表达形式，没有改变判断条件。

## 工具链与现场保护

方案要求保留用户已暂存的 `flake.lock` 更新。本轮未把该更新自行纳入提交：工作区/暂存区原字节 SHA-256 是 `fd297949d676bd92fb446a59f83a58111d5a6d41454c2382d2f64046ca7f2688`。两个分支提交使用原 HEAD 的锁文件。

两组工具链已分别核对：

| 范围 | Rust | Fortran | Python |
|---|---|---|---|
| 分支提交的原锁文件 | 1.98.1 | GCC 15.3.0 | 3.14.7 |
| 用户暂存更新的工作区 | 1.99.0 | GCC 16.2.0 | 3.14.7 |

所有项目命令通过 Nix devShell 执行，Cargo 使用 `--locked --offline`，没有安装/升级/联网获取 Rust 依赖。原锁文件还发现并修复了 workspace `default-features=false` 继承兼容性问题。新根 A 的默认 geospace 配置已独立 `cargo check`。

实施开始时保存原文件、指纹、原 index、用户暂存差异及全部八项资产；备份位置见 `.git/main-cache-implementation-location`。建立分支时再次核对原 index 的实际条目与用户锁文件指纹。旧历史恢复引用和远程 `origin/main` 均保留。

## 已观察到的证据

| 检查 | 结果与边界 |
|---|---|
| Rust 完整模型/数据/CLI 测试 | 190 项通过，2 项显式在线测试按原约定忽略；原锁文件和暂存更新工具链均执行 |
| Python Rust standard | 6 项通过；feature 合并下的 Python 基座另有 2 项通过 |
| 无快照基座 | core/indices/Python 共 60 项通过，2 项在线测试忽略；不声称没有快照仍能提供离线指数覆盖 |
| 格式、Clippy、rustdoc | 原锁文件的最终代码通过 fmt、全 workspace/targets/features Clippy `-D warnings` 和 rustdoc `-D warnings` |
| 单模型 Python 编译 | igrf/iri/hwm/msis 四组严格 Clippy 通过；实际 wheel 结果见本地发行物检查日志 |
| 干净 Cargo target | 没有来源的离线 IGRF 构建按预期明确失败；外部真实缓存下四个模型测试通过 |
| 构建来源边界 | 4 项真实 build-script 测试通过，含 fresh OUT_DIR、缺失、损坏、已验证来源/重用、indices None/Some；需先编译全部相应 build scripts |
| 同一 Cargo target 切换 | 实际 Cargo 构建按无缓存 → 显式缓存 → 无缓存执行，三次编译后的 optional_cache.rs 为 None/Some/None，相关测试通过；不存在旧嵌入残留 |
| 独立下载器与缓存准备 | 17 项合成 HTTP/日历边界测试通过；另有 2 项真实资产集成测试通过 |
| 真实同步链 | 本地 HTTP 提供 curl 单文件下载、16 项真实资产/通知、固定 commit、哈希、receipt 和重复复用；未联系 GitHub 远程 |
| Cargo 包 | 十个 package 文件清单含各自许可且排除八项快照；实际 core `.crate` 在仓库外解包，原许可字节核对及 10 项测试通过 |
| 六组最终 wheel | 基座及 igrf/iri/hwm/msis 各 2 项，standard 20 项 Python 测试通过；全部在独立临时 site 导入，Mach-O 动态依赖均无直接 Nix-store 路径 |
| sdist | 在实际含八项快照的缓存工作树打包，276 项内容检查通过且八项快照全部排除；独立解包后，Python 基座 cargo check --locked --offline 通过，不借用原仓库源码 |

macOS 链接器仍会报告 Fortran compact-unwind 警告；测试及严格 Clippy/rustdoc通过不表示这些链接器警告消失。静态 guard 的局部 Drop 清理测试通过，Rust 进程级 statics 在退出时不执行 Drop，不承诺进程退出后自动删除全部临时目录。

本地发行物、日志及许可证证据保存在根目录忽略的 `target/release-artifacts/`。许可证检查复制了 Cargo 缓存中 367 个依赖声明、619 份原文，28 个包没有单独打包的许可文本；这是 inventory 范围，包含任一具体 binary 之外的依赖，不是逐 binary 授权结论。

最终 wheel 使用分支提交的原锁文件工具链。在首次跨语言对照中，资产完全相同但 Rust/Python 的获取模式分别是 cargo-out-dir-cache/bundled-cache；两个全量元数据比较按预期报错。最终对照给两者提供同一套经过哈希核验的显式官方源码目录，再构建 Rust example 和 standard wheel，20 项通过，没有删掉来源字段或放宽全量比较。

sdist 的实际独立编译发现 maturin 基座依赖视图遗漏可选模型路径，而清单仍引用这些路径；通过显式 `crates/**/*` 纳入完整源码闭包并保留快照排除规则后修复。发行检查同时核对全部 workspace 成员和 path 依赖的清单及许可证，不只检查文件存在或缩减后的成员表。

## 使用与待本人核验

工作目录是仓库根。完整模型使用 `cache-snapshots` 的本地资产，或 main 下显式 `IONORAY_CACHE_ROOT=/absolute/cache-root`。完整命令、来源变量及产物检查入口见 [发行流程](releases.md)。维护方向始终是 main 合入 cache-snapshots，分支差异必须只包含八项资产；运行 `scripts/check-release.py --packages` 再核对。

建议本人先在缓存分支运行 `direct_models`，检查输出中的模型版本、SHA-256、单位和所有显式科学驱动。core 可进入 `position::serde_tests::nested_query_point_cannot_bypass_position_validation` 测试，在 `json` 被修改处断点观察弧度输入被拒绝；模型 `backend::asset_tests` 可检查目录、文件哈希与 guard。

CLI 自动测试和发行物导入检查已观察；VS Code 真正断点命中、本人的运行/解释/修改以及观测数据上的科学适用性尚未核验。没有从这些未知推断学习表现或科学接受。

## 公开发行仍阻塞

HWM 软件/派生适配器授权、MSIS 第 4(b)/4(c) 条的实际适用与履行证据、复制进完整 wheel 的 GNU/Apple 运行库及 Rust binary 依赖通知尚须审阅。macOS libcharset 原源码头部与 Nix 许可元数据存在 APSL/BSD 差异。HWM/MSIS crate 明确 `publish=false`；完整模型 wheel 和缓存分支保持本地研究材料。

Linux/Windows、Rust 1.89 MSRV、registry 依赖发布顺序、实际 GitHub 缓存来源以及远程默认 main 替换均未验证。本轮没有推送、上传发行物、公开缓存分支、联系 NRL 或声明这些授权已完成。工程结果可运行和检查，公开发行条件仍按 [发行阻塞](releases.md#public-release-blockers) 分项处理。
