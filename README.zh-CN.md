# PixelDraw

[English](README.md) | [简体中文](README.zh-CN.md)

让 AI 通过明确的坐标和受约束的 MARD 色表绘制像素画的本地 MCP 工具，而不是直接调用文生图模型。

Rust 程序负责执行绘图指令，MCP 客户端负责提供模型。PixelDraw 本身不需要 API Key，也不开放网络监听端口；模型服务的配置和费用由客户端负责。

## 功能介绍

在 MCP 客户端中描述想画的内容，AI 就可以创建画布，用直线/矩形/三角形/圆/椭圆/油漆桶或像素落点画出图形；画完可以出图看看效果，再继续修改和调整，满意后保存。

- **精确像素绘制：** 画布宽高均支持 1 至 128 像素，通过坐标控制每个落点。
- **固定色表：** 支持 24、144、221 色 MARD 色表，可以使用色号或当前色表的中文颜色名。
- **多尺寸笔刷：** 支持 1×1、2×2、4×4、8×8 方形笔刷，也可用作直线/描边线宽。
- **整批校验：** 格式、颜色、坐标或笔刷不合法时，整批指令被拒绝，不会只画上一部分。
- **即时预览：** 创建和绘制后返回最近邻放大 8 倍的 PNG，在支持图片工具结果的客户端中查看。
- **本地保存：** 可以指定输出目录；自动命名遇到重名会重试，显式同名报错，不覆盖已有作品。

适合制作小图标、像素插画，以及探索 AI 通过工具调用进行绘图的过程。它不是独立的 AI 模型、照片转像素画工具或完整图像编辑器。

## 效果展示

以下选用项目 `output/` 中已有的作品，复制到 `docs/images/`，供 GitHub 展示。导出的 PNG 为原始画布的 8 倍大小；示例不代表模型每次都会生成相同的作品。

| 可颂 | 海边日落 | 天坛 |
| --- | --- | --- |
| ![可颂像素画](docs/images/croissant-32x32.png) | ![海边日落像素画](docs/images/seaside-sunset-32x32.png) | ![天坛像素画](docs/images/temple-of-heaven-64x64.png) |
| 32×32 画布 | 32×32 画布 | 64×64 画布 |

## 安装

### 免编译安装（推荐）

打开 [最新 Release](https://github.com/HHpetra/PixelDraw/releases/latest)，展开 **Assets**，下载适合系统的安装包。不要选择 GitHub 自动生成的 **Source code** 源码压缩包。

| 系统 | 文件名后缀 |
| --- | --- |
| Windows x64 | `x86_64-pc-windows-msvc.zip` |
| Linux x64（glibc 2.35 及以上，例如 Ubuntu 22.04 及以上） | `x86_64-unknown-linux-gnu.tar.gz` |
| macOS Apple Silicon（ARM64，M 系列芯片） | `aarch64-apple-darwin.tar.gz` |
| macOS Intel（x64） | `x86_64-apple-darwin.tar.gz` |

解压到固定目录。Windows 程序为 `pixeldraw.exe`，Linux/macOS 为 `pixeldraw`，包内附使用文档。运行服务不需要安装 Rust、Cargo、Git 或额外的模型运行环境；AI 模型仍由 MCP 客户端提供。

Windows PowerShell 示例（路径按实际解压位置修改）：

```powershell
& "C:/Tools/PixelDraw/pixeldraw.exe" --version
& "C:/Tools/PixelDraw/pixeldraw.exe" --help
```

Linux/macOS 解压与检查示例（这里以 Apple Silicon 安装包为例）：

```sh
tar -xzf pixeldraw-v0.1.1-aarch64-apple-darwin.tar.gz
./pixeldraw-v0.1.1-aarch64-apple-darwin/pixeldraw --version
```

然后在 OpenCode 中配置程序的**绝对路径**，不再使用 `cargo run`：

```json
{
  "mcp": {
    "pixeldraw": {
      "type": "local",
      "command": ["C:/Tools/PixelDraw/pixeldraw.exe", "--output-dir", "C:/Pictures/PixelDraw"],
      "enabled": true
    }
  }
}
```

Linux/macOS 将程序路径改为实际位置，例如 `/home/you/tools/pixeldraw` 或 `/Users/you/tools/pixeldraw`，同时替换输出目录。配置完成后重启或重新加载 MCP 客户端，由客户端启动服务。双击程序不会出现绘图窗口，它是供 AI 调用的后台工具。

每个压缩包附有 `.sha256` 校验文件。PowerShell 使用 `Get-FileHash <压缩包> -Algorithm SHA256` 对照校验值；Linux 使用 `sha256sum -c <压缩包>.sha256`；macOS 使用 `shasum -a 256 -c <压缩包>.sha256`。压缩包与校验文件应放在同一目录。校验值用于验证文件完整性，不等于发布者身份签名。

程序尚未进行代码签名或 macOS 公证，系统可能显示安全提示。确认来自本仓库 Release 后，按系统提供的单文件批准流程处理，不要关闭系统级安全防护。

### 源码安装（开发者）

通过 [rustup](https://rustup.rs/) 安装 Rust 1.96.1 或更新版本，然后执行：

```sh
git clone https://github.com/HHpetra/PixelDraw.git
cd PixelDraw
cargo install --path . --locked
pixeldraw --help
```

程序安装到 Cargo 的 bin 目录，该目录需要在 PATH 中。从 v0.1.1 开始提供预编译安装包。CI 在 Windows、Linux 和 macOS 上运行，具体结果可查看仓库 Actions 页面。

## 连接 MCP 客户端

配置 stdio MCP 服务，启动命令为 `pixeldraw`，参数为 `--output-dir` 和输出目录。建议使用绝对路径；如果桌面客户端无法找到程序，也需要指定可执行文件的绝对路径。

OpenCode 配置示例：

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "pixeldraw": {
      "type": "local",
      "command": ["pixeldraw", "--output-dir", "/absolute/path/to/art"],
      "enabled": true
    }
  }
}
```

将示例目录替换为实际目录。Windows 可以使用 `C:/Users/you/Pictures/PixelDraw` 这样的路径。仓库中的 `opencode.json` 是从项目目录启动 Cargo 的开发配置。

自动化测试覆盖 MCP 通信流程，但不同客户端版本的配置方式和图片显示能力可能不同。

## 绘制第一张图

可以向 AI 提出：

> 请使用MCP工具绘制一幅像素画，内容为海边日落。尺寸为32*32

图元与精确描点都可用。画完可查看预览，再继续修改和调整。最小流程如下，花括号内为工具参数，而不是终端命令：

```text
create_canvas {"width":16,"height":16,"palette":"24"}
draw_circle   {"cx":8,"cy":8,"radius":5,"color":"C4","fill":true}
draw_rect     {"x0":2,"y0":2,"x1":13,"y1":13,"color":"A4","fill":false,"brush":2}
save_image    {"filename":"first-icon.png"}
```

这会画一个填充圆和一圈描边，输出一张 128×128 的 PNG。工具返回文字和 PNG 图片内容，需要支持图片工具结果的客户端才能直接预览。

## 工具与约束

| 工具 | 功能 |
| --- | --- |
| `create_canvas` | 创建白底画布，宽高 1 至 128；锁定 24、144 或默认的 221 色表。成功重建会替换旧画布。 |
| `list_colors` | 查询当前锁定的色表；尚未创建画布时可以指定要查询的色表。仅在需要查色时调用。 |
| `draw_line` | 画直线；`brush` 为线宽 1/2/4/8，坐标不必对齐网格。 |
| `draw_rect` | 矩形；`fill` 区分填充/描边，描边可用 `brush` 加粗。 |
| `draw_triangle` | 三角形；`fill` 区分填充/描边，描边可用 `brush` 加粗。 |
| `draw_circle` | 圆；`fill` 区分填充/描边，描边可用 `brush` 加粗。 |
| `draw_ellipse` | 椭圆；`fill` 区分填充/描边，描边可用 `brush` 加粗。 |
| `flood_fill` | 油漆桶，四连通替换种子点同色区域。 |
| `draw_batch` | 多笔有序批量，原子执行；任一步失败整批拒绝，只回最终预览。 |
| `draw_pixels` | 散点/方块笔刷落点：空白分隔的 `x y 颜色` 三元组。 |
| `save_image` | 将当前画布导出为放大 8 倍的 PNG，不覆盖已有文件。 |

- 左上角为 `(0,0)`，向右为 X 正方向，向下为 Y 正方向。
- 多笔请用 `draw_batch`，不要并行调用多个绘制工具。画完看预览，可继续修改和调整。
- 线宽/描边笔刷为 1、2、4 或 8，以落点为中心扩展；填充与油漆桶忽略笔刷。
- `draw_pixels` 的笔刷坐标必须是笔刷尺寸的倍数，整个方块都必须在画布内。
- 重复落点按输入顺序覆盖；非法批次不会修改画布。
- 圆/椭圆半径可超出画布，超出部分自动裁剪；端点、圆心、种子点必须在画布内。
- 每个服务实例只有一张内存画布；重启会丢失未保存内容。暂不支持撤销、导入、会话持久化或原始尺寸导出。
- 所有图片都通过最近邻采样放大 8 倍。24 色旧版色表的部分名称和 RGB 值与大色表不同，不应跨色表假定颜色一致。

## 输出与排错

`--output-dir PATH` 指定保存目录，默认是服务运行目录下的 `./output`，不是源码或可执行文件所在目录。请使用自己控制且可写的目录；本地工具不是文件系统沙箱。

文件名只接受 ASCII 字母、数字、点、下划线和连字符；补全 `.png` 后最多 200 字节。路径、尾点和 Windows 设备名会被拒绝。省略 `filename` 可自动命名并重试冲突；显式文件名已存在时返回错误。

直接在终端启动后看似没有反应是正常现象：服务正在等待标准输入上的 MCP 消息。日志写入标准错误。保存失败时检查目录权限；颜色被拒绝时先调用 `list_colors`。需要诊断日志时可设置环境变量 `RUST_LOG=debug`。

## 开发与许可

开发检查见 [CONTRIBUTING.md](CONTRIBUTING.md)，完整绘制说明与色表见 [AGENTS.md](AGENTS.md)。代码采用 [MIT 许可证](LICENSE)，色表来源及第三方权利范围见 [THIRD_PARTY.md](THIRD_PARTY.md)。

仓库只收录精选展示图片；本地参考照片、截图和其余 `output/` 内容不随文档发布。
