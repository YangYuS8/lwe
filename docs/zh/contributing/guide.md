# 贡献指南

## 开发环境

安装核心工具链：

- 推荐 Node.js 24；当且仅当前端工具链支持时，Node.js 20 或更新版本也可能可用；
- pnpm；
- Rust stable；
- 当前发行版所需的 Tauri 2 平台依赖。

安装依赖：

```bash
pnpm install
```

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

对于仅文档变更，`pnpm docs:build` 是必需验证项。

## 发布冒烟清单

发布稳定版或预发布标签前，先验证该标签将触发的发布路径：

1. 如果变更范围较大，在本地运行等价的必需检查：`pnpm check`、`pnpm test`、`pnpm build`、`pnpm docs:build`、`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo check --workspace` 和 `cargo test --workspace`。
2. 确认待打标签提交上的 GitHub Actions 质量检查通过。
3. 确认发布 workflow 会构建并上传预期 Linux 制品：`.deb`、`.rpm` 和 `.AppImage`。
4. 确认包渠道元数据匹配发布渠道：稳定标签使用 AUR stable（`lwe`），开发/预发布路径使用 AUR git（`lwe-git`）。
5. 对影响运行时的发布候选版，在写成受支持前，必须在已验证的 Wayland + `niri` 会话上运行下面的真实桌面运行时验收清单。
6. 条件允许时执行新安装冒烟流程：安装一个包制品，打开 Settings，生成诊断信息，检查 Workshop/Library 状态，应用一张兼容视频壁纸，清除它，重启 LWE，并确认保存分配行为可见。

包安装成功只表示应用可从该包启动；它不保证未验证的合成器、GPU/EGL 栈、显示器布局或壁纸类型具备运行时支持。

## 真实桌面运行时验收

运行时变更在写成“已支持”前，必须先在真实受支持桌面会话上验证。当前已验证目标是 Wayland + `niri`，内容类型是视频类壁纸。

会话恢复使用 LWE 会话文件：当设置了 `XDG_CONFIG_HOME` 时为 `$XDG_CONFIG_HOME/lwe/session.toml`，否则为 `$HOME/.config/lwe/session.toml`。测试恢复行为时，如果 Desktop 页面显示过期的保存分配，请检查或移除该文件。

运行时变更请使用以下清单：

1. 在 LWE 中发现至少一个活动显示器；
2. 从本地库把一张兼容的视频壁纸应用到一个显示器；
3. 确认桌面可见变化；
4. 如果存在多个显示器，把壁纸应用到第二个显示器，然后只清除其中一个显示器；
5. 确认清除一个显示器不会停止其他显示器上的壁纸；
6. 重启 LWE，确认已保存分配能恢复，或恢复失败能在 Desktop 页面中显示；
7. 清除所有分配，并确认保存的会话不会再次恢复它们。

如果某个运行时步骤失败，请保留终端日志中指出失败阶段的行：后端启动、输出发现、首帧应用、按显示器清除或启动时恢复。这些信息是区分缺失视频资源、输出不匹配、Wayland layer-shell/EGL 失败和后端超时的受支持方式。

Rust 测试套件中的真实桌面测试需要显式启用，因为它们依赖当前合成器、显示器布局、GPU/EGL 栈、Steam 创意工坊内容和本地视频资源。请只在已验证机器上运行：

```bash
LWE_REAL_DESKTOP_TESTS=1 cargo test -p lwe-shell desktop_apply_flow -- --nocapture
```

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
