# LWE 文档

LWE 是一个 Linux 桌面应用，用于浏览、管理、检查、导入并应用 Wallpaper Engine 内容。

它面向实际的 Linux 迁移工作流：把创意工坊发现能力放进桌面应用，在应用壁纸前展示兼容性，并提供以本地库为中心的日常使用体验。

## 从这里开始

- [安装](./guide/installation.md)：通过 AUR 或 GitHub Releases 安装。
- [快速开始](./guide/quick-start.md)：配置 Steam、导入内容并应用第一张壁纸。
- [使用指南](./guide/usage.md)：了解本地库、创意工坊、兼容性、设置和桌面行为。
- [故障排查](./guide/troubleshooting.md)：处理常见安装和运行问题。

## 贡献者文档

- [项目概览](./contributing/project.md)：产品范围、架构、发布模型和活跃路径。
- [v1 路线图](./contributing/roadmap.md)：从 v0.6.1 迈向 v1 的维护中里程碑。
- [贡献指南](./contributing/guide.md)：开发环境、检查项、文档政策和产品变更工作流。

## 当前支持范围

| 领域 | 状态 |
| --- | --- |
| 平台 | Linux 桌面应用 |
| 已测试会话 | Wayland + `niri` |
| 主要内容来源 | Steam 创意工坊中的 Wallpaper Engine 内容 |
| 首发运行时重点 | 视频类壁纸 |
| 兼容性报告 | 在可获取元数据时识别视频、场景和网页类壁纸 |
| 语言 | 英文和简体中文 |

场景类和网页类壁纸可能会出现在兼容性报告中，但除非应用明确显示支持，否则不应假定它们具备与视频类壁纸相同的运行时支持水平。
