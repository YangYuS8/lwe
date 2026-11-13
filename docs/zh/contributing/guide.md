# 贡献指南

## 开发环境

安装核心工具链：

- Node.js 24，与 CI 一致；
- pnpm 11.1.3，由 `package.json` 固定；
- Rust 1.99.0，由 `rust-toolchain.toml` 选择；最低支持的 Rust 版本另行声明在 `Cargo.toml` 中；
- 当前发行版所需的 Tauri 2 平台依赖。

Cargo Tauri CLI 版本由 `.tauri-cli-version` 固定（2.12.1）。使用共享安装脚本验证或安装该精确版本，不使用未限定的最新 CLI。

安装依赖：

```bash
pnpm install --frozen-lockfile
pnpm exec svelte-kit sync
./scripts/ensure-tauri-cli.sh
export PATH="$PWD/target/tauri-cli/bin:$PATH"
```

安装脚本默认将 CLI 单独安装到 `target/tauri-cli`，与已有 Cargo 工具分开。如需使用其他位置，在运行安装脚本时设置 `CARGO_INSTALL_ROOT`，并在执行 `pnpm tauri:dev` 或 `cargo tauri build` 前把该目录下的 `bin/` 加入 `PATH`。

运行前端检查：

```bash
pnpm check
pnpm test
pnpm build
pnpm docs:build
```

运行 Rust 检查：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace
cargo test --workspace
```

本地构建文档：

```bash
pnpm docs:build
```

本地预览文档：

```bash
pnpm docs:preview
```

## 修改产品行为前

把本站中维护的文档作为产品变更的政策来源。

每个重要变更都应说明它如何支持 Linux 动态壁纸平台，而不是已退役的 wayvid 产品故事。当范围可能被误解时，也应写明首发非目标。

设计工作应标明所属领域：

- 产品基础；
- 创意工坊循环；
- 兼容性；
- 运行时；
- 应用壳。

当内容类型或导入路径受到影响时，需要说明用户可见的兼容性影响。

## 用户可见文本和 i18n

LWE 的用户可见界面目标支持英文和简体中文。

修改用户可见文本时：

1. 同时更新两种语言；
2. 检查更长的中文或英文字符串对布局的影响；
3. 在任务或 Pull Request 说明中包含 i18n 验证；
4. 保持术语与文档一致。

文档站点也遵循该规则：英文根目录下的每个维护页面，都必须在 `docs/zh/` 下有对应的简体中文页面。

## 文档规则

- 所有维护中的文档都放在 `docs/` 下。
- 优先编写任务导向页面，而不是归档堆积。
- 过时规划文档中的有用信息合并进维护页面后，应删除原文档。
- 不要重新引入已删除的 `openspec/` 目录；项目指导现在由本文档站维护。
- 根 README 保持简洁，并链接到发布文档获取详情。
- 不要把不支持的运行时行为写成已支持。
- 如果某功能只在已验证环境中工作，需要明确写出该环境。

## 测试期望

提交变更前，运行最小且有意义的检查。对于范围较大的维护变更，使用：

```bash
pnpm check
pnpm test
pnpm build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace
cargo test --workspace
pnpm docs:build
```

Quality Check 还运行轻量打包回归：

```bash
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
```

前端测试包含服务端渲染和模拟 DOM 中的真实组件交互；`pnpm test` 会运行两个测试项目。单独检查时可使用 `pnpm test --project server` 或 `pnpm test --project interaction`。交互测试模拟 IPC，并点击实际挂载的组件；覆盖业务失败（`ActionOutcome.ok=false`）、IPC 拒绝，以及动作成功后刷新数据失败。失败后保留已有内容，区分重新加载数据与重复执行已经成功的动作。

Tauri 命令通过 `commands/background.rs` 在后台执行同步服务逻辑。依赖桌面状态的命令会等待后台启动恢复完成，写入操作在服务层串行执行。会话和设置使用私有临时文件后原子替换；持久化测试必须验证部分写入或替换失败时旧文件仍然有效。网络错误脱敏测试使用虚构 API key 和本地 HTTP 测试服务。

对于仅文档变更，`pnpm docs:build` 和 `git diff --check` 是必需验证项。

## 打包验证

两条发布 workflow 都读取 `.tauri-cli-version`，将其纳入 CLI 缓存键，并在恢复缓存后验证 `cargo-tauri`。缓存中的版本不匹配时，必须替换并重新核验，才能开始构建。

AppImage 在 Ubuntu 22.04 上构建，以保持 glibc 要求与 AppImage 目录的测试主机兼容。原生 deb/rpm 包使用 Ubuntu 24.04，以保留 Arch 软件包使用的 libmpv.so.2 ABI。发布 CLI 和 Rust 缓存按发行版隔离；静态元数据校验通过不能证明 AppImage 可以在最低支持系统上启动。Quality CI 使用校验过 SHA-256 的 actionlint 检查所有 workflow。

AppImage 验证需要 Python 3、`unsquashfs`（来自 squashfs-tools）、`file` 和 `desktop-file-validate`（来自 desktop-file-utils）：

```bash
python3 scripts/validate-appimage.py target/release/bundle/appimage/*.AppImage
# 也可验证已经解包的 AppDir：
python3 scripts/validate-appimage.py --appdir path/to/LWE.AppDir
```

校验器检查可执行的 `AppRun`、根目录唯一且有效的 desktop 文件、它引用的图标，以及可读取的 PNG `.DirIcon`。相关软链接必须使用相对路径、解析后位于 AppDir 内且目标存在；普通图标文件同样可用。检查不会启动应用或修补失败的包。所有选中上传的 AppImage 都必须在发布产物前通过该门禁。检查选中的版本，避免把旧构建产物纳入上传范围。

回归套件覆盖正常包，以及缺失、断链、绝对路径或越界的图标链接。测试样本通过不能替代真实发布产物验证或真实桌面验收。

## GitHub 到 CNB 的单向镜像

GitHub `main` 及其发布标签是权威源码与构建源。CNB `Nesoriel/lwe` 仅作为代码和发布的单向镜像。CNB 仓库设置为 `auto_trigger=false`；`.cnb.yml` 只保留用于 EdgeOne 文档部署的 `api_trigger_docs`。CNB 不重复运行 Quality Check、Rust 或安装包构建。

`Sync CNB` workflow 在 GitHub `main` 推送时镜像代码。两条发布 workflow 在 GitHub release 发布后，以 reusable job 传入 `release_tag` 和 `expected_source_sha`，不依赖 `GITHUB_TOKEN` 产生的 release 事件。`workflow_dispatch` 可回填单个 `release_tag`，或通过 `deploy_docs=true` 显式请求文档 API 触发器。

CNB 写入共用不取消任务的 GitHub 队列（`queue: max`）。CI 严格检查该队列声明，只精确排除固定 actionlint 版本不支持队列字段的误报；其他 workflow 保持完整检查。上传后核对 CNB 服务端 SHA-256 和大小；旧附件没有 SHA-256 时下载文件校验。

GitHub 仓库 Secret `CNB_TOKEN` 使用限定到 `Nesoriel/lwe` 的长期 CNB 个人访问令牌（PAT），所需权限为：

- `repo-code:rw`；
- `repo-release:rw`；
- `repo-cnb-trigger:rw`；
- `repo-manage:r`。

本机 OAuth 令牌有效期为八小时，不适合长期 CI。管理员在本机完成一次性 CNB 仓库设置；CI 不授予仓库管理写权限。令牌值不得进入源码、日志或发布说明。

保留 GitHub tag 对象，并核验其源码提交与 `expected_source_sha` 一致；不得改写 tag 或强制更新 CNB `main`，后者只允许快进。公开 GitHub release 附件原样同步，核对两侧 `.deb`、`.rpm` 和 `.AppImage` 的 SHA-256，CNB 附件以 `ttl=0` 永久保留。重跑时补齐缺失内容、跳过相同的已有附件，拒绝同名但不同内容的附件。任何同步或校验失败都必须保持为失败的运行结果。

源码变更不能证明镜像或 EdgeOne 部署已上线。合并并执行 workflow 后，应读回 CNB 提交、tag 对象、发布附件及哈希；文档触发和部署结果单独报告。

## 发布冒烟清单

发布稳定标签或合并到 `main` 触发预发布前，先验证该操作将触发的发布路径：

1. 如果变更范围较大，在本地运行等价的必需检查：`pnpm check`、`pnpm test`、`pnpm build`、`pnpm docs:build`、`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo check --workspace`、`cargo test --workspace`，以及上述打包回归。
2. 确认待打标签提交上的 GitHub Actions 质量检查通过。
3. 确认发布 workflow 会构建预期 Linux 制品（`.deb`、`.rpm` 和 `.AppImage`），并在上传前通过 AppImage 验证。发布后下载公开的 AppImage，记录 SHA-256 并再次验证。
4. 确认包渠道元数据匹配发布渠道：稳定标签使用 AUR stable（`lwe`），开发/预发布路径使用 AUR git（`lwe-git`）。
5. 对影响运行时的发布候选版，在写成受支持前，必须在已验证的 Wayland + `niri` 会话上运行下面的真实桌面运行时验收清单。
6. 条件允许时执行新安装冒烟流程：安装一个包制品，打开并移动/缩放窗口，从托盘恢复窗口，打开 Settings 并生成诊断信息，检查 Workshop/Library 状态，应用一张兼容视频壁纸，清除它，重启 LWE，并确认保存分配行为可见。改变 Wayland 启动行为的 Tauri 升级也需要在 `niri` 上完成此项检查。
7. 读回发布及包渠道结果。修复 AppImage 目录收录时，应在新稳定版 AppImage 已公开且验证通过后请求上游复测，并确认上游日志选中了新版本。

包安装成功只表示应用可从该包启动；它不保证未验证的合成器、GPU/EGL 栈、显示器布局或壁纸类型具备运行时支持。

## 真实桌面运行时验收

运行时变更在写成“已支持”前，必须先在真实受支持桌面会话上验证。Wayland + `niri` 上的视频是主要已验证目标；实验性 Scene 验收仅限下方记录的样本与环境。

会话恢复使用 LWE 会话文件：当设置了 `XDG_CONFIG_HOME` 时为 `$XDG_CONFIG_HOME/lwe/session.toml`，否则为 `$HOME/.config/lwe/session.toml`。验收时使用独立的 `XDG_CONFIG_HOME`，检查恢复行为时读取该测试会话文件。不要把删除现有用户设置或分配作为常规测试准备。

运行时变更请使用以下清单：

1. 在 LWE 中发现至少一个活动显示器；
2. 从本地库把一张兼容的视频壁纸应用到一个显示器；
3. 确认桌面可见变化；
4. 如果存在多个显示器，把壁纸应用到第二个显示器，然后只清除其中一个显示器；
5. 确认清除一个显示器不会停止其他显示器上的壁纸；
6. 重启 LWE，确认已保存分配能恢复，或恢复失败能在 Desktop 页面中显示；
7. 清除所有分配，并确认保存的会话不会再次恢复它们；
8. 在同一进程中至少三轮应用、清除并重新应用视频，检查 EGL 资源复用。

如果某个运行时步骤失败，请保留终端日志中指出失败阶段的行：后端启动、输出发现、首帧应用、按显示器清除或启动时恢复。这些信息是区分缺失视频资源、输出不匹配、Wayland layer-shell/EGL 失败和后端超时的受支持方式。

Rust 测试套件中的真实桌面测试需要显式启用，因为它们依赖当前合成器、显示器布局、GPU/EGL 栈、Steam 创意工坊内容和本地视频资源。请只在已验证机器上运行：

```bash
lwe_test_config="$(mktemp -d /tmp/lwe-real-desktop.XXXXXX)" && \
XDG_CONFIG_HOME="$lwe_test_config" LWE_REAL_DESKTOP_TESTS=1 \
  cargo test --locked -p lwe-shell --lib \
  services::desktop_service::tests::desktop_apply_flow_reapplies_video_after_clear_on_same_backend \
  -- --exact --nocapture --test-threads=1
```

验收实验性的原生 2D Scene 子集时，请先确保本地库中至少有一张通过本地验证的 Scene 和一张可运行视频，再使用独立测试配置目录：

```bash
lwe_scene_test_config="$(mktemp -d /tmp/lwe-scene-desktop.XXXXXX)" && \
XDG_CONFIG_HOME="$lwe_scene_test_config" LWE_REAL_DESKTOP_TESTS=1 \
  cargo test --locked -p lwe-shell --lib \
  services::desktop_service::tests::desktop_apply_flow_scene_video_switch_clear_and_restore \
  -- --exact --nocapture --test-threads=1
```

该测试默认检查三轮应用/清除，以及同一后端中的 Scene → Video → Scene 切换、隔离会话保存失败后的回滚、commit 命令已处理后的取消、后端关闭/join/重启后的 Scene 分配恢复和最后清除。`LWE_REAL_DESKTOP_SCENE_CYCLES` 可指定 1–1,000 轮；重跑已记录的混合输入源压力测试可使用 100 轮，并复用同一个隔离配置目录：

```bash
XDG_CONFIG_HOME="$lwe_scene_test_config" LWE_REAL_DESKTOP_TESTS=1 \
LWE_REAL_DESKTOP_SCENE_CYCLES=100 \
  cargo test --locked -p lwe-shell --lib \
  services::desktop_service::tests::desktop_apply_flow_scene_video_switch_clear_and_restore \
  -- --exact --nocapture --test-threads=1
```

保留 `HOME` 供 Steam 内容发现使用，检查持久化时读取隔离目录中的会话文件。还需确认桌面上壁纸可见，且受支持的效果会动画。增加轮数是操作步骤，不代表已记录通过。此测试只覆盖当前 2D 子集，不能证明完整 Scene 兼容性。独立输出渲染与清除需要真实多显示器硬件，单显示器测试无法覆盖这些场景。

### Scene inventory

只读 inventory 示例接收单个项目目录，或包含最多 64 个打包项目的目录。请替换为实际 Steam 库路径，并把报告保留在本地；不需要桌面 opt-in 标志：

```bash
lwe_scene_projects="$HOME/.local/share/Steam/steamapps/workshop/content/431960"
lwe_scene_assets="$HOME/.local/share/Steam/steamapps/common/wallpaper_engine/assets"
lwe_scene_inventory_report="$(mktemp /tmp/lwe-scene-inventory.XXXXXX.json)"
cargo run --locked -p lwe-scene --example inventory -- \
  "$lwe_scene_projects" "$lwe_scene_assets" > "$lwe_scene_inventory_report"
```

`SceneInventory::collect` 沿已建模引用遍历，并扫描包内 JSON/TEX/着色器和项目元数据。未打包文件只按引用读取，不递归枚举目录。报告区分可达与仅存在于包中的声明、`user`/`value` 绑定与 JS、HDR 与 bloom、资源来源、结构和未知项。脚本/着色器/文字正文及私有值不导出，未知/文字子树中看似资源的字符串也省略。它不把内容标为可运行或已支持。预算耗尽及无效/缺失资源会明确标为部分覆盖；动态/脚本生成的引用仍未知。

默认每次遍历包含 8,192 个资源、32 MiB JSON、16 MiB 着色器来源、16,384 个 mip 与 16,384 个动画帧的上限，同时限制文档、深度/节点、观察及报告字符串。结构读取器在循环与分配前消耗共享的 mip/帧剩余预算，跳过媒体负载且不执行解码。PKG 索引、范围读取和哈希复用同一打开的 descriptor；元数据检查能发现普通原地修改，路径替换也不会把指纹转向新文件。这是保守的一致性检查，不是文件系统不可变快照。完成有界遍历不等于完整运行时语义已验证。

### Scene UI 与运行时测量

以下工具的运行时模式会应用和清除真实壁纸，只能在具备条件的 Wayland + `niri` 会话中，为真实桌面验收显式启用。保留 `HOME` 供 Steam 发现使用，并填入本地已获取的项目/资源路径。工具不能替代原版 Wallpaper Engine 视觉 A/B、多显示器或长期验收。

`scene_probe` 启动自有引擎后端，设置 30 FPS 引擎上限，在频繁 `GetStatus` 请求下测量动态、暂停与恢复阶段。JSON 输出包含 draw/swap/callback 计数、时长与逻辑 Scene 负载；不保存 settings/session，也不测试 Tauri UI。请把 `eDP-1` 替换为实际输出：

```bash
lwe_scene_project="$HOME/.local/share/Steam/steamapps/workshop/content/431960/3337606455"
lwe_scene_assets="$HOME/.local/share/Steam/steamapps/common/wallpaper_engine/assets"
lwe_scene_probe_report="$(mktemp /tmp/lwe-scene-probe.XXXXXX.json)"
LWE_REAL_DESKTOP_TESTS=1 cargo run --locked -p lwe-engine --example scene_probe -- \
  "$lwe_scene_project" "$lwe_scene_assets" eDP-1 60 \
  > "$lwe_scene_probe_report"
```

追加 `--static /absolute/path/to/an-authored-static-project` 可检查暂停时的静态第一帧和独立静态空闲阶段。静态项目须通过准备验证且无动画特效；未提供时，报告没有静态阶段证据。工具检查命令流量下的限帧、稳定暂停/静态时零活动、清除及 shutdown/join；它主动产生状态请求，不能把该过程当作普通空闲 CPU/功耗基线。报告的逻辑 RGBA 纹理/FBO 字节不含真实驱动 VRAM、EGL 缓冲或视频资源。

添加 `--properties /absolute/path/to/choices.json` 可使用作者属性覆盖值准备并测量项目。JSON 必须是声明名称到对应类型值的对象，文件上限为 64 KiB。准备过程共用 LWE 的验证；Probe 不修改原始项目，也不把这些选择保存到 settings/session 文件。

`scene_resources` 分开诊断 CPU 准备、Scene 渲染、视频渲染、混合输入源切换和后端反复重启。包括 `cpu` 模式在内都要求 `LWE_REAL_DESKTOP_TESTS=1`；运行时模式要求恰好一个物理输出。每次清除先确认活动会话/指标为空的状态 ACK，再采样；后端关闭后等待 join。工具逐条输出 JSON 行，不在进程中累计报告，不写 settings/session：

```bash
lwe_scene_project="$HOME/.local/share/Steam/steamapps/workshop/content/431960/3337606455"
lwe_scene_assets="$HOME/.local/share/Steam/steamapps/common/wallpaper_engine/assets"
lwe_scene_video="/absolute/path/to/local/video.mp4"
lwe_scene_resource_report="$(mktemp /tmp/lwe-scene-resources.XXXXXX.jsonl)"
LWE_REAL_DESKTOP_TESTS=1 cargo run --locked -p lwe-engine --example scene_resources -- \
  "$lwe_scene_project" "$lwe_scene_assets" "$lwe_scene_video" mixed 30 \
  > "$lwe_scene_resource_report"
```

把 `mixed` 替换为 `cpu`、`scene`、`video` 或 `restart`，循环次数为 1–200。`cpu` 只准备/释放 Scene，不启动桌面后端；`restart` 每轮应用/清除 Scene，并关闭/join 后端。其余模式在多轮之间保留同一后端。

JSON 记录 Rust 请求的活跃/峰值字节、glibc 已用/空闲/mmap 块、`/proc/self/smaps` RSS 分类、可用的 DRM fdinfo 内存字段、FD 和线程数。Rust 计数不含直接原生分配，其逻辑 realloc 峰值也不包含原生分配器临时保留的新旧缓冲。glibc 空闲块是分配器保留空间，不是活跃壁纸资源；mmap 与驱动分配需要分别解释。采样自身会临时分配 `/proc`/JSON 缓冲，并有固定预热开销。这些进程快照不是总 VRAM、帧耗时、功耗或全部原生分配无泄漏的证明。在全新子进程中设置 `MALLOC_ARENA_MAX=2` 只用于受控分配器诊断，不是生产设置或已验收的内存修复。

`scripts/scene-ui-smoke.py` 使用原生 Tauri WebDriver DOM 操作与真实 IPC，以原生鼠标点击选择 Library 项目与显示器、应用、读取 Desktop 反馈与隔离会话、检查标题栏关闭隐藏但壁纸继续、重启自己的应用进程、确认恢复、清除，并可检查阻断项目。它输出 UI/桌面截图及 `report.json`。当前工具要求目标 compositor 恰好一个输出、已有 shell 构建、`PATH` 中可用的 `tauri-driver` 和 `WebKitWebDriver`、`grim`、当前输出上未命名的空工作区，以及已获取的 Scene 内容。嵌套 compositor 的输出须标为嵌套范围，不能记为物理显示器验收。UI 显示器标签可与后端输出名不同，报告会同时记录。测试 debug shell 时，在另一终端运行 `pnpm dev`，并先执行 `cargo build --locked -p lwe-shell`：

```bash
lwe_scene_project="$HOME/.local/share/Steam/steamapps/workshop/content/431960/3337606455"
lwe_scene_blocked_project="$HOME/.local/share/Steam/steamapps/workshop/content/431960/2454418355"
lwe_scene_driver="$(command -v tauri-driver)"
lwe_scene_ui_report="$(mktemp -d /tmp/lwe-scene-ui-report.XXXXXX)"
LWE_REAL_DESKTOP_TESTS=1 python3 scripts/scene-ui-smoke.py \
  --binary "$PWD/target/debug/lwe-shell" --driver "$lwe_scene_driver" \
  --project "$lwe_scene_project" --blocked-project "$lwe_scene_blocked_project" \
  --blocked-reason unsupported_hdr \
  --output "$lwe_scene_ui_report"
```

脚本创建并删除自己的隔离 `XDG_CONFIG_HOME`，保留 `HOME`，临时命名验收工作区，结束时恢复此前获得焦点的工作区。壁纸截图前会检查该工作区获得焦点且没有遮挡。`--output` 是报告目录，不是显示器名。除业务状态断言外还需检查截图，显示 Running 本身不足以证明画面。缺少驱动、合适的空工作区或桌面条件时，应记为验收阻断。工具不能证明硬件电池切换、实测 VRAM、功耗、24 小时运行或独立多输出行为。

追加 `--playback-settings` 可执行原生 Settings 操作：保存 15 FPS 和电池暂停，在壁纸运行时改为 30 FPS，随后在自有进程重启后核验保存偏好。示例显式检查 `2454418355` 的 `unsupported_hdr`，复跑前应重新核对阻断项目/原因。`3659880761` 现在通过原始包准备，不再是当前泛光阻断示例。UI 保存值不测量实际节奏或硬件电池转换，下方独立桌面服务测量覆盖引擎节奏。

原生属性/失败/全屏验收应使用默认 `3659880761` 项目，追加 `--scene-properties --scene-properties-write-failure --long-occlusion`，可同时加 `--playback-settings`。`--scene-properties` 需要作者声明的 bool 和可修改 slider，保存编辑后验证重启的会话/控件恢复，并重置全部作者默认值。写入失败标志要求非 root 用户，仅更改工具自有隔离配置的权限，验证回滚/草稿保留后重试。`--long-occlusion` 只用自有 Tauri 全屏窗口覆盖至少十二秒，不使系统休眠/锁屏，也不测 callback 暂停。应复核真实壁纸、编辑器、回滚与恢复截图，自动业务断言本身不足以通过画面验收。

控件较多的项目可添加 `--all-scene-properties`，它会启用属性流程并编辑全部声明控件。bool 和 slider 使用原生 WebDriver 鼠标/键盘操作；color 通过已渲染颜色输入的实际 input 事件修改，再走正常 Save/IPC 流程，不测试系统颜色选择器。原始 `2903458386` 的二十个控件使用此参数，并同时启用 `--scene-properties-write-failure --playback-settings --long-occlusion`。

恰有一个图片资源控件的项目可在 `--all-scene-properties` 后添加 `--scene-image /absolute/path/to/static.png`，并将 `wtype` 放入测试 PATH。工具通过自有原生文件选择器取消/选择，仅删除复制出的选图来源，再验证内容 token 的持久恢复和作者图片重置。配置和数据目录均隔离，保留 `HOME` 供 Steam 发现。验收完整效果图时，为需要保持启用的作者选项重复添加 `--keep-scene-property NAME`，其余控件照常编辑。将全部 bool 取反可能关闭所有效果，不能作为完整渲染验收。这些参数不注入 IPC，不绕过候选/保存门控。

原生选择器按键不能触发 GTK 按钮时，可添加 `--native-dialog-accessibility`。它依赖 Python GI/AT-SPI 与 GTK 无障碍桥接。工具自动且仅在自有子进程环境中设置 `GTK_MODULES=gail:atk-bridge`、`NO_AT_BRIDGE=0` 和 `LC_ALL=C.UTF-8`。`wtype` 仍输入复制出的来源路径；替代模式核对自有选择器窗口、PID 和焦点后，调用真实 Cancel/Open 按钮的 AT-SPI action，不调用业务 IPC，也不替换原生文件选择器。

在 `--scene-image --playback-settings` 后添加 `--narrow-image-layout`，可在真实 380 像素原生视口中记录英/中文图片控件。工具通过 Settings 切换语言，检查图片控件及按钮无溢出，再恢复原生窗口尺寸。该检查覆盖图片编辑器布局，不代表整个 Library 的窄窗口布局验收。

### Miku 扩展（2026-10-13） {#miku-extension-2026-10-13}

共享有界路径现已实现空默认图片属性、实例材质、genericimage4/sprite 帧选择、原 motionblur 累积/复制/合并图与 CRT 合成别名、所需 SceneScript 帧/音频/属性形式及内置 Atami CFF 文字。作者隐藏层仍参与准备，实际选项决定可见性。默认图保留十一个声明、十个渲染层、五个效果及二十五个 pass，含四个全局 bloom pass。

未修改的原版 `3420277613` 已在最终生产源码上通过完整 CPU 准备与软件 EGL 全图 oracle。两组八帧图集均在实际原脚本 VM 的受控非零音频下完成选择和可见绘制；三个独立历史、准确暂停画面、同项目候选、冻结旧画面的精确回滚、解冻与最终清理通过。保留的 CPU 计费为 255,616,293 字节，预留峰值为 525,312,699 字节（500.98 MiB）；已验收旧代/候选 GPU 并集为 525,624,984 字节（501.28 MiB）。两项峰值均低于各自 536,870,912 字节上限。冻结时将 79,833,600 字节私有历史移入封闭 CPU 快照；若旧历史留在 GPU，会需要 605,458,584 字节，按预算被拒绝。清理后 CPU 计费与共享上传均归零。这些生产分配/预留计数不包含测试专用 readback vector、分配器开销、RSS 或实测 VRAM。

最终源码的独立 CPU 检查还通过了全部五个原始脚本模块与原版 Atami 字体，包括共享字体所有权及有界文字栅格化。2.5D Space 软件 EGL 回归的二十六项像素比较也全部通过。这些检查使用受控输入频谱，不能证明实时音频采集或桌面验收。

独立 CPU 准备/worker 检查通过了原 Glitch OFF（`0`）和 POOF（`2`）分支，开启 Framerate 并保留默认效果。OFF 准备出十一个图层/五个效果/二十六个 pass，显示 Static50；POOF 为十/五/二十五，按作者条件有意隐藏角色。脚本控制的隐藏图层在 callback 后仍保持隐藏。这是 CPU 选项证据，不是这两个分支的原生像素验收。包中的 scheme-color 控件没有显式对象或脚本绑定，其 UI/持久化检查不能证明原引擎隐式配色行为。

独立 `original-miku-private-capture.json` 在 AMD Radeon/radeonsi surfaceless EGL 上通过。自有 PCM 播放至私有静音 Pulse sink，再经生产默认采集进入已验收的十六/三十二/六十四频段频谱；十三份已验收帧快照匹配原十六/六十四频段 VM 不透明度响应，同时渲染完整默认图与三个历史。单独的因果 probe 固定输入、几何与时间，真实音频通过未修改的原 SimpleBars shader 及三十二频段立体声 uniform 改变了 1,468 个像素。暂停断开采集且精确保留已验收像素，恢复后重连，清理后采集与共享上传释放。该证据是真实私有采集加硬件离屏渲染，与原生桌面呈现分开记录。

最终原生 engine-only probe 在 AMD 680M 的嵌套 niri 上以 1920×1080 渲染未修改的默认图。30 FPS 上限下，三十秒动画完成 136 次 swap（4.53 FPS）；三十秒暂停的 draw/swap/callback 均为零，恢复后三秒完成十三次 swap（4.33 FPS）。清除后输出指标移除，后端退出成功 join。所有效果与作者资源分辨率均保留。**该次运行未达到 30 FPS，不代表日用就绪。** 当前 GPU 计划 500,230,580 字节是逻辑并集，重叠的 pool/私有诊断视图不能相加。私有最终 manifest 记录 shell SHA-256 `09857b830cc0d2147c0ffb7ec32aaff0cbac6928d8a094a461d4a40b3de5355d` 及 probe SHA-256 `42ee6e3d9e04f5f20357107e52ad7e2ff72a42c4ac0a4d84dabf1460701fe29a`。

真实原生布局检查通过了英/中文 380 像素视口，图片控件宽 230 像素，控件内部及按钮均无溢出。语言通过真实 Settings 流程切换，运行期间源码不变，未注入 DOM 样式，自有窗口/工作区清理通过。现有 catalog toolbar 溢出不在此图片编辑器检查范围内。

该机器 Mesa 26.2.4/llvmpipe LLVM 23.1.1 的独立微型标量数组/tint 夹具出现像素错误，但 uniform 回读正确。纯 GL 独立复现确认软件驱动问题；四十二个相同 AMD Radeon/radeonsi 对照及 fresh-program A→B→A 检查通过。保留软件像素比较失败的事实，不降低期望值，也不由源码/数学检查推定原引擎一致性。

最终 `native-full-effects-final3/report.json` 在同一嵌套环境中通过全部十七项原生步骤，无模拟 IPC 或清理错误。流程涵盖全部十四个值控件，保持 Glitch ON 与完整效果 bool 启用，修改 Framerate、scheme color、全部六个 CRT slider 和背景图片。两轮真实 GTK 文件选择器取消/选择使用自有无障碍按钮；导入后删除复制出的选图来源，内容 token 经原子保存与重启保留。PermissionDenied 保存失败保留原会话字节/inode、可编辑草稿和此前 Scene Running 状态，重试成功。Settings 将 FPS 上限从 15 改为 30，应用进程与分配保持不变；标题栏关闭隐藏同一进程，整个自有进程组退出 join 后，新进程恢复图片与选项。重置删除选项覆盖并恢复全部作者默认值，十二秒自有全屏遮挡后恢复，随后目标清除移除分配。该次流程还通过两种语言的 380 像素图片布局，并恢复原命名工作区。全屏遮挡不证明 callback 暂停或系统休眠。

图片契约与边界见 [Scene 图片、动画帧与历史缓冲](./scene-miku-runtime.md)。按用户明确要求跳过物理休眠/恢复；真实多显示器、安装包、Windows A/B 及 Miku 长期运行验收仍未完成。

### Miku 性能调查（2026-10-14） {#miku-performance-2026-10-14}

本次迭代以已合并的 `00a8ceed5b876139ce649ebf2d321ec218a0d296` 为起点。上文 2026-10-13 的源码/二进制身份与 4.53 FPS 验收保留为独立历史记录。新基线和候选测量分别记录构建 profile、源码/二进制哈希、输出及采样条件，不能将 debug 与 release 当作相同构建直接比较。

对已安装 Wallpaper Engine 编辑器执行了惰性 REA JavaScript 分析，并精确读取随附声明、shader 及 effect/material 资源，没有运行 Windows 程序。私有 `editor-rea/editor-rea-findings.md` 和来源 manifest 保留观察边界与随附文件的准确身份。动画声明描述来源时长、帧控制、通过 material join 共享计时器，以及非动画重画时 frametime 为零；generic image 1–4 通过 UV 平移与两个基向量定位 sprite 区域。这些是编辑器/声明结构观察，原生默认相位、隐藏或呈现失败时的时钟推进、实例分离及 GPU 上传调度仍未确认。声明不能扩大 LWE 接纳的 host API，preset 的 `nointerpolation` 也不能证明时间插值或精确 sampler 规则。

随附 motionblur 图按 buffer2 累积、buffer2 复制到 unique buffer1、读取 buffer2 合成的顺序执行。rate 是不含帧时间输入的固定 shader 因子，因此 buffer2 在复制与合成期间都必须存活，持久历史须按效果/代次隔离。静态证据不能证明原生分配/执行频率、精确保留名称解析，或允许将 copy 改为 buffer swap；仅有零 dt 重画也不能证明原效果被跳过。编辑器取消选图保留原值，移除恢复作者默认/空选择；LWE 的有界私有内容 token 导入仍是独立适配，不提供任意路径 IPC。资源 wrapper 可见 value/user/script/scriptproperties/animation 字段，音频声明列出十六/三十二/六十四频段注册；这些编辑接口既不接纳未支持绑定，也不能证明 Windows FFT 归一化。有限效果 inventory 也不等于接纳额外 FBO 格式或高级效果结构。

LWE 已在来源、帧及采样身份不变时复用经过验证的动画帧区域/view，这是本次沿用的行为，不是新增优化，也不会跳过新脚本 packet 或正常效果/历史处理。调度候选将 FPS 截止时间锚定到最后一次成功 swap，并为未完成的 Scene 准备设置独立 8 ms 重试间隔，Video 保留原 decoder 重试策略。另一项候选以新的解码契约 tag、封闭来源的实际编码 SHA-256、准备阶段 namespace/来源 fingerprint、帧索引及准确 ROI/gutter 矩形推导区域 key，避免再次扫描解码 RGBA。原文件校验、BC3 解码、不可变所有权与 512 MiB CPU/GPU 门控保留，过期的准备身份不能替代封闭来源的实际摘要。两项候选均不改变原资产、效果或资源分辨率，重试间隔不是帧率承诺。

探索性测量与最终受控对比分开。初始 debug 基线三十秒完成 149 次 swap（约 4.97 FPS）、741 次尝试，成功 render 加 swap 平均 5.86 ms，采样 worker 活动约占单 CPU 核心的 34.58%。探索性 release 基线完成 176 次 swap（5.87 FPS）、704 次尝试，成功 render 加 swap 平均 5.44 ms；仅调度修改的 release 候选完成 238 次 swap（7.93 FPS）、1,932 次尝试及平均 5.56 ms，完成 swap 数约增加 35.23%。三十秒暂停的 draw/swap/callback 均为零，恢复/清除/join 通过。增加的重试次数须保留，这些运行不是最终组合候选对比。

区域 key 修改通过六项 CPU 隔离回归，涵盖过期准备身份下编码像素改变、仅元数据改变、namespace/帧/ROI 隔离、不同真实 owner 及重复 worker epoch。温热的纯 CPU benchmark 执行 32 个原始区域/case，逐 ROI release 平均解码 11.12 ms、旧 RGBA key 哈希 10.53 ms、新 key 推导 119.75 ns；debug 分别为 26.02 ms、10.57 ms 和 172.96 ns。其 namespace 身份为替代测试值，最终 CPU 计费归零，没有 GPU 上传或桌面呈现，不能预测 FPS 或证明进程总内存。

组合候选通过受控 release A–B–B–A 对比。四次运行均采用相同 1920×1080 嵌套 niri/AMD 680M 条件、原始默认配置及私有 PCM，父 compositor 窗口完全可见并固定在 `[96, 112]`。

| 运行 | 构建 | 完成 swap / 实测动画秒数 | FPS |
| --- | --- | --- | --- |
| A1 | 基线 release | 174 / 30.000054910 | 5.80 |
| B1 | 组合候选 release | 291 / 30.000036961 | 9.70 |
| B2 | 组合候选 release | 296 / 30.000055844 | 9.87 |
| A2 | 基线 release | 179 / 30.000075875 | 5.97 |

按完成 swap 与实测时间合计为 5.8833 → 9.7833 FPS，每秒完成 swap 约增加 66.29%。四次三十秒暂停的尝试/draw/swap/callback 均为零，清除、后端退出/join 和身份检查通过。当前逻辑 GPU 计划保持 500,230,580 字节，原效果、历史、资源分辨率及 512 MiB 门控均保留。这是有限、相同 profile 的调度/key 结果，没有用隔离微基准或 debug 测量预测 FPS。

组合 debug 候选也在三十秒内完成 242 次 swap（8.07 FPS）、1,982 次尝试，成功 render 加 swap 平均 5.37 ms，暂停无活动，恢复/清除/join 通过。探索性 debug 基线约为 4.97 FPS，但父窗口位置不同，因此仅作为辅助证据，不是主要受控对比。

最终源码的 `final-miku-gpu-oracle.json` 在软件 EGL/llvmpipe、受控输入频谱下通过未修改的完整默认图：十个图层、五个效果、二十五个 pass。两个原八帧来源均选择并可见绘制 0–7 帧，三个历史保持独立，暂停/resize/候选/回滚/解冻检查保留要求的像素与状态。CPU 预留峰值保持 525,312,699 字节，旧代/候选 GPU 并集为 525,624,984 字节，均低于各自 512 MiB 上限；清理后 CPU/pool 计费归零，原始来源哈希不变。六项自主历史测试与两项共享测试另在 AMD 上通过。普通 workspace 测试通过 549 项、三十项 opt-in 仍忽略；这九项显式执行的 GPU 检查是独立证据，不代表全部被忽略的桌面/GPU 测试均已运行。

帧率仍低于 30 FPS 上限，不代表日用就绪或长期运行。本次没有复测上轮原生 UI 保存/恢复或安装包流程，有限嵌套引擎运行也不能证明物理输出验收或 Windows 一致性。按用户明确要求继续跳过物理休眠/恢复。

### Miku worker 通知与 codec（2026-10-14） {#miku-worker-wake-2026-10-14}

本次独立迭代以已合并的 `9278023` 为基线，其中已包含此前调度与封闭区域 key 修改。前文 2026-10-13 及 2026-10-14 的源码身份、量测和证据范围保持不变。本轮 A 为该基线，C 仅增加 CPU codec 行复制修改，D 在 C 上增加 renderer 私有 worker 通知。整体吞吐另由最终 release A–D–D–A 对比评估，短阶段百分比不能相加。

BC3 修改将已验证行几何计算移到外层，迭代固定十六字节 block，直接复制完整内部行，ROI/mip 边缘仍裁切。checked 私有布局、来源/输出长度检查、endian/舍入公式、输出全写入、不可变所有权及固定 84 字节 codec scratch 预留保留。二十个非零 sentinel ROI 补齐起始相位、连续内部行、单像素角及准确的部分右/下边缘。最终源码原始资源 DDS oracle 通过全部十六帧和十六个邻域。独立纯 CPU A–C–C–A 对比逐解码平均 11,172.47 → 8,659.33 µs（−22.49%），每次运行 32 区域，生产峰值保持 89,121,153 字节，最终计费归零。它测量温热 CPU 解码，不是 GPU 时间或 FPS 预测。

Scene script/texture worker 现先发布结果或错误，再发送属于该 renderer 的合并 event-loop 提示；其他代的晚到通知不能标记当前选中 renderer 就绪。处理提示仅移除尚未完成的 Scene 准备重试延迟，保留最后成功 swap 的 FPS 截止时间原点及 Video decoder 策略，暂停/idle 输出不会因提示强制绘制。八毫秒轮询仍是后备，完成通知不确认首帧，也不绕过完整 packet/draw/swap 验收。原效果、历史、资源分辨率与 512 MiB CPU/GPU 门控保留。

最终仅 codec 短阶段观察到 9.5657 → 10.0000 FPS（约 +4.54%），采样 texture worker 单核占用从 21.5655% 到 18.0719%。基线范围为 9.20–9.93 FPS，因此不能把小 FPS 差异称为稳定吞吐收益。独立通知阶段观察 C 为 8.9333 FPS（7.93–9.93），D 为 12.2000 FPS（12.13–12.27）；C 的波动使约 +36.57% 不能当作稳定通知独立效应。CPU 采样采用一秒 phase-start 标签，可能包含稳定/恢复交界，并非精确逐阶段 profile。短阶段暂停/清除/join 和身份检查均通过。

通知阶段记录 682 次选中 renderer dispatch，首次合并结果发布到 event-loop dispatch 平均 24.34 µs；不含解码、packet 消费、render、swap 或 GPU 工作。旧探针没有该指标时应记为未测，而非零。

最终相同 profile 的 release primary 对比包含四次三十秒动画运行，均采用相同 1920×1080 嵌套 niri/AMD 680M 条件、原始默认配置及私有 PCM，并核验几何/输入/二进制身份：

| 运行 | 版本 | swap / 尝试 | FPS |
| --- | --- | --- | --- |
| A1 | 已合并基线 | 297 / 1,480 | 9.90 |
| D1 | Codec 加通知 | 361 / 1,627 | 12.03 |
| D2 | Codec 加通知 | 367 / 1,651 | 12.23 |
| A2 | 已合并基线 | 301 / 1,494 | 10.03 |

合计 swap/时间为 A 598/60.000133134 秒、D 728/60.000109441 秒：9.9666 → 12.1333 FPS（约 +21.74%），尝试次数从每秒 49.57 增至 54.63。采样 texture worker 单核占用从 21.1669% 到 16.8344%（约 −20.47%），engine 为 6.2985% → 6.4277%，全部 probe 线程为 29.7367% → 25.6624%。CPU 采样窗口约 62.08 秒，包含阶段交界。成功 render 加 swap 的 CPU wall 均值为 5.2991 → 4.4352 ms，p95 直方图桶均为 6 ms，这不是 GPU timer 测量。D 记录 1,350 次选中 renderer dispatch，发布到 loop 延迟平均 25.38 µs，A 没有对应测量。四次暂停/清除/恢复/join、来源/二进制身份和跨轮一致性门禁均通过。

最终源码还通过完整默认软件 EGL oracle：两组可见八帧、三个私有历史、准确暂停及候选回滚。CPU 峰值 525,312,699 字节及旧代/候选 GPU 并集 525,624,984 字节仍低于 536,870,912 字节，最终计费归零，输入哈希不变。六项 AMD 历史及两项共享测试独立通过，最终原始来源 CPU DDS oracle 也通过。普通 workspace 测试通过 558 项、三十项 opt-in 仍忽略；本轮实际执行的一项原始 CPU 加九项 GPU opt-in 是独立证据。

冻结候选还通过嵌套原生 engine-only 替换检查：原 Miku 动画、三秒暂停的尝试/draw/swap/empty frame/callback/worker dispatch 均为零，随后恢复。暂停时换成私有 16×16 静态 Scene 恰好产生一次 draw/swap，三秒静态播放保持 idle，清除/退出 join 后无自有子进程残留。二进制和原 project/package 哈希不变，暂停的原默认画面经过可见复核。该检查覆盖引擎替换/idle 行为，没有重跑 UI 保存/恢复流程。本轮仍不能证明 30 FPS、日用或长时稳定、新 UI 持久化/安装包验收、物理输出行为或 Windows 一致性；按明确要求继续跳过物理休眠/恢复。

### Scene CPU 准备重叠（2026-10-14） {#scene-cpu-ahead-2026-10-14}

本次独立迭代以已合并的 `b092cbfc955887150bb869922eb8b6ece0553300` 为起点，上文 Miku 量测和源码身份保留为历史记录。共用运行时可在等待 compositor callback 或 FPS 就绪期间，通过现有 SceneScript worker 与动画帧/文字 worker 准备下一个 CPU epoch；沿用原 packet slot、worker、不可变资源 owner 与 512 MiB CPU/GPU 门控。CPU 准备不安装 GPU view、不消费历史、不授予呈现就绪或确认首帧，完整 packet 消费/上传/render 仍走有门控的呈现路径。

每个预备 packet 将序号、场景时间与捕获 PCM 频谱绑定，CPU-only 准备期间已验收序号/时间/音频、已呈现像素和历史不变。暂停/idle 或未配置输出、待 apply/commit/rollback 事务、首帧尚未呈现的启动阶段、冻结/隔离 renderer 及等待成功 swap 的帧均不接纳提前准备。暂停与需要 CPU 重构的 replay 只取消预备 timed 工作，带标签的 VM packet 可保留供后续门控消费；已验收重画/resize/回滚先使用已验收 epoch，再消费未来 packet。序号零的时钟 rebase 现仅用于真正的第一次成功呈现，已呈现 seq0 帧的 resize 不再重启时钟。

只有 `state.sessions.len() == 1` 才启用 timed decode ahead。多个 session 时 VM 仍可重叠准备，但 timed decode 保留原有门控策略，并取消预备 timed 结果。timed 结果可能在门控消费前持有全局 producer permit，该守卫避免一个 session 的 callback 被阻止时饿死其他 session。取消后仍由现有 worker/reaper 完成前持有来源/capacity owner。这是保守多 session 策略，不代表物理多显示器验收或多输出通用 timed-ahead 吞吐承诺，Video decoder 与呈现策略保留。

最终 release A–B–B–A 对比使用保留的 #109 基线 A 二进制（`25e1ca224b59bed394a6a88373bd806cada187189627f8d7be49dfb98de4c172`，十二个运行时源码身份与已合并基线一致）及候选 B（`39ed54ca0503401503c350e93ef16aea79922870e9428bf16da3595be97c5615`）。每轮均为 AMD680M 上 1920×1080 嵌套 `niri`、原完整默认配置/效果/历史/资源分辨率、30 FPS cap、自有私有 PCM 与 2 ms 状态查询压力，私有 profiling 关闭；包含三十秒动画、三十秒暂停及三秒恢复。每个 variant 仅两轮短测，不代表显著性或长时结论，百分比不与此前迭代相加。

| Variant | 单轮 FPS 范围 | 成功 swap / 总动画秒数 | 合并 FPS |
| --- | ---: | ---: | ---: |
| A：已合并 #109 基线 | 11.4333–11.9333 | 701 / 60.000116463 | 11.6833106554 |
| B：CPU 准备重叠 | 14.3000–15.1666 | 884 / 60.000078995 | 14.7333139357 |

直接观察吞吐变化为 **+26.1056%**，空绘制尝试从 2,527 次降为零；暂停期间六项活动计数均为零，恢复/清除/join/源码身份门禁通过。texture worker 单核采样 CPU 随吞吐增加从 17.1868% 上升至 25.8182%，不能称为 CPU 占用下降。约 62.08 秒的带阶段标签 CPU 采样窗口可能包含阶段边界，不是精确三十秒阶段 profile。成功 render+swap 的 CPU wall 时间为 4.8431 → 4.8057 ms，不含 ahead worker 等待，也不是 GPU elapsed。

Rust workspace 检查全部通过，**ordinary 559 项 passed（含两项 doctest）、36 项 ignored**；另显式执行十四项 opt-in AMD 自建离屏检查：六项新增 ahead、六项历史、两项共享。新增回归覆盖 ahead 不修改 GPU/已验收状态、序号/时间/音频标签、门控安装、暂停/resize/恢复 replay、冻结回滚、拓扑取消/全局 permit 释放及启动/未确认 swap/shader-only 动画的拒绝。ignored 不计为通过。完整原 Miku 另在软件 EGL 验证：十图层、五特效、二十五 pass、两个 timed 纹理各选中八原帧、三历史、暂停/冻结回滚 replay 精确，以及 candidate/resize/thaw 清理；八个输出像素 hash 与二十四个历史像素 hash 全部匹配已合并 #109 的软件 oracle。CPU peak 525,312,699 B、GPU 旧/候选并集 525,624,984 B 均低于 536,870,912 B（512 MiB），CPU/pool 清理为零。完整软件结果与 AMD 小型离屏样本、原生吞吐量测分别报告，像素相等不代表原生或 Windows 对比。

独立原生 engine-only 检查通过 Miku 暂停/恢复、暂停时换私有 16×16 静态图片（恰好一次 draw/swap）、三秒静态 idle、清除/join 和自有子进程退出；1920×1080 原 Miku/CRT 截图经过视觉复核。自有 `niri`/音频子进程已退出，宿主音频默认值和焦点已恢复。本轮不重验 UI 持久化或安装包，不宣称 30 FPS、日用、物理输出、Windows 一致性或长时稳定，物理休眠/恢复按明确要求继续跳过。

下一步从本轮完整默认 14.73 FPS 基线进行受控逐 pass GPU 归因：维持现有 query/内存预算，对二十五个 pass 轮转稀疏采样，把 layer/stage/pass 关联 owner+序号+时间，确认 CRT、运动模糊或合成热点后，再结合 REA 原版 shader/material/editor 证据优化共用执行路径。这些采样和归因仍为计划。此前私有 `85cc` 诊断的十个稀疏、匹配 ring 尾段样本中，aggregate draw/composite GPU elapsed 均值约 61.32 ms，upload 约 6.34 ms；它们仅是后续调查线索，不是候选 `39ed` 的 GPU 测时、精确稳定阶段或等价负载对比。不能合并 CPU/GPU 时钟及这些均值，或据此预测 FPS。

### Scene GPU pass 调查（2026-10-14） {#scene-gpu-passes-2026-10-14}

本次独立迭代以已合并 `4cb9fe14dcea8534a84f4198f3871af56c0a7139` 为起点，此前量测和源码身份保留为历史记录。临时 AMD GPU query 在 1920×1080、原默认配置、私有 PCM 和 30 FPS cap 下完成串行 A-off/P-on/P-on/A-off 诊断。A 为保留的生产 `39ed` release，P 为临时插桩 `3c135` release；合并吞吐 15.066639 → 14.849973 FPS，观察变化 −1.4381%。不同二进制包含 hook 与采样开销，这不是优化结果，也不代表同二进制或零开销；诊断 hook 为私有，不进入生产变更。

目录包含四十六个 site：二十一个 stage site（十八 render、三历史 copy）、四 bloom，以及独立标注的 upload/composite/copy/canvas/present 区间。二十一加四对应原二十五 logical pass，附加区间不改变该图。四十个 site 在两轮动画 CPU 提交内窗均有样本，缺失 conditional/hidden site 不补零。每第四个序号轮转采样一个 site，加 upload batch，并关联 layer/stage/pass 与 owner、序号、时间。三十二个 query 已创建并删除，八 slot、固定 512 条记录上限；owner 仅进程内唯一，跨轮使用命名空间。

| 诊断区间（零基索引） | 样本数 | GPU 区间均值 |
| --- | ---: | ---: |
| Layer 8 material/composite 到 scene | 4 | 4.983690 ms |
| Layer 2 / stage 1 / pass 0：原 Simple Audio Bars | 4 | 4.606180 ms |
| 三个历史 copy site | 6 / 6 / 5 | 1.993293 / 1.401640 / 1.219672 ms |
| Bloom combine | 4 | 0.674490 ms |

Layer 8 区间覆盖 material/composite，不能当作其独立 flat shader。copy 是 GPU blit/driver 区间，不是稳态 CPU readback；四个 bloom pass 及原尺寸/效果全部保留，不能据稀疏结果删除 pass 或降分辨率。189 个 upload-batch 样本混合 acquire、text、cached/no-op 与实际上传，不代表 189 次纯 atlas transfer。多数 site 只有四至六样本，来自不同帧、PCM 与 atlas 状态。GL completion interval 含 submission/driver 间隔，不是纯 shader busy，不能把不同均值相加当整帧 critical path 或 FPS 预测。CPU begin 时间仅分类有守卫的内窗，query available 轮询也不是 GPU 结束时间，不宣称精确 GPU phase，不做 CPU/GPU 时钟相减。

两个标量 material 尝试均保留为被拒研究。较宽正整数源码在严格 AMD Count=3/frame0 比较出现一个像素 LSB 差异，失败记录保留；最终仅二次幂源码（`0e0b`）虽通过其已记录正确性检查，却为 15.132597 → 14.833289 FPS（观察 −1.977906%），因无性能收益归档并恢复。两者均不交付，其证据与下文 Blend 实现独立。

本轮实现的共用编译路径仅匹配完整 canonical LWE 自有 image/text shader 模板，源码及 metadata 均须精确相同；在 program 编译时将当前 renderer generation 固定整数 `g_BlendMode` 0/1/2（Replace/Alpha/Add）替换为 typed GLSL 常量。其他模板或模式保留原 uniform 路径。原艺术/effect 公式和整数参数、ColorMode、prepared source/plan、资源/copy、效果/分辨率、运行时输入、worker/slot 及 512 MiB 预算不变，不做通用标量或艺术 shader 替换。

独立最终 release A–B–B–A 比较保留的 #110 A（`39ed54ca0503401503c350e93ef16aea79922870e9428bf16da3595be97c5615`，十二个记录的源码 inventory 身份匹配已合并基线）与 Blend B（`0c893ccc5b9d9b713a14a6b817ee750d9a1ea16720e6834aeaff29c292c059fe`）。两者同完整原默认配置/资源/效果、已记录 1920×1080 嵌套 `niri`/AMD680M 环境、私有 PCM、30 FPS cap 与 2 ms 状态查询压力，私有 profiling 关闭；每轮为三十秒动画、三十秒暂停及三秒恢复。

| Variant | 单轮 FPS 范围 | 成功 swap / 总动画秒数 | 合并 FPS |
| --- | ---: | ---: | ---: |
| A：已合并 #110 基线 | 14.7666–14.8000 | 887 / 60.000109578 | 14.7833063346 |
| B：canonical Blend 模板 | 15.1666–15.5647 | 922 / 60.003809558 | 15.3656910585 |

观察吞吐增加 **3.9395%**；每 variant 仅两轮短测，私有 tone/silence epoch 未同步相位，不代表稳定、普遍或统计显著收益，不与此前迭代百分比相加。两 variant 动画/恢复的空绘制尝试均为零，三十秒暂停计数/worker dispatch/render 统计保持零，恢复/清除/join/源码与输入身份、自有 probe/player 退出门禁均通过。成功 render+swap CPU wall 均值为 3.956879 → 3.835216 ms，不含 worker 等待或异步 GPU 时间；带标签 CPU 采样可能跨阶段边界，不将这些均值或旧私有 GPU site 均值合并成整帧时间或 FPS 预测。

最终 Blend 源码通过 Rust workspace 检查：**ordinary 562 项（含两项 doctest）、37 项 ignored**；另显式执行十六项 opt-in GPU 测试：一个含十二内部 case 的新增 AMD 模板测试、六 ahead、六历史、两共享、一个完整原图软件测试。ignored 不算通过。小模板 fixture 验证精确 RGBA 与 sampler liveness，不替代完整 Miku 软件 EGL oracle：十图层、五特效、二十五 pass、两个八帧 atlas 可见、三历史及暂停/冻结回滚 replay 精确。八输出、二十四历史像素 hash 全部匹配 #110，CPU peak 525,312,699 B、GPU transition 并集 525,624,984 B 仍低于 536,870,912 B（512 MiB），CPU/pool 清理为零。ledger 不代表 allocator/RSS 或 driver VRAM，重叠原生 GPU 字段不相加；完整软件、小型 AMD 和原生吞吐各有独立证据范围。

独立原生 engine-only 功能检查通过 Miku 暂停/恢复、暂停时换静态源（恰好一次 draw/swap）、三秒静态 idle、清除/join/退出，并保存 1920×1080 截图；三十一项功能及十五项清理检查全部通过。自有 helper 进程/窗口已退出，宿主音频默认值的最终 raw 前后快照一致，原焦点已恢复；不追溯声称每轮持续观察，也不作为 UI/会话保存验收。

下一步优先评估历史复制复用 Named/History 来源的现有 FBO，保留 Input 来源的当前回退路径、ownership 和已验收 replay/rollback 行为；全帧 blit 已使用现有 FBO，其输入 liveness 单独评估；Layer 8 ColorMode 28 与原 Simple Audio Bars 仍为调查主线，原艺术公式及完整效果/分辨率保留。计划不代表已消除 copy 或进一步收益。本批不代表 30 FPS/日用、物理输出、新 UI/原子保存或安装包验收、Windows 一致性或长时行为；物理休眠/恢复继续跳过。

### Scene 历史 copy FBO 复用（2026-10-14） {#scene-history-fbo-2026-10-14}

本次独立迭代以已合并 #111 `5df7317b9cbe85608cc848b811c74a217d042b1a` 为起点，此前量测、源码身份和被拒候选保留为历史记录。本轮实现仅对 Named/History copy 源借用已有 `RenderTarget.fbo` 作为 READ framebuffer，其不可变 texture attachment 已在构造时验证；Input、alias-preservation 和其他仅 texture 来源保留临时 create/attach/delete 路径。现有 full-frame blit 原本已复用目标 FBO，不算本次新增优化。

copy 本身仍使用 `BlitFramebuffer`、相同边界、`COLOR_BUFFER_BIT`、`NEAREST` 和 pass 顺序，不移除、融合或延迟任何 copy/pass。资源、尺寸/分辨率、generation ownership、已验收历史/replay/rollback、worker/slot 与 512 MiB 核算保留；feedback 或尺寸不符在两条来源路径修改 GL binding 前拒绝。恢复的历史 target 仍须保留经验证 attachment 和 lifetime，这是源 FBO 复用，不是跨 generation 共享 texture/history 或移除 CPU readback。

最终源码通过 Rust workspace 检查：**ordinary 562 项（含两项 doctest）、40 项 ignored**，另显式执行十九项 opt-in GPU 测试：三项新 copy、一个自有 Blend 模板、六 ahead、六历史、两共享、一个完整原图软件 oracle；ignored 不计为通过。新增三项验证临时/复用来源 FBO 与恢复 snapshot 的精确 RGBA 一致、Named/History copy 推进及暂停 resize/freeze-thaw/resume，以及两条路径修改 GL 前拒绝 feedback/尺寸不符。完整原 Miku 软件 EGL 另保持十图层/五特效/二十五 pass、两个八帧 atlas 可见、三历史及暂停/冻结回滚 replay 精确；八输出、二十四历史像素 hash 严格匹配 #111/#110。CPU peak 525,312,699 B、GPU transition 并集 525,624,984 B 均低于 536,870,912 B（512 MiB），结束 CPU/pool 为零。完整软件、小型 AMD 样本和原生吞吐分别报告，不作为 Windows 或原生像素等价证明。

最终 release A–B–B–A 比较保留的 #111 A（`0c893ccc5b9d9b713a14a6b817ee750d9a1ea16720e6834aeaff29c292c059fe`；十五项记录的源码 inventory 身份含 probe/tests，匹配已合并基线，不称十五运行时模块或重新构建）与 B（`7114db88cc33f4f409b67d7169328ba4e2ae1d2cb709bf4755a2f482d87e4cac`）。两者同原完整默认配置/效果/资源、私有 PCM、1920×1080 嵌套 `niri`/AMD680M 环境、30 FPS cap 与 2 ms 状态查询压力，私有 profiling 关闭；每轮为三十秒动画、三十秒暂停及三秒恢复。

| Variant | 单轮 FPS 范围 | 成功 swap / 总动画秒数 | 合并 FPS |
| --- | ---: | ---: | ---: |
| A：已合并 #111 基线 | 14.6000–14.8333 | 883 / 60.000098472 | 14.7166425137 |
| B：Named/History READ FBO 复用 | 15.1666–15.3000 | 914 / 60.000132644 | 15.2332996566 |

观察吞吐增加 **3.5107%**，两轮 B 均高于两轮 A，但每 variant 仅两轮短测且私有 tone/silence epoch 未同步，不代表稳定、普遍或统计显著收益，不与此前百分比相加。动画/恢复无空绘制，三十秒暂停 render/draw/swap/empty/callback 与 dispatch、CPU render 统计均为零，恢复/清除/join/身份/自有 probe/player 退出门禁通过。成功 render+swap CPU wall 均值 3.966114 → 3.823484 ms 不含 ahead worker 等待或异步 GPU，也不证明 GPU elapsed 下降；带标签 CPU 采样可能跨阶段边界，不能与旧 GPU site 均值合并预测 FPS。四轮各自直接记录 1920×1080 输出，宿主 geometry 仅 B1 中途快照，不代表 batch 起点或逐轮持续可见性/宿主音频监视。ledger 不是 allocator/RSS 或 driver VRAM，重叠字段不相加。

独立原生 engine-only 检查通过原 Miku 暂停/恢复、暂停时换静态源（恰好一次 draw/swap）、三秒静态 idle、清除/join/退出；三十一项功能与十五项清理检查均通过，1920×1080 截图另经主任务视觉复核。三个自有 helper PID/窗口退出、两个 supervisor/session exit0、宿主音频默认值 raw 前后快照一致、原焦点恢复均已核对，不扩展为逐轮持续观察或 UI/会话保存验收。

REA 独立静态检查原 x86-64 PE `wallpaper64.exe`（SHA-256 `bced836b83eaa078d212eb4781e4e7c4fc89ab04f84d6e85ac3c1ee6081c3e42`），未执行原程序，保留三十一条 Evidence。`FUN_1401598b0` 区分空 target handle 与已有 target resize 路径；`FUN_14017ab60` 对 named target 做 hash/compare 并返回保存指针；`FUN_1401abd20` 的 image-layer 路径 lookup、test，命中即跳过 create；`FUN_14013f7b0` 的尺寸相关 resize 另跳过未变尺寸。这些观察支持调查通用 render-target 持久复用，LWE 的实现及收益仍需独立验证。

原 PE 的 `FUN_14010e2c0` 实际调用 D3D11 device create，因此静态缓存证据不证明 OpenGL FBO 复用、原 History API/view lifetime、copy 融合/延迟/删除、GPU 成本或 FPS。原 History dispatch/copy API、Windows 执行、device-loss 和长时行为仍未知，缺失 History 字符串不能确定实现；原反编译源码不复制进仓库。下一步优先继续对 Layer 8 ColorMode 28 与原 Simple Audio Bars 做有界逐 pass 归因，关联 owner/序号/时间并评估共用执行路径；原艺术公式、完整效果/分辨率、资源和预算保留，不预先宣称进一步收益。本批不代表 30 FPS/日用、物理输出、新 UI/会话保存或安装包验收、Windows 一致性或长时行为；物理休眠/恢复继续跳过。

### Scene 绘制归因与 FullFrame 来源复用（2026-10-15） {#scene-material-bind-2026-10-15}

本次独立调查从已合并 #112 `71b69f5497929b70d013694f6315d5020049dd41` 开始，此前日期记录和被拒候选保持原样。REA 对上文同一原 `wallpaper64.exe` PE 身份保留六十条 Evidence：复用三十一条、新增二十九条，包含二十三个指令锚点；未执行 Wallpaper Engine/Wine。观察到的 material 路径在 dirty bit 清零时跳过静态值更新，更新后清除此位；具体 shader vtable 将该更新与动态绑定分开。metadata 驱动的 `FUN_1400c57e0` packer 对每个 32 段通道复制 128 字节，16/64 段另有 64/256 字节分支。这是静态 CPU 打包证据，不是 Windows 实测活跃范围、每帧 driver 调用、GPU 耗时，也不证明 LWE 音频上传过大；原版全部 mutation 路径及精确 backend API 名称仍未证实。原 ColorMode 28 公式保留，未建立精确算术替换。REA 会话已关闭，反编译源码不导入仓库。

随后私有两 site 探针在 AMD Radeon 680M（`radeonsi`、Rembrandt）、desktop GL 4.6/64-bit timestamp 上记录 LWE 实际 linked program 及独立 CPU/GPU 区间。A 为保留的 #112 release `7114db88cc33f4f409b67d7169328ba4e2ae1d2cb709bf4755a2f482d87e4cac`；P 为临时 instrumented release `8616d101e7b6cca6b210f29ee0a868764fb9403458b1380c20fb47e3887ac8a5`。冻结 renderer/module/helper 源码快照 SHA 前缀分别为 `ea88e30e` / `bcea691f` / `223523b2`，完整身份保留在私有 build manifest。诊断源码已归档/恢复，不作为产品功能提交。

串行 A-off/P-on/P-on/A-off 保留完整原默认配置、私有 PCM、原生 1920×1080 输出、30 FPS cap 与 2 ms 状态查询压力。P 每第四个 normal sequence 轮换一次，在 Layer 2/stage 1/pass 0（Simple Audio Bars）与 Layer 8 composite 间选择一个 site；两者实际离屏目标均为 **3840×2160**，不是原生输出尺寸。每个 context 的三十二 query 全部创建/删除，八 slot、512-record cap、最终 pending0；图像 ledger 的 512 MiB 上限保留，query 对象及私有诊断 bookkeeping 与图像计费字节分列。

P1/P2 共输出 260 条已呈现记录，guarded 动画 CPU-submission 窗口保留 210 条：Audio Bars 104、composite 106；启动/settle/边界四十三条与恢复阶段七条另列。用 CPU begin/clear/draw anchor 选取 `[observed begin + 1 s, min(observed end − 1 s, begin + actual probe duration − 1 s)]`。旧 query 的异步结果可能在暂停/恢复后才回收，结果读取时间不用于归类提交阶段，也不是 GPU 完成时刻。owner/序号/time bits/轮转按 process/context 验证，不跨进程合并同名 owner；replay 与 seq0 排除遵循冻结 producer 契约，两轮 P 各排除一次 warmup。

| 动画提交内部窗口区间 | 样本 | 均值 | 中位数 | 范围 |
| --- | ---: | ---: | ---: | ---: |
| Audio Bars DrawArrays-only completion | 104 | 4.579791 ms | 4.613920 ms | 4.301520–5.352960 ms |
| Layer 8 DrawArrays-only completion | 106 | 5.159573 ms | 5.162860 ms | 4.895080–5.395960 ms |
| Audio Bars Clear-only completion | 104 | 13.096538 µs | 5.100000 µs | 3.480000–807.920000 µs |
| Audio Bars CPU spectrum loop | 104 | 0.706683 µs | 0.666000 µs | 0.521000–1.343000 µs |
| Audio Bars 其余 CPU 绑定 span 合计 | 104 | 3.756567 µs | 3.410500 µs | 2.463000–20.210000 µs |
| Layer 8 其余 CPU 绑定 span 合计 | 106 | 1.242396 µs | 1.252500 µs | 0.431000–2.334000 µs |

实际 linked Audio Bars program 活跃左右两组各 32-float 的 spectrum array：每次被采样绑定为两次 active API call、64 floats、**256 字节**。loop 仍尝试六次调用/224 floats，16/64 段 location 为 −1，被忽略的调用不算上传。composite 活跃 matrix、两个 texture 及 color uniform，没有 spectrum-binding loop；该调用的 Clear/spectrum 为不适用，不填零样本。active API payload 不是 PCIe 流量或 GPU 实际读取量。Clear 的 807.92 µs 单点保留，不删 outlier 制造稳态估计。GPU 区间为相应 GL 调用周围的 completion interval，不是纯 shader busy time；other-bind CPU span 也不是完整 site 关键路径，轮换 site 来自不同帧，均值不能相加或预测 FPS，不跨 CPU/GPU 时钟相减。

instrumentation 对照合并 FPS 为 14.799973 → 14.949973（观察 +1.0135%），比较的是含 hook/采样的不同 baseline/private 二进制，不是 same-binary off/on、生产优化或开销可忽略的证明；每 variant 仅两轮三十秒。A1 动画阶段保留两次 empty（444 attempts = 442 draws + 2 empty），呈现/callback/bucket 核算仍一致，不能称全组 empty0。P2 cleanup 丢弃一条未回收 sample（133 selected、132 emitted），缺失 duration 不补零，三十二 query 仍完整删除；ring-full/QBO/clock/incomplete/unpresented/overwrite 均为零，但不能称全组无丢弃。暂停 counter/dispatch、恢复、清除/join、原输入身份及记录的自有 probe/player 退出另行核验。

另一个独立三秒基线 shader 诊断先以 `AMD_DEBUG=ps` 通过运行，但未捕获 NIR/ISA；修正后的 child-only 请求使用 `AMD_DEBUG=ps,nir,asm,stats` 与 `MESA_SHADER_CACHE_DISABLE=true`，未修改 host driver 配置。保留的请求 manifest 记录这些 flag，harness 的 environment subset 本身未重复记录。随后直接观察到来自未变 A 二进制的 213,119 字节 NIR/ACO ISA 原始捕获，SHA-256 为 `0da29f0215c89c14394ec4a0518ecb64a2659180023eeef1d8069310ca5e76d5`。flag 调查记录了独立的 stage/output gate 及 [Mesa 23.1 release notes](https://docs.mesa3d.org/relnotes/23.1.0.html) 所述的 flag 调整；所读 upstream 源码只是结构参考，不是本地 package 的精确源码身份。编译/cache 扰动使该捕获不计入吞吐测量。

dump 名称 `GLSL15`/`GLSL60`、相同 program 编号、唯一 linked interface 及 Audio32/Color 数据流共同支持与两个归因 site 的映射。但捕获与 P1/P2 属于不同进程/二进制，未输出同进程 expanded-source-SHA/link-attachment marker，不能称为密码学 GLSL-to-NIR 身份。精确 `source_blake3` 只用于 **dump 内部** NIR↔ISA 关联，不绑定另一进程，也不按编译顺序推测归属。

Audio NIR 已有四个 lane-varying `load_ubo`，紧凑字节范围为 0–127/128–255；ISA 对应四条 `buffer_load_dword ... offen`，右通道 offset 为 128。array lookup 没有展开成 32/64 元素 comparison/select 链，bar 形状的其它 select 仍保留；这是 driver 内部 default UBO，不是应用声明的 std140 block。Color NIR 已消除背景 hue/saturation，仅通过 min/max/average 求背景 lightness；源 hue/saturation、最终 HSL-to-RGB、opacity 插值和背景 alpha 仍保留，两次纹理采样均活跃，不能据此删掉背景采样或其所需内容。

| 最终 ISA 资源字段 | Audio | Color composite |
| --- | ---: | ---: |
| VGPR / SGPR | 32 / 108 | 24 / 108 |
| Code bytes | 856 | 948 |
| Scratch bytes/wave / SGPR spill / VGPR spill | 0 / 0 / 0 | 0 / 0 / 0 |
| Max Waves | 16 | 16 |

表中为最终 ISA 字段；较早 `Shader Stats` 的 836/928 B code size 属于另一 dump 标签。Max Waves 是编译器容量字段，不是实测活跃 occupancy；静态 load 或零 spill 不证明 cache miss、每指令成本或瓶颈占比。现有证据不足以支持改用显式 UBO，或删除已被消除的背景完整 HSL 计算；原公式继续保留。

另一个 production-path material 候选经过验证，**因未显示实际吞吐收益而被拒绝**。它尝试在所属 pass/program 首次成功 bind 时上传非 `g_` material 常量，`g_` override 仍在原 per-bind 位置写入，time/audio/matrix 输入保留动态路径。归档 renderer 快照为 `fc1c45ee9c3c7882d241d75f7fb0ffdef9966fdab1ba5d1dae42d5684e30d875`，无私有 profiler 的候选 release 为 `439fb7ce7dcff24a064851d493b42b7b9eeb51e0aa6cd1dc1d0f98d3dbcfd8af`；十七文件冻结库存包含 probe/tests，不代表十七个实际运行模块。renderer 及两项新增测试已归档/恢复，这个 material 上传优化不交付。

以下正确性检查仅属于该被拒候选：**ordinary 562 项（含两项 doctest）、42 项 ignored**，另显式执行二十一 GPU 测试（material 新增二、copy 三、自有模板一、ahead 六、历史六、共享二、完整原图软件一）。新测试覆盖隐藏后的首次绑定、override、冻结/解冻像素，以及首次上传失败后重试且成功前不标记 ready。八输出/二十四历史 hash 及整帧 metadata 与基线严格相同，原 10/5/25、两个八帧 atlas、三历史及 paused/frozen replay 保持；CPU peak 525,312,699 B、GPU transition 并集 525,624,984 B 和结束 CPU/pool 零计费均不变。软件/小型 AMD 正确性通过不把被拒源码变成交付或原生像素等价结论。

独立、profiling 关闭的 release A–B–B–A 保留完整原默认配置/效果/资源、原生 1920×1080、私有 PCM、30 FPS cap 与 2 ms 状态压力，每轮三十秒动画、三十秒暂停、三秒恢复；记录的合并结果如下：

| Material 候选对照 | 单轮 FPS 范围 | 合并 FPS |
| --- | ---: | ---: |
| A：保留的 #112 | 15.1333–15.4000 | 15.2666388412 |
| B：被拒的首 bind material 常量 | 15.2333–15.3666 | 15.2999669087 |

观察 **+0.2183%** 落在互相重叠的单轮范围和时间趋势内，没有建立实际或稳定收益，不与此前百分比相加，不足以保留该修改。这组 `material-runs` 与上文私有归因 A/P 对照独立；正确性通过或 uniform 调用减少本身不算性能验收。

独立 FullFrame 来源复用候选只改变 layer 前的 snapshot 决策：staged、非 direct 且没有 authored composite 的 layer，在 `work_offset >= 2` 保证全部 scratch/named target 避开两个 full-frame slot 时，可读取当前 composition 纹理；这些读取均在最终 copy compositor 写入 composition 前结束。其它情况保留 snapshot，包括 direct/partial-work 与 authored-composite 分支。这是通用 liveness 条件，不按 Miku/layer ID 特判；shader 算术、prepared graph、源分辨率、资源分配和预算上限不变。

保留的候选 release 为 `73aa35ef86820048c0029b5c060a5b4aedc7d713ca73f3dc3fde353fa4688a30`，renderer 为 `50fef0b3edcd8229992ef01dabbe823cab9b93216ca3385cb60212fc055af6b1`，最终测试源码为 `a3e2e283b49cc0b236a9a0e1619651c82ab0c5d506972effef98c45530a91db5`；十七文件库存含 probe/tests。初始源码通过 562 ordinary/42 ignored 与二十一显式 GPU 测试。最终 focused 两测试复跑另以毒化 snapshot slot 证明 fast path 不更新它，同时保留严格 RGBA、history/discard/alpha/thaw、replay 与 fallback 覆盖；两项复跑不另计为两个 unique GPU 测试。最终 engine clippy/fmt 通过。完整原图软件 EGL oracle JSON 的 SHA-256 `917825fff4d4c0536a4c53c5929838a15893bc45a2b6cc0d8ae15a62e271ceb1` 与 #112 相同，含八输出、二十四历史、metadata/graph、CPU peak 525,312,699 B、GPU union 525,624,984 B 及结束零计费。

另在 AMD 离屏执行完整原图对照：重新构建 #112 基线，再恢复并测试冻结候选源码；两者均使用相同的固定测试频谱、2160×1440 输出和原 4K composition/history target。八输出、二十四历史 hash、frame metadata、图结构、replay 及资源/清理检查全部一致，两份报告的 SHA-256 同为 `3c1c4fb2a5af42ae4e2dbdbc82fe43334e300b6c363559b61e2a5975b352cb04`，生产源码已恢复并核对。这是同一原场景测试的两次执行，不额外增加两个 unique 回归；证明与 #112 的硬件离屏一致，和实时 PCM 原生呈现、Windows 等价分别记录。

独立、profiling 关闭的原生 release A–B–B–A 保留完整原默认场景、私有 PCM、1920×1080 输出、30 FPS cap 和 2 ms 状态压力；每轮三十秒动画、三十秒暂停、三秒恢复，按实测 duration 加权的动画结果如下：

| FullFrame 对照 | Swaps / 实测秒数 | 单轮 FPS 范围 | 合并 FPS |
| --- | ---: | ---: | ---: |
| A：保留的 #112 | 883 / 60.000115738 | 14.566637–14.866640 | 14.7166382788 |
| B：FullFrame 来源复用 | 982 / 60.000110696 | 15.933304–16.799968 | 16.3666364713 |

观察 **+11.2118%**，两轮 B 均高于两轮 A，本批据此保留修改。它仅省去原 Miku 两个符合条件 layer（2/9）的 layer 前全幅 helper snapshot，二十五个 logical pass 全部保留，Audio/Color 公式不变。成功 render+swap 的 **CPU wall** 均值从 3.969689 降为 3.525623 ms，不是 GPU elapsed。动画/恢复 empty 均为零；暂停全部 attempt/draw/swap/empty/callback counter、worker dispatch 和 render CPU total 均为零。恢复、clear/join、自有 probe/player 退出、原输入和二进制身份、私有环境及 512 MiB 门控通过。逻辑图像计费不是 RSS/VRAM，重叠 ledger 字段不相加。

独立原生 engine 功能检查使用同一冻结 B 二进制，十七项记录源码身份均匹配：原 Miku 动画三秒、暂停三秒计数全零、再恢复三秒；暂停时换私有静态图恰好一次 draw/一次 swap，之后三秒 static playing 全部帧计数/dispatch 为零。clear、backend join、自有 probe/player 退出通过，原输入与二进制未变。审阅者已查看 1920×1080 暂停截图，默认 Miku/CRT 完整；它证明一个原生可见帧，不证明 Windows 像素等价或持续可见。

清理前核验 exact owned command，仅停止本批嵌套 compositor/私有音频；两个 supervisor 均以零退出，三个自有 helper PID 及窗口消失，host 音频默认 sink/source 的前后原始读数一致，并恢复原应用焦点。这些前后读数和批次间一次 host snapshot 不扩展为持续可见或每轮 host 音频观测。

每 variant 两轮三十秒、未同步的十二秒 PCM epoch 不证明统计显著、日用稳定或通用性能收益；该百分比不与此前批次相加，也不靠轮换诊断 site 均值相加来解释。下一步应先验证可用硬件 counter 能力与 instrumentation 扰动，再区分采样/写回流量与算术成本，之后选择生产修改；已有证据不再支持假设性的 array-select/UBO 或已消除的背景 HSL 方向。

本批不代表 Windows 运行、30 FPS/日用、物理多输出、新 UI/原子保存、安装包或长期验收；物理休眠/恢复继续跳过。

### 已记录的实验性 Scene 验收

2026-10-09 的开发版本验收环境为 Wayland 与 `niri`、AMD Radeon 680M（`1002:1681`，`amdgpu`），仅一个 `eDP-1` 输出，2160×1440、缩放 1.25。场景样本 `3337606455` 使用默认零 X/Y 相机，包含一个图片图层与 Shine：5 个特效 pass、总计 6 个 pass。全分辨率截图确认 Shine 实际可见渲染。

最终兼容性门控在本地目录中仅允许该 Scene 样本播放。`desktop_apply_flow_scene_video_switch_clear_and_restore` 通过了三轮 Scene ↔ Video/应用/清除、隔离会话保存失败回滚、commit 已处理后的取消，以及后端关闭并 join 后的 Scene 分配恢复。原有三轮视频重新应用测试也通过。测试使用隔离的 `XDG_CONFIG_HOME`，已恢复原工作区画面，用户原配置未修改。

在 2026-10-09 的这次验收中，`3258032485` 因非零 X/Y 相机语义未确认，以 `unsupported_camera` 阻断。此前截图属于研究，不证明支持。这段历史记录只覆盖单显示器上的一个已验收 Scene，不代表完整 Scene 兼容；独立多显示器行为当时未测试。场景路径由 Rust 独立实现，不依赖 Open Wallpaper Engine 运行时。

### 2026-10-10 开发记录

新的受限相机分支基于本机原版 `wallpaper64.exe` 的 REA 证据：正交加载器在活动相机路径向量为空时重置保存的 eye/center/up，运行时相机更新使用重置后的值。LWE 仅允许显式固定尺寸、保存向量平行且向前、默认 up、未启用自动投影、路径缺失/为空，其他相机模式仍阻断。该证据支持 `3258032485` 所用的受限实现，不代表与原引擎视觉一致。原版视觉 A/B 尚未执行。

| 范围 | 已实现的证据/工具 | 本版验收状态 |
| --- | --- | --- |
| P0 inventory | 有界声明/结构 API、同 descriptor PKG 指纹、隐私与循环前总预算回归 | 最终 `scene-inventory-p0.json`：11 项、2,695,798 字节，11 个 PKG 的 SHA-256 均匹配基线。可达声明包括五个项目中的 31 处 JS、五个项目中的 20 处 puppet，三个项目存在层级声明；不判定运行时资格。 |
| P1 运行时 | 每输出截止时间、稳定静态/暂停空闲、计数器、包含回滚资源的 512 MiB 逻辑 RGBA/FBO 核算、一个/两个临时目标；EGL 生命周期清理和单次初始视频加载 | `scene-probe-shine.json` 已通过频繁状态请求下的 30 FPS swap 完成率上限和独立 60 秒暂停/静态空闲窗口。资源后续记录见下文。Settings FPS/电池持久化及实时接入、独立 15 → 30 FPS 桌面服务测量和有限原生 Settings/重启流程已通过。显示时间戳、GPU p95、硬件电池/交流电转换、VRAM/功耗/长期运行仍未验证。 |
| P0 真实 UI | 原生鼠标选择显示器 → 应用 → Desktop → 标题栏关闭/隐藏 → 自有进程重启/恢复 → 目标清除，隔离会话与无遮挡截图 | `3337606455`、`3258032485` 和原始默认 `3659880761` 通过原生鼠标应用/隐藏/新进程恢复/清除与截图复核；旧报告保留 365 当时的阻断解释。365 默认流程另通过 Settings 15 → 30 FPS/重启偏好，下文记录的独立属性/失败/重置/全屏流程也已通过。清理与原工作区恢复均通过。 |
| P2 相机 | 自建正向/阻断样本、原版二进制控制流/常量证据 | 原引擎视觉 A/B 与日用一致性未验证。 |
| P2 属性 | 精确 bool/slider/color 绑定默认值和经验证的项目/输出编辑；分配/属性原子持久化与回滚 | 未知表达式/类型/形状及错误声明继续阻断。原生 bool/slider 编辑、真实写入失败回滚/重试、inode 原子替换、新进程控件恢复与完整默认重置已通过；CPU/持久化测试仍与 UI 证据分开。 |

此前开发快照通过了资源 crate 的 18 个测试及 clippy；独立隐私/预算/指纹复核没有剩余阻断。Rust fmt、workspace clippy、check 和测试已通过。普通引擎测试通过 49 项、忽略一个 GPU 样本；随后在 surfaceless EGL 下显式运行该样本并通过，普通 ignored 结果不作为其 GPU 证据。最终 inventory 已确认共享的 16,384 mip/16,384 动画帧预算在循环前扣除，哈希后再次核验索引 descriptor stamp。CSS 修复后的前端类型检查报告零错误/零警告，25 个文件中的 113 个测试及生产构建均通过。Python 套件通过 88 个测试，包含五个 UI 工具安全回归。上述数量属于此前快照，后续设置/属性/资源改动需要独立记录最终检查。文档构建结果单独记录。100 轮结果见下文。

2026-10-10 的 `3337606455` UI 流程在所记录的 Wayland + `niri`、AMD 680M 单 `eDP-1` 机器（2160×1440，缩放 1.25）上通过。原生鼠标选择核验了 Library 身份 `d6d309c547f3c764` 与显示器身份 `niri:BOE:0x0893:eDP-1`（后端 `eDP-1`，UI 标签 `BOE 0x0893`）。应用保存分配后，对应 Desktop 卡片显示 Running。正常标题栏 Close 隐藏窗口，同一应用 PID、壁纸和会话保留。测试随后结束自己的进程组，启动新应用进程，确认保存恢复并清除目标。`3659880761` 保持可浏览，Apply 禁用且显示属性绑定解释。应用/恢复/清除的背景截图已确认获得焦点且无遮挡。

原生鼠标测试发现 CSS 作用域问题：`body > div` 把应用根尺寸套到了显示器菜单的 portal wrapper，菜单因此超出可视区。应用根新增 ID `lwe-app`，相关样式改为 `body > #lwe-app`，修复后原生鼠标选择通过。验收使用原生 WebDriver 点击，没有合成 DOM 输入。

最终 Shine 与 Shake 定时截图在静态桌面部件以外的背景区域都有像素变化（ROI x=100–1700，y=400–1400）。两份 UI 报告均完成画面审查，并有独立 visual-review 与 animation-difference 记录。这证明像素变化，不证明与原引擎视觉一致。普通托盘 Show/Quit 未测试，结束自有进程仅是测试中的重启步骤。`3258032485` 最终 UI 复跑也通过：Library 身份 `18bab41e6ab0c721`、同一显示器的原生鼠标选择、同 PID 关闭隐藏、自有进程退出后新进程恢复、目标清除，以及 `3659880761` 当时的属性阻断解释。报告无清理错误，恢复原工作区 ID 1。swap 完成率/空闲 Probe 及 100 轮生命周期压力均通过，记录见下文；后续资源诊断仍不足以证明日用 RSS 稳定。

最终 `scene-probe-shine.json` 记录了 `eDP-1` 上设置 30 FPS 上限的 engine-only Probe，不写 session/settings，也不测试 UI。动态窗口每秒处理约 475.93 个状态请求，稳定的逻辑 Scene RGBA/FBO 负载为 51,871,748 字节（49.47 MiB），不等于实测 VRAM。

| 阶段 | 窗口 | 完成的 swap | 其他观察 |
| --- | --- | --- | --- |
| 动态 Shine | 60.000055 秒 | 1,773 次，约 29.54997/s | 频繁命令唤醒下 draw 为 1,773 次。 |
| 稳定后的暂停 | 60.000080 秒 | 0 | Render attempt、draw、empty frame、callback 请求/接收均为零。 |
| 恢复 | 3 秒 | 89 次，约 29.66595/s | 动画恢复且仍受配置上限约束。 |
| 暂停时的自建静态候选 | 第一帧 | 1 | 恰好一次 draw 和 swap，完成候选接纳。 |
| 稳定后的静态播放 | 60 秒 | 0 | Render attempt、draw、swap、callback 请求/接收均为零，逻辑负载 1,032 字节。 |
| 清除与关闭 | Probe 结束 | — | 输出指标移除，后端关闭后 join 成功。 |

这些是引擎完成 swap/callback 的计数及有限空闲窗口观察，证明命令流量下的截止时间约束，以及该窗口内没有持续暂停/静态渲染循环。它们不测量合成器显示时间戳、GPU 渲染耗时 p95、普通空闲 CPU/功耗、VRAM、24 小时运行或多个输出。下方已完成的 100 轮生命周期结果不证明内存平坦或日用就绪。

最终 `scene-stress-100.json` 和日志记录了同一后端上交替 Shake/Shine 的 100 轮循环，每轮 Scene → Video → Scene → Clear。Rust 测试在 424.37 秒内通过，外层流程耗时 430.392 秒。隔离保存失败回滚、commit ACK 迟到后的截止时间取消、后端关闭/join、保存的 Scene 恢复和最终清除均通过。用户原配置未改变，自有应用进程已退出，工作区 1 已恢复。

整个测试进程的 RSS 包含准备/缓存/引擎/libmpv，峰值 832,548,864 字节（793.98 MiB），结束时 714,219,520 字节（681.13 MiB）。四个时间段的中位数从 562.84 → 620.44 → 641.48 → 664.35 MiB 上升。这不是应用专属内存或 VRAM，在该次验收时增长尚未归因。P1 日用资源稳定性未通过。四段 FD 最大值均为 22，结束为 14；起始 4 不是稳定运行基线。这些窗口内未观察到 FD 增长，不证明长期稳定。

资源后续工作增加了 context/display 的显式销毁和幂等 surface 清理，覆盖初始化失败路径；display/library 的所有者保留到最后一个 context/surface 释放。视频构建改为只加载一次初始输入源，移除了连续两次 `loadfile replace`。这些改动处理实际的生命周期和重复工作问题，不宣称它们解释了此前全部 RSS 增长。

私有 `resources-{cpu,scene,video,mixed,restart}.jsonl` 和 `resources-summary.json` 记录同一单输出 680M 机器上的独立 engine-only 子进程，均成功退出，用户原配置未改变。下表比较首轮与末轮释放/清除后的采样，不是冷启动基线：

| 模式 | 循环次数 | 释放/清除后 Rust 请求的活跃字节 | 其他观察 |
| --- | --- | --- | --- |
| CPU 准备/释放 | 30 | 2,177 → 2,177 | 不启动桌面后端，解码的 Scene 数据每轮释放。 |
| Scene 应用/清除 | 30 | 49,259 → 51,539 | 后端保持运行，该小幅变化不证明已经达到稳态。 |
| 视频应用/清除 | 30 | 49,259 → 49,259 | Rust 活跃字节固定，RSS 仍上升。 |
| Scene → Video → Scene → Clear | 50 | 49,637 → 63,029 | glibc 空闲块：166,546,944 → 654,284,608 字节；RSS：290,889,728 → 614,535,168 字节。glibc 已用块从 15,622,656 变为 16,472,256 字节。 |
| 后端重启 | 20 | 每次关闭/join 后均为 2,229 | 每个停止后的快照均为四个 FD、一个线程。 |
| 全新子进程 `MALLOC_ARENA_MAX=2` 混合模式 | 100 | 49,260 → 51,846 | glibc 空闲块：134,691,232 → 198,131,456 字节；清除后 FD 均为 12，末轮清除后线程为 19。 |

这些计数支持分配器保留的大量空闲空间是视频/混合模式进程内存趋势的一个来源，不证明全部原生分配正确，也不证明 RSS 达到稳定上界。Rust System 分配计数与 glibc 块计数的范围不同，Probe 也未包含 shell 的目录/缓存/UI 工作。P1 继续开放。

私有 `resources-mixed-arena2.jsonl`/日志中的受控 arena 对照在 174.26 秒内通过了 100 轮。最终关闭/join 后 Rust 活跃字节为 2,228，四个 FD、一个线程、无 DRM client。末轮清除后的 RSS 为 226,689,024 字节，另有 29,519,872 字节换出，文件页驻留也发生变化。观察结果与 arena 策略影响空闲块保留相符，但默认模式运行 50 轮、受控模式运行 100 轮，内存压力不同；它们不是生产性能对照或 RSS 收益证明。环境变量只传给诊断子进程，未采用产品分配器调优或强制 trim。

后续实现还接通了可持久化的 FPS/电池控制与引擎实时策略更新。实时更新失败时，已保存设置仍为权威状态，UI 会暴露运行时问题。FPS 选项限制按节奏执行的渲染/swap；电池暂停保留用户手动暂停意图。显式启用的 `desktop_settings_live_updates_preserve_assignments_and_backend` 测试在隔离配置下用时 22.05 秒通过，私有 `settings-live.log` 记录如下：

| 保存的上限 | 测量窗口 | 完成的 swap/速率 | 状态请求 |
| --- | --- | --- | --- |
| 15 FPS | 10.0003 秒 | 147 次；14.6996/s | 4,589 |
| 实时改为 30 FPS | 10.0004 秒 | 296 次；29.5987/s | 4,758 |

两份采样均确认后端相同、分配字节未变，并回读保存的偏好。电池暂停偏好已保存并进入引擎策略，机器报告 `on_battery=false`、`power_paused=false`，没有测试实际电池/交流电转换。这些是实际服务路径完成的 swap 计数，不是合成器呈现时间戳或 Settings UI 操作。

独立原生 UI 流程在私有 `ui-settings-verified/report.json` 中通过，`visual-review.json` 记录画面复核完成。原生英文 Settings 操作保存了 15 FPS 和电池暂停，在壁纸运行时改为 30 FPS，并在新的自有进程恢复壁纸后核验 30 FPS 与电池偏好；中文 Settings 布局也已复核。应用、同 PID 关闭隐藏、自有进程组完整退出后重启、保存恢复、目标清除和 `3659880761` 当时的可见泛光阻断均通过。清理错误为空，隔离配置已删除，原工作区 ID 1 已恢复。此前 `ui-settings-final` 自动结果的 15/30 FPS 截图取景不完整，不计为这次新的视觉验收。这个有限流程不证明硬件电池转换、锁屏/休眠恢复、功耗、24 小时运行或完整 P1 就绪。结果来自开发 checkout，安装包验收仍待完成。

### 混合 Scene 与每输出属性：2026-10-10

独立 Rust CPU/GPU 路径已实现 `3659880761` 原始包的活动粒子树、WaterRipple/Shake/Iris 特效、LDR 泛光和作者配置的单条 FLAC 循环层。准备成功，包含 7 个渲染图层、5 个特效和 21 个 pass。默认配置为 6 个可见粒子根系统加全部 5 个子系统，最大粒子/顶点/work 为 3,965/26,790/30,980。第七个 Reactive 根系统是字面量 `visible:false`，保留作者隐藏状态才是正确默认播放；启用其音频分析仍阻断。没有为通过准备而关闭作者开启的粒子/特效/泛光层。

粒子按 SceneClock 执行有界的发射器、初始化器、运动/阻力/重力、透明度/尺寸振荡、角运动、湍流、sprite/rope 渲染及 spawn/follow 子系统。未知行为明确报错。OWE 源码仅供行为参考，Rust 运行时使用独立状态结构、RNG 和梯度/curl 场；原版噪声、子系统生命周期/变换、rope 采样及完整画面等价仍未验证。

REA 确认了 LDR 四 pass 输入链、RGBA8 颜色目标、向下取整且最小为二像素的缩放尺寸，以及来自完整输出尺寸的 texel。另确认单条循环无随机 min/max 间隔、图层音量采用平方。这些是原生静态发现，与视觉或音频验收分开。

最终有限 engine-only Probe 私有记录为 `runtime/365-engine-final-60s.json`，在所记录的 Wayland + `niri`、AMD 680M、单 `eDP-1` 机器上通过，shutdown 已 join、清除后输出指标移除；不写 session/settings，不进行 Tauri UI 操作：

| 阶段 | 观察 |
| --- | --- |
| 默认混合 Scene，30 FPS 上限 | 六十秒完成 1,677 次 swap，约 27.95/s；约 466 次状态请求/s，empty frames 为零。 |
| 稳定暂停 | 六十秒与 28,000 次状态请求下，render attempt、draw、swap、empty frame、callback 均为零。 |
| 恢复 | 三秒完成 85 次 swap，约 28.33/s。 |
| 逻辑 GPU 负载 | RGBA 纹理/FBO 与粒子 VBO 共 134,862,328 字节，不含驱动/EGL/视频开销。 |

另一次私有 Pulse sink/PCM Probe 在候选、暂停、静音和 drop 阶段观测到零 PCM，激活后 RMS 为 2,894.53。自有 sink 已清理，原有 sinks、默认 sink/source 未变。这不证明硬件扬声器输出、每个循环边界或完整属性/音频回滚事务。

独立的默认原生 UI 流程在私有 `runtime/ui-365-default-final/report.json` 中通过，`mockIpc=false`，使用隔离配置并人工复核截图。原生 Library/显示器应用达到 Desktop Running；标题栏关闭隐藏同一 PID，壁纸/session 保留；整个自有进程组退出后，新 Tauri 进程恢复 Scene，目标清除还原基线。原生 Settings 保存 15 FPS 和电池暂停，在 PID/分配字节不变时改为 30 FPS，新进程恢复后回读 30 FPS 与电池偏好。英/中文截图已核验完整构图、两帧粒子变化、恢复可见、清除基线和标签布局。cleanupErrors 为空，工作区 1 已恢复，没有关闭原始默认活动特性。这通过的是默认 UI 流程，独立属性与有限全屏流程记录于下文，日用验收仍未完成。

有限 `runtime/365-resources-mixed-20.jsonl` 及摘要记录 20 轮 `3659880761` → 视频 `2848041076` → `3659880761` → 清除。每次清除均确认 active status/metrics 为空，关闭已 join。清除后 FD 恒为 12、Rust 活跃字节恒为 48,811；首/末轮进程 RSS 为 257,609,728 → 287,825,920 字节，glibc 已用块为 14,897,040 → 15,433,312 字节。关闭后为四个 FD、一个线程、2,227 Rust 活跃字节、191,705,088 字节 RSS 和 250,144,480 字节 glibc 空闲块。这支持该有限窗口的资源归还及无 FD 累加；显著分配器/驱动 RSS 保留不支持内存稳定或全部原生分配无泄漏声明，功耗与日用长期运行仍未验收。

最终实现快照通过 workspace fmt/clippy/check/测试和 98 个 Python 测试。错误状态重置修复后，前端类型检查零错误/零警告，25 个组件回归及 26 个文件中的全部 148 个测试通过，生产构建通过。新增错误恢复仍属于组件证据，与已记录的原生正常保存/重置流程分开。三个 ignored GPU 样本在独立 surfaceless EGL 运行中显式通过：自建图片/GL 状态、泛光 halo/状态，以及本机原版资源的 bloom/WaterRipple/Iris/generic particle/填充方向/cutout 像素。此结果与 CPU 准备及原生桌面/UI 验收分开，不是原引擎完整画面 A/B。文档构建作为这些记录完成后的最终整合检查。

经验证的 bool/slider/color 编辑按项目和输出保存，与分配同处 `session.toml`。“保存并应用”事务为第一帧/commit 接纳 → 一次原子保存分配与属性 → Finalize 并启用音频。原子发布前的准备、候选接纳或保存失败保留原渲染器与选择；保存成功后的持久分配保持权威，延迟 Finalize 清理不能撤销它。普通保存/清除保留其他显示器分配和所有无关选择，清除最后一个分配也不删除已保存属性；无文件视为空状态，损坏或超限状态禁止覆写，诊断省略属性值。下述独立原生属性流程已通过。作者 schema 变化使形状合法的保存选择失效时，无快照的属性加载错误状态也提供重置默认值；它让 `{}` 通过默认准备，并仅在应用/保存成功后删除该显示器/项目选择，普通保存仍要求经验证的声明/值。不支持的默认值、无法读取或损坏的 session 仍明确报错。此新增错误恢复路径由组件回归验证；已记录原生流程覆盖正常保存/重置，不是独立作者 schema 变化场景的原生验收。

原生属性/失败/全屏流程私有记录为 `runtime/ui-365-properties-final/report.json`，真实 checkbox/number 输入与人工截图复核均通过。作者属性 `hdr` 从 true 改为 false 是关闭 **LDR 泛光**，不是启用 HDR 渲染；`newproperty2` 从 0 改为 0.1，对应作者音频层音量。仅把隔离 `config/lwe` 目录设为 0500 后，真实 Save 因 PermissionDenied 失败，session 字节/inode 未变，编辑草稿保留，原 Scene 仍 Running 且实际可见。恢复 0700 后用相同选择重试成功：完整 TOML 通过新 inode 原子替换，已打开 descriptor 仍读到旧值；新 Tauri 进程恢复保存的属性与真实控件。重置默认值删除 overrides，恢复所有作者值（包括 bloom=true）和原 session 字节哈希，并保留分配。

随后自有 Tauri 窗口完整覆盖逻辑 1728×1152 输出 12.000 秒，揭示后仍 Running，两份截图可见粒子变化；没有直接测量 callback/遮挡暂停，不能证明系统休眠或锁屏恢复。所有自有进程组停止，隔离配置删除，工作区 1 恢复，cleanupErrors 为空。通过的是有限属性/全屏流程，不代表原版视觉一致、24 小时运行、真实休眠/锁屏、硬件电池转换、功耗/VRAM 稳定或安装包验收。

原引擎对照尝试使用隔离的引擎副本和 Wine prefix；对照 UI 在获得可用截图前以 `c0000005` 失败，自有清理无错误、源文件哈希未变。这不是原引擎 A/B 结果。只读 11 项 CPU 复跑已私有保存为 `scene-prepare-corpus-2026-10-10-mixed.json`，包含二进制/inventory SHA-256、原包哈希、耗时、退出码及原始首项阻断。通过 `3258032485`、`3337606455` 和 `3659880761`，八项首阻断为五项 SceneScript、`2798622011` 相机、`2454418355` HDR 和 `3485147659` 图层数量上限；后者的 feature code 为 null，附精确图层数量验证错误。11 个原包均与 P0 一致。复用现有编译示例，没有构建、启动 backend/GL 或写 settings/session。3/11 CPU 结果不是 Workshop 兼容率或原生 UI 验收。有限 CPU/GPU/PCM 证据及已通过的默认原生 UI 流程须与尚未完成的原版视觉对照、24 小时运行、十次真实休眠恢复、硬件电池/交流电转换、功耗/VRAM 测量、独立多显示器及安装包验收分开，不能据此宣称日用就绪。

2026-10-09 验收及此前显示属性/泛光阻断的 UI 截图保留为历史证据。新增原始包混合 Scene 实现和有限 Probe 不扩大旧流程的验收边界，也不证明日用支持。

三个独立 GPU 像素/GL 状态样本可按以下方式重跑，无须应用桌面壁纸；原版资源覆盖需要本机默认 365 项目与已安装资源：

```bash
lwe_scene_project="$HOME/.local/share/Steam/steamapps/workshop/content/431960/3659880761"
lwe_scene_assets="$HOME/.local/share/Steam/steamapps/common/wallpaper_engine/assets"
for lwe_scene_gpu_test in \
  scene::renderer::tests::offscreen_pixels_and_gl_state_are_preserved \
  scene::postprocess::tests::offscreen_bloom_halo_threshold_tint_and_state \
  scene::postprocess::tests::offscreen_original_bloom_and_effect_shader_assets
do
  EGL_PLATFORM=surfaceless LWE_SCENE_TEST_PROJECT="$lwe_scene_project" \
    LWE_SCENE_TEST_ASSETS="$lwe_scene_assets" \
    cargo test --locked -p lwe-engine --lib "$lwe_scene_gpu_test" \
    -- --ignored --exact --test-threads=1 --nocapture
done
```

这些样本在可用 EGL/GPU context 上验证相应像素用例与 GL 状态恢复，不比较原引擎截图，也不证明 Wayland/多输出生命周期行为。

### Space 2026-10-10

原创意工坊 [2.5D Space（`2903458386`）](https://steamcommunity.com/sharedfiles/filedetails/?id=2903458386) 在不改写原包、不删减图层/特效的条件下，通过共用 CPU 准备与真实 LWE 应用：十个图片图层、十一个特效和二十四个 pass。八个 Scroll 实例、音频 Pulse/色差和四 pass 项目自带泛光保留作者顺序。该项目关闭通用泛光，但自定义泛光继续启用；项目资源保持高于本机 builtins 的优先级。

十四段未修改的标量脚本通过七十个独立数值用例，覆盖默认、最小、最大、零值与混合属性快照。Oxc 先检查模块 AST，再由有界 QuickJS 在 CPU worker 执行，渲染线程不运行脚本回调。十二个原始着色器程序以 GL 130 链接成功。真实桌面输出 monitor 采集另通过立体声低/高频、暂停/恢复、静音和清理检查；每通道十六档归一化频谱不能证明 Windows 分频或视觉一致。

私有 `space-runtime/gl-acceptance.json` 的二十六项 GPU 像素比较全部通过：八个图片层分别将不透明度设为零均改变超过一百万像素；六组 opacity、色差/Pulse/Bloom 开关、图层顺序和背景颜色按预期改变完整场景。原始着色器合成模式 0/30/31 通过各自像素检查。非零频谱下，暂停强制重绘及 resize 后回到原尺寸保持像素完全一致，恢复后接受新频谱。这些 EGL 检查证明相应作者管线和输入保留，不是 Windows 引擎截图对照。独立的生产 monitor 到桌面流程也已通过：Global Speed 设为零后，安静帧及停止声音后返回的帧完全一致；自有低音量立体声测试音改变壁纸，暂停时在声音变化期间保留该帧，恢复后再次响应声音。像素比较使用现有桌面部件以下 2160×1090 的壁纸区域；用户音频路由、配置、原始资产和工作区均保留。

真实原生 Tauri 流程保存在私有 `runtime/space-ui-delivery/report.json`，在记录的单输出 Wayland + `niri`/AMD 680M 机器上通过：Library/显示器 Apply 达到 Running；全部二十个作者控件（十六 slider、三 bool、一 color）修改并保存；隔离 PermissionDenied 保留旧分配/选择和草稿，重试原子替换完整会话。新的自有应用进程恢复已保存控件与壁纸，Reset 恢复全部作者默认值。Settings 15 → 30 FPS、十二秒自有全屏覆盖/揭示、目标清除及自有资源清理也通过。颜色使用已渲染控件的 input 事件，未操作系统颜色选择器。这个有限 UI 流程未测量硬件休眠/锁屏或遮挡 callback 策略。

最终 engine-only Probe 使用 30 FPS 上限并频繁查询状态：十二秒约 27.25 次完成 swap/s，稳定暂停十二秒 render attempts/draw/swap/empty frames/callback 均为零，恢复约 27.33/s。按图层尺寸共享的临时目标、命名目标和完整画面目标共计 202,180,100 逻辑 GPU 字节。配置上限不保证达到 30 FPS；统计不含驱动/EGL/视频开销，也不是实测 VRAM。Windows A/B、24 小时运行、十次真实休眠恢复、硬件电池转换、功耗测量、独立多显示器与安装包验收仍未完成。

### Yakuza 扩展 2026-10-12

本批在纯标量路径之外增加有界动态 SceneScript worker。接纳的生命周期为模块初始化、`init`、初次声明属性交付及重复 `update`；回调可用有限 `Vec3`/`copy`、声明控件和图层 `origin`/`scale`/`angles`/`color`/`alpha`/`visible`，`init` 返回值可更新所绑定属性。`createLayer(path)` 仅在初始化期间接受安全的图片模型 JSON，并经过正常共用准备/解码；渲染线程不运行 JavaScript VM。先保留作者对象 handle，再分配创建 handle；已验证 worker 状态先于首帧 ACK，暂停强制重绘保留已接纳状态，可逆应用/保存/Finalize 边界继续负责失败和回滚。该子集未实现 config/handle 重载、任意 clone、销毁、场景查询、Vec2、engine 时间、光标/媒体事件或通用交互。官方 [init](https://docs.wallpaperengine.io/en/scene/scenescript/reference/event/init.html)、[update](https://docs.wallpaperengine.io/en/scene/scenescript/reference/event/update.html) 和[场景 API](https://docs.wallpaperengine.io/en/scene/scenescript/reference/class/IScene.html) 的完整契约大于 LWE 当前接纳范围。

Oxc 在 QuickJS 前校验源码与 AST：最多 32 个绑定、每段 64 KiB/合计 256 KiB 源码、AST 深度 48、作者/创建对象 handle 合计 128、VM 内存 8 MiB、栈 256 KiB，以及初始化 100 ms/每帧执行 10 ms。原型/全局逃逸、未经验证的计算属性写入、导入、未知 API 与不支持输出严格阻断。运行异常、执行超时、worker 退出与取消须保留上次持久分配；CPU 测试覆盖有限规则，不能替代 GPU 或原生应用验收。

`registerAudioBuffers(64)` 仅允许模块全局注册，`init`、`update` 或属性回调中注册会被拒绝；其 `average` 是实际固定长度 `Float32Array`，由采集桌面输出后的真实 64 档变换就地刷新。十六档立体声 shader uniform 保持独立，扩展它们不能产生 64 个独立频档。LWE 目前仅接纳这一分辨率的 average 通道，输入不可用时静音，暂停/回滚保留已接纳输入与状态。官方[音频缓冲](https://docs.wallpaperengine.io/en/scene/scenescript/reference/class/AudioBuffers.html) 还有其他通道/分辨率；Windows 分频、幅度归一化和画面一致性仍未验证。

Combo 声明现保留作者标签及非空唯一字符串选项，默认值与全部覆盖值须属于声明选项；界面提供带可访问标签的 select，字符串保存至同一项目/输出选择快照。精确 `{name, condition}` combo 相等条件产生 bool 可见性；混合 script/user 包装先提取脚本，再验证剩余 user 条件。标量 shader 映射不消费 combo 字符串。原始 Yakuza 控件为 `av` 的 `av`/`0`、`pic` 的四个 variant（`Majima_1`、`Kiryu_1`、`YakuzaDragon`、`Majima_2`）及 `schemecolor`，无效声明/选择不得静默替换为默认值。

单图片 `TEXS0002` 粒子图集保留经验证的矩形 UV 帧、序列/随机帧选择及序列模式下受支持的相邻帧插值。带时长 `TEXS0003`、多图片图集及作为普通图片/特效纹理使用仍门控。新增平面粒子组件包括湍流初速度、随机透明度/角速度、角运动、透明度/尺寸振荡与湍流，有限模拟/几何/work 预算继续执行。保留的原生结构/组件证据支持特定字段，噪声/随机行为及原引擎完整粒子 A/B 尚未核验。

Yakuza 作者的 `perspective:true` 音频条仅进入共面固定正交子集：origin Z=0、无 X/Y 旋转及无相机运动/视差。保留的原二进制证据支持规范相机/投影行为，未确认逐层 perspective 分支是否附加其他映射。平面等价须标为数学推断，保留原引擎视觉一致性缺口。

共用 `genericimage2` 图片适配现保留缺省 VERSION 的旧版 Brightness/Alpha 及显式 VERSION 1/2 Color4，不增加 renderer API，也不放宽未知输入/combo 门控。下述最终 GPU 用例已验证原始图片 color/alpha/可见性，原引擎视觉一致性仍未核验；最终适配也在 VERSION 缺省时保留作者对象 color/alpha，修正原 owner 音频条的黑色色调且保留 handle；此前 Space VERSION 2 记录保留。

**已记录有限验收：**私有 `yakuza/cpu-prepare-report.json` 通过全部八组原始包组合：四个 `pic` 背景 × `av`/`0`。私有 `yakuza/gpu-report-final.json` 通过五组完整场景原生离屏 OpenGL 用例：每个背景开启音频条，另加默认背景关闭音频条。保留 64 个音频条 handle、63 个初始化创建层、原始 4×4 音频条材质/纹理、66–67 个渲染层与 67–68 个 pass。二十五项像素比较和十五轮 worker 销毁/重建均通过，包含原始图片 color、alpha=0 与隐藏断言；注入 64 档音频使每组开启音频条的隔离区域改变 8,070 像素，关闭时差异为零，二十次暂停强制重绘与 resize 后回原尺寸保留已接纳画面。注入输入的 GPU 证据不能证明实际采集路径。

私有 `yakuza/ui-run-2/report.json` 使用最终冻结构建，在单 `eDP-1`、Wayland + `niri`/AMD 680M 上通过真实 `mockIpc=false` Tauri Library/Desktop 流程。私有 `yakuza/visual-review-final.json` 记录完整默认与 Kiryu 场景、恢复控件及中文 Settings 布局画面复核；作者颜色标签仍为原始本地化 token，不代表作者标签已完整翻译。原生控件保存 `av=0`、`pic=Kiryu_1` 和颜色，真实隔离 PermissionDenied 保留 session 字节/inode、可编辑草稿及原 Running 渲染器。重试原子保存完整选择，新进程恢复选择，Reset 删除覆盖值并保留分配、恢复全部默认值。Settings 15 → 30 FPS、同 PID 关闭/隐藏、十二秒自有全屏恢复与目标清除通过；cleanupErrors 为空，工作区 1 已恢复。颜色使用已渲染 input 事件，未操作系统颜色选择器；私有 `yakuza/native-temporal-pixels-final.json` 对桌面部件下方（y≥300）区域进行精确 RGB8 像素比较，默认两帧差 1,297 像素，全屏恢复后两帧差 1,848，证明 Scene 区域存在时间变化，不把每个差异像素归因于某个组件；全屏恢复未测 callback 遮挡暂停或物理休眠。

私有 `yakuza/gpu-report-final-live-audio.json` 另通过真实 PulseAudio/PipeWire PCM 自有静音 null-sink monitor 采集 → 真实 64 档 FFT average → 未修改的原始脚本经 LWE worker → 原生离屏音频条 GPU 像素链路。渲染器为 AMD Radeon Graphics（`radeonsi`、`rembrandt`、ACO），OpenGL 4.6/Mesa 26.2.4。开始播放改变 1,285 个音频条像素，暂停断开/恢复重连后改变 690 像素；暂停期间已显示像素完全一致，停止播放后完全回到静音音频条画面，初始与释放后的 64 元素缓冲均为零。已核验采集源为自有 sink monitor，采集及脚本 worker 释放、自有 sink 卸载，默认 sink/source 未变。该结果证明隔离路由下真实采集/FFT/脚本/GPU 链路，使用离屏像素，不包含扬声器输出或默认设备采集，与已通过的可见桌面 UI 流程分开；不证明用户当前音乐/视频路由或 Windows FFT/视觉一致性。

最终源码/二进制身份私有保存在 `yakuza/final-source-identity.json`，记录的 125 个 Rust 源码/manifest/lock 文件均与冻结源码一致。最终 workspace fmt/clippy/check、466 项常规 Rust 测试及 shell 构建通过，此前前端 154 项测试与 Python 144 项回归也通过；源码检查与运行时验收分开。

最终 `yakuza/space-regression-gpu-report-final.json` 另通过本扩展后的全部二十六项 2.5D Space GPU 比较，未修改的原项目保留十层、十一个特效、二十四个 pass、二十个控件及 202,180,100 逻辑 GPU 字节，覆盖原透明度/特效/顺序/合成、注入立体声频谱及暂停重绘/resize 保留。这是新增确定性离屏 GPU 回归，保留旧 Space 验收记录，不代表另一次桌面/实际音频采集或 Windows 一致性验收。

**剩余验收：**保留原始文件哈希及私有证据，旧阻断结果作为带日期快照。用户明确延后物理休眠恢复，因为它会挂起 Codex，本批不执行，也不作为完成条件。旧 24 小时任务为释放显示器，通过核验自有 PID 后正常结束，证据保留；这是中断运行，不是长期验收通过。真实多显示器、安装包、Windows A/B、硬件电池/功耗、高级脚本、骨骼与 HDR 继续单独保留未验证或未支持状态；这些有限结果不能证明日用就绪。

## 报告问题

有效的问题报告应包含：

- 安装包类型（`lwe`、`lwe-git`、`.deb`、`.rpm` 或 `.AppImage`）；
- 发行版和版本；
- 会话类型和合成器/桌面环境；
- 显示器布局；
- 已知的壁纸类型（`video`、`scene` 或 `web`）；
- LWE 显示的兼容性状态；
- Settings 中可复制的诊断信息，其中 Steam Web API Key 会被隐藏；
- 可用的终端日志。

### 持久记录的场景可靠性验收

用户因休眠会挂起 Codex，明确将物理休眠恢复移出当前批次。下列不休眠记录器示例要求零次休眠，不能证明休眠恢复；完整未来恢复目标与辅助脚本单独保留。

使用 `scripts/scene-reliability.py` 启动已编译 `lwe-shell` 库测试二进制的固定副本，选择确切的忽略测试 `services::desktop_service::reliability_tests::desktop_scene_daily_use_reliability`。先执行 `cargo test --locked -p lwe-shell --lib --no-run --message-format=json`，从输出的 `compiler-artifact.executable` 取得路径。验收期间保留完整未提交源码指纹和原始输入。记录器隔离 `XDG_CONFIG_HOME`，Steam 发现仍保留 `HOME`。

设置 `LWE_REAL_DESKTOP_TESTS=1`，参数为 `--hours 24 --cycles 100 --suspend-cycles 0 --sample-seconds 5 --memory-growth-mib 64`，并提供实际 `--binary`、`--project`、`--assets`、`--video` 和 Git 外全新私有 `--output` 目录。固定样本集使用 `--scene-ids 2903458386,3659880761,3337606455,3258032485`，最后追加 `-- services::desktop_service::reliability_tests::desktop_scene_daily_use_reliability --ignored --exact --nocapture --test-threads=1`。首项是连续运行主样本，其余参加最终确认后的往返切换。记录器不会发起休眠。

未来任务明确包含物理恢复验收时，待 `suspend-readiness.json` 显示新鲜、健康的主场景后，先用 `scripts/scene-suspend-cycles.py --check --readiness PATH` 只读检查。RTC 真实循环需要获授权的本机特权执行，提供 `--readiness PATH --output-dir NEW_PRIVATE_HELPER_DIRECTORY --cycles 10`。脚本保留正常 logind inhibitor 和锁屏，等待正常密码认证解锁；不得覆盖已有 RTC 闹钟。凭证不得放入参数、环境、文件、报告或自动化提示。

已结束记录器的 `passed=true` 只证明实际记录的目标，`--suspend-cycles 0` 不能证明物理恢复；未来恢复验收另需真实休眠报告。`report.json`、`manifest.json` 和追加的 `evidence.jsonl` 区分实际呈现、完整切换、内核休眠、资源增长、帧耗时和清理。任务启动、mock/单测、经过一天、中断后重启或重编译源码均不算通过 24 小时验收。

可选的 `--finish-after-soak` 会在有效场景呈现与最终确认的完整往返达到目标后，正常清除、关闭并等待后端退出，即使单独要求的休眠目标尚未完成。此部分结果仍为 `passed=false`、记录器退出码 1，但 `status=completed`、子进程成功退出，并列明完成/未完成组件与 `stop_reason=soak_and_switch_targets_reached_suspend_incomplete`；它不是应自动重启的运行故障。该策略写入不可变 manifest，不能用 `--resume` 更改。中断记录实际收到的信号。首次完整验收可以通过；历史连续性片段仍各自独立，不得追加拼接成不中断的通过记录。
