# 安装

本指南说明如何安装 LWE，以及首次启动前需要准备什么。

## 使用前提

LWE 使用 Steam 创意工坊中的 Wallpaper Engine 内容。使用创意工坊相关功能前，请确认：

- 你的 Steam 账号拥有 Wallpaper Engine；
- Linux 桌面上已安装 Steam 客户端并已登录；
- Steam 中已安装 Wallpaper Engine；
- 当前桌面会话支持你要测试的运行路径。

目前已验证的桌面环境是 Wayland 会话配合 `niri`。其他合成器或桌面环境可能可用，但在完成验证前应视为未验证环境。

## Arch Linux AUR

项目发布两个 AUR 包：

| 包名 | 渠道 | 适用场景 |
| --- | --- | --- |
| `lwe` | 稳定版 | 需要最新稳定版本。 |
| `lwe-git` | 预发布/开发版 | 需要来自活跃开发分支的更新构建。 |

可使用 `yay` 等 AUR 助手安装稳定版：

```bash
yay -S lwe
```

如果你明确需要预发布变更，可安装开发版：

```bash
yay -S lwe-git
```

## GitHub Releases

稳定版和预发布版本会通过 GitHub Actions 发布 Linux 安装包。请在仓库 Releases 页面下载适合你发行版的包。

发布的构建产物包括：

- `.deb`
- `.rpm`
- `.AppImage`

`.deb` 或 `.rpm` 文件请使用发行版常规包管理工具安装。AppImage 构建需要先添加可执行权限再启动。

```bash
chmod +x LWE*.AppImage
./LWE*.AppImage
```

## 从源码构建

当你需要贡献代码或验证本地改动时，请使用该路径。

需要的工具：

- Node.js 20 或更新版本
- pnpm
- Rust stable 工具链
- 当前发行版所需的 Tauri 2 构建依赖

安装 JavaScript 依赖：

```bash
pnpm install
```

运行前端检查：

```bash
pnpm check
pnpm test
```

运行 Rust 检查：

```bash
cargo check --workspace
```

本地应用开发请使用仓库中的 Tauri/Svelte 入口。活跃桌面应用路径是 `src-tauri`，前端位于 `src`。

## 安装后

继续阅读[快速开始](./quick-start.md)，配置 Steam Web API 密钥并应用第一张壁纸。
